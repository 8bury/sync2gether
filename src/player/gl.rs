//! A API render do mpv roda somente com o contexto GL da janela corrente.
//! Nenhum comando síncrono de controle é executado nesta thread.
use super::{Core, Event, Param};
use crate::diagnostics::{self, Stage};
use eframe::{
    egui, egui_glow,
    glow::{self, HasContext},
};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

type ProcAddress = Arc<dyn Fn(&CStr) -> *const c_void + Send + Sync>;
struct Pending {
    core: Arc<Core>,
    ready: mpsc::Sender<Result<(), String>>,
    events: tokio::sync::mpsc::Sender<Event>,
}
#[derive(Clone)]
pub(crate) struct Bridge {
    pending: Arc<Mutex<Option<Pending>>>,
    ctx: egui::Context,
}
impl Bridge {
    pub(super) fn initialize(
        &self,
        core: Arc<Core>,
        events: tokio::sync::mpsc::Sender<Event>,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        let (ready, rx) = mpsc::channel();
        *self.pending.lock().unwrap() = Some(Pending {
            core,
            ready,
            events,
        });
        self.ctx.request_repaint();
        while !stop.load(Ordering::Acquire) {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.pending.lock().unwrap().take();
        Err("Inicialização do player encerrada".into())
    }
}
#[repr(C)]
struct InitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ctx: *mut c_void,
}
#[repr(C)]
struct Fbo {
    id: c_int,
    width: c_int,
    height: c_int,
    format: c_int,
}
struct Wake {
    ctx: egui::Context,
    changed: AtomicBool,
}
unsafe extern "C" fn wake(data: *mut c_void) {
    // SAFETY: o Box vive até depois da remoção do callback e liberação do contexto.
    let wake = unsafe { &*data.cast::<Wake>() };
    diagnostics::callback(true);
    wake.changed.store(true, Ordering::Release);
    wake.ctx.request_repaint();
    diagnostics::callback(false);
}
unsafe extern "C" fn get_proc_address(data: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: o Box com o loader e o nome C vivem durante as chamadas de libmpv.
    let loader = unsafe { &*data.cast::<ProcAddress>() };
    loader(unsafe { CStr::from_ptr(name) }).cast_mut()
}
struct Target {
    fbo: glow::Framebuffer,
    color: glow::Renderbuffer,
    size: [i32; 2],
}
struct State {
    bridge: Bridge,
    gl: Arc<glow::Context>,
    loader: Box<ProcAddress>,
    wake: Box<Wake>,
    core: Option<Arc<Core>>,
    context: usize,
    target: Option<Target>,
    events: Option<tokio::sync::mpsc::Sender<Event>>,
    failed: bool,
    destroyed: bool,
}
/// Renderiza direto na GPU. Liberar com `destroy` em `eframe::App::on_exit`.
/// Criação, pintura e liberação usam exclusivamente a thread GL do eframe.
pub struct GlRenderer {
    state: Arc<Mutex<State>>,
}
impl GlRenderer {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Result<(Self, Bridge), String> {
        let gl = cc.gl.clone().ok_or("Contexto OpenGL indisponível")?;
        let loader = cc
            .get_proc_address
            .clone()
            .ok_or("Loader OpenGL indisponível")?;
        if loader(c"glBlitFramebuffer").is_null() {
            return Err("OpenGL sem suporte a framebuffer blit".into());
        }
        let bridge = Bridge {
            pending: Arc::default(),
            ctx: cc.egui_ctx.clone(),
        };
        let state = State {
            bridge: bridge.clone(),
            gl,
            loader: Box::new(loader),
            wake: Box::new(Wake {
                ctx: cc.egui_ctx.clone(),
                changed: AtomicBool::new(true),
            }),
            core: None,
            context: 0,
            target: None,
            events: None,
            failed: false,
            destroyed: false,
        };
        Ok((
            Self {
                state: Arc::new(Mutex::new(state)),
            },
            bridge,
        ))
    }
    /// Inicializa o mpv com GL corrente, mesmo nas telas sem área de vídeo.
    /// Não renderiza frames nem cria um framebuffer para o filme.
    pub fn prepare(&self, ui: &egui::Ui) {
        let state = self.state.clone();
        ui.painter().add(egui::Shape::Callback(egui::PaintCallback {
            rect: egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(1.0, 1.0)),
            callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
                diagnostics::mark(Stage::PaintLock);
                let mut state = state.lock().unwrap();
                if !state.destroyed && !state.failed && state.context == 0 {
                    state.initialize();
                    // SAFETY: este callback possui o contexto GL corrente.
                    unsafe {
                        state
                            .gl
                            .bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
                    }
                }
                diagnostics::mark(Stage::PaintDone);
            })),
        }));
    }
    pub fn callback(&self) -> egui::PaintCallback {
        let state = self.state.clone();
        egui::PaintCallback {
            rect: egui::Rect::NOTHING,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                diagnostics::mark(Stage::PaintLock);
                state
                    .lock()
                    .unwrap()
                    .paint(info, painter.intermediate_fbo());
                diagnostics::mark(Stage::PaintDone);
            })),
        }
    }
    pub fn destroy(&self) {
        self.state.lock().unwrap().destroy();
    }
}
impl State {
    fn initialize(&mut self) {
        diagnostics::mark(Stage::GlInitialize);
        let pending = self.bridge.pending.lock().unwrap().take();
        let Some(pending) = pending else { return };
        let api = &pending.core.api;
        let mut context = std::ptr::null_mut();
        let mut init = InitParams {
            get_proc_address,
            ctx: (&mut *self.loader as *mut ProcAddress).cast(),
        };
        // eframe pode suspender a pintura de janelas ocultas/minimizadas.
        // ADVANCED_CONTROL exige atender update mesmo sem frames; habilitá-lo
        // aqui pode bloquear o core enquanto não houver callback de pintura.
        // O padrão mantém a decodificação independente das atualizações GL.
        let mut params = [
            Param::new(1, c"opengl".as_ptr().cast_mut()),
            Param::new(2, &mut init),
            Param::end(),
        ];
        // SAFETY: GL corrente; core inicializado na thread de controle. Nenhum
        // comando de reprodução é processado antes da confirmação `ready`.
        let result = unsafe {
            reset_gl(&self.gl);
            (api.render_create)(
                &mut context,
                pending.core.handle as *mut c_void,
                params.as_mut_ptr(),
            )
        };
        if result < 0 {
            self.failed = true;
            let _ = pending.ready.send(Err(
                "Não foi possível iniciar o renderizador OpenGL do vídeo".into(),
            ));
            return;
        }
        unsafe {
            (api.callback)(context, Some(wake), (&mut *self.wake as *mut Wake).cast());
        }
        self.context = context as usize;
        self.events = Some(pending.events);
        self.core = Some(pending.core);
        let _ = pending.ready.send(Ok(()));
    }
    fn paint(&mut self, info: egui::PaintCallbackInfo, destination: Option<glow::Framebuffer>) {
        if self.destroyed || self.failed {
            return;
        }
        if self.context == 0 {
            self.initialize();
        }
        if self.context == 0 {
            return;
        }
        let viewport = info.viewport_in_pixels();
        let size = [viewport.width_px.max(1), viewport.height_px.max(1)];
        // SAFETY: callback executado com GL corrente na mesma thread de criação.
        let result = unsafe { self.render(size, info, destination) };
        if let Err(error) = result {
            self.failed = true;
            if let Some(events) = &self.events {
                let _ = events.try_send(Event::Unavailable(error));
            }
        }
        // egui restaura programa/VAO/blend depois do callback; o destino é nosso.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, destination);
        }
    }
    unsafe fn render(
        &mut self,
        size: [i32; 2],
        info: egui::PaintCallbackInfo,
        destination: Option<glow::Framebuffer>,
    ) -> Result<(), String> {
        unsafe {
            let resized = self.target.as_ref().is_none_or(|t| t.size != size);
            if resized {
                self.free_target();
                let fbo = self.gl.create_framebuffer()?;
                let color = match self.gl.create_renderbuffer() {
                    Ok(color) => color,
                    Err(error) => {
                        self.gl.delete_framebuffer(fbo);
                        return Err(error);
                    }
                };
                self.target = Some(Target { fbo, color, size });
                self.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
                self.gl
                    .renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, size[0], size[1]);
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                self.gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::RENDERBUFFER,
                    Some(color),
                );
                if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("Framebuffer do vídeo indisponível".into());
                }
                self.gl.bind_renderbuffer(glow::RENDERBUFFER, None);
            }
            let core = self.core.as_ref().unwrap();
            let context = self.context as *mut c_void;
            let changed = self.wake.changed.swap(false, Ordering::AcqRel);
            diagnostics::mark(Stage::GlUpdate);
            let update = changed && (core.api.update)(context) & 1 != 0;
            let target = self.target.as_ref().unwrap();
            if resized || update {
                reset_gl(&self.gl);
                let mut fbo = Fbo {
                    id: target.fbo.0.get() as c_int,
                    width: size[0],
                    height: size[1],
                    format: glow::RGBA8 as c_int,
                };
                let mut flip = 1_i32;
                let mut block = 0_i32;
                let mut params = [
                    Param::new(3, &mut fbo),
                    Param::new(4, &mut flip),
                    Param::new(12, &mut block),
                    Param::end(),
                ];
                diagnostics::mark(Stage::GlRender);
                if (core.api.render)(context, params.as_mut_ptr()) < 0 {
                    return Err("Erro ao renderizar o vídeo por OpenGL".into());
                }
            }
            // Mantém a imagem na GPU entre frames e reapresenta ao desenhar menus.
            diagnostics::mark(Stage::GlBlit);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, destination);
            let clip = info.clip_rect_in_pixels();
            self.gl.enable(glow::SCISSOR_TEST);
            self.gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            let viewport = info.viewport_in_pixels();
            self.gl.blit_framebuffer(
                0,
                0,
                size[0],
                size[1],
                viewport.left_px,
                viewport.from_bottom_px,
                viewport.left_px + size[0],
                viewport.from_bottom_px + size[1],
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
        }
        Ok(())
    }
    unsafe fn free_target(&mut self) {
        if let Some(target) = self.target.take() {
            unsafe {
                self.gl.delete_renderbuffer(target.color);
                self.gl.delete_framebuffer(target.fbo);
            }
        }
    }
    fn destroy(&mut self) {
        diagnostics::mark(Stage::GlDestroy);
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        if let Some(pending) = self.bridge.pending.lock().unwrap().take() {
            let _ = pending.ready.send(Err("Janela encerrada".into()));
        }
        // SAFETY: on_exit ainda tem o mesmo GL corrente. Liberação antecede o core.
        unsafe {
            if let Some(core) = &self.core {
                (core.api.callback)(self.context as *mut c_void, None, std::ptr::null_mut());
                (core.api.render_free)(self.context as *mut c_void);
            }
            self.free_target();
        }
        self.context = 0;
        self.core = None;
    }
}
unsafe fn reset_gl(gl: &glow::Context) {
    // Estado padrão exigido por render_gl.h. egui restaura o próprio estado depois.
    unsafe {
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::BLEND);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.color_mask(true, true, true, true);
        gl.use_program(None);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }
}
