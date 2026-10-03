//! libmpv carregada em execução. Controle separado do renderizador da janela.
//! A FFI abaixo segue client.h e render.h, ABI libmpv >= 2.
pub(crate) mod gl;
pub use gl::GlRenderer;
mod tracks;
use libloading::Library;
use std::{
    ffi::{CString, c_char, c_int, c_ulong, c_void},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
pub use tracks::{Track, TrackKind};

pub const WIDTH: usize = 960;
pub const HEIGHT: usize = 540;
pub type Frames = Arc<Mutex<Option<Vec<u8>>>>;
#[derive(Clone, Debug)]
pub struct Status {
    pub generation: u64,
    pub measured_ms: f64,
    pub speed: f64,
    pub seeking: bool,
    pub loaded: bool,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub blocked: bool,
    pub tracks: Vec<Track>,
    pub ended: bool,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            generation: 0,
            measured_ms: 0.0,
            speed: 1.0,
            seeking: false,
            loaded: false,
            position: 0.0,
            duration: 0.0,
            paused: true,
            blocked: false,
            tracks: Vec::new(),
            ended: false,
        }
    }
}
impl Status {
    pub fn position_at(&self, local_ms: f64) -> f64 {
        (self.position
            + if self.paused || self.seeking || self.blocked {
                0.0
            } else {
                ((local_ms - self.measured_ms) / 1000.0).max(0.0) * self.speed
            })
        .min(self.duration)
    }
}
#[derive(Debug)]
pub enum Event {
    Status(Status),
    Prepared { id: u64 },
    Started { id: u64, at: Instant },
    Missed { id: u64 },
    Error(String),
    Unavailable(String),
}
pub enum Command {
    Load(PathBuf, u64),
    Pause(bool),
    Seek(f64),
    Prepare { id: u64, position: f64 },
    Schedule { id: u64, at: Instant },
    Speed(f64),
    Volume(f64),
    Audio,
    Subtitle,
    AudioTrack(i64),
    SubtitleTrack(Option<i64>),
    Shutdown,
}

#[repr(C)]
struct MpvEvent {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}
#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}
impl Param {
    fn end() -> Self {
        Self {
            kind: 0,
            data: std::ptr::null_mut(),
        }
    }
    fn new<T>(kind: c_int, value: *mut T) -> Self {
        Self {
            kind,
            data: value.cast(),
        }
    }
}

struct Api {
    _library: Library,
    version: unsafe extern "C" fn() -> c_ulong,
    create: unsafe extern "C" fn() -> *mut c_void,
    initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    destroy: unsafe extern "C" fn(*mut c_void),
    option: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int,
    get: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int,
    free_node: unsafe extern "C" fn(*mut tracks::Node),
    wait: unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent,
    render_create: unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut Param) -> c_int,
    render: unsafe extern "C" fn(*mut c_void, *mut Param) -> c_int,
    update: unsafe extern "C" fn(*mut c_void) -> u64,
    callback:
        unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render_free: unsafe extern "C" fn(*mut c_void),
}
impl Api {
    fn load() -> Result<Arc<Self>, String> {
        // SAFETY: símbolos têm as assinaturas públicas de libmpv e a Library vive
        // até depois da destruição do renderer e do handle.
        unsafe {
            let library = Library::new("libmpv.so.2")
                .or_else(|_| Library::new("libmpv.so"))
                .map_err(|_| {
                    "libmpv não encontrada. Instale mpv no Arch ou libmpv2 no Ubuntu.".to_owned()
                })?;
            macro_rules! symbol {
                ($name:literal) => {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|_| "libmpv incompatível".to_owned())?
                };
            }
            Ok(Arc::new(Self {
                version: symbol!("mpv_client_api_version"),
                create: symbol!("mpv_create"),
                initialize: symbol!("mpv_initialize"),
                destroy: symbol!("mpv_terminate_destroy"),
                option: symbol!("mpv_set_option_string"),
                command: symbol!("mpv_command"),
                get: symbol!("mpv_get_property"),
                free_node: symbol!("mpv_free_node_contents"),
                wait: symbol!("mpv_wait_event"),
                render_create: symbol!("mpv_render_context_create"),
                render: symbol!("mpv_render_context_render"),
                update: symbol!("mpv_render_context_update"),
                callback: symbol!("mpv_render_context_set_update_callback"),
                render_free: symbol!("mpv_render_context_free"),
                _library: library,
            }))
        }
    }
    fn command(&self, handle: *mut c_void, args: &[&str]) -> Result<(), String> {
        let strings: Result<Vec<_>, _> = args.iter().map(|s| CString::new(*s)).collect();
        let strings = strings.map_err(|_| "Argumento inválido para o player".to_owned())?;
        let mut pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
        pointers.push(std::ptr::null());
        // SAFETY: argumentos terminados em NUL permanecem vivos durante a chamada.
        if unsafe { (self.command)(handle, pointers.as_ptr()) } < 0 {
            Err("O player recusou o comando".into())
        } else {
            Ok(())
        }
    }
    fn double(&self, handle: *mut c_void, name: &std::ffi::CStr) -> f64 {
        let mut value = 0.0_f64;
        // SAFETY: MPV_FORMAT_DOUBLE = 5, apontador válido para f64.
        unsafe {
            (self.get)(handle, name.as_ptr(), 5, (&mut value as *mut f64).cast());
        }
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    }
    fn flag(&self, handle: *mut c_void, name: &std::ffi::CStr) -> bool {
        let mut value: c_int = 0;
        // SAFETY: MPV_FORMAT_FLAG = 3, apontador válido para int.
        unsafe {
            (self.get)(handle, name.as_ptr(), 3, (&mut value as *mut c_int).cast());
        }
        value != 0
    }
}

unsafe extern "C" fn wake(data: *mut c_void) {
    // SAFETY: callback é removido antes de liberar o Arc que fornece o endereço.
    unsafe { &*data.cast::<AtomicBool>() }.store(true, Ordering::Release);
}
fn renderer(
    api: Arc<Api>,
    handle: usize,
    frames: Frames,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<Result<(), String>>,
    events: tokio::sync::mpsc::Sender<Event>,
) {
    // SAFETY: somente esta thread usa o render context. O handle permanece vivo
    // até a thread terminar. Chamadas de controle ocorrem em outra thread.
    unsafe {
        let mut context = std::ptr::null_mut();
        let mut advanced: c_int = 1;
        let mut params = [
            Param::new(1, c"sw".as_ptr().cast_mut()),
            Param::new(10, &mut advanced),
            Param::end(),
        ];
        if (api.render_create)(&mut context, handle as *mut c_void, params.as_mut_ptr()) < 0 {
            let _ = ready.send(Err("Não foi possível criar o renderizador libmpv".into()));
            return;
        }
        let changed = Arc::new(AtomicBool::new(true));
        (api.callback)(context, Some(wake), Arc::as_ptr(&changed).cast_mut().cast());
        let _ = ready.send(Ok(()));
        // u32 garante o alinhamento exigido por rgb0.
        let mut pixels = vec![0_u32; WIDTH * HEIGHT];
        while !stop.load(Ordering::Acquire) {
            if changed.swap(false, Ordering::AcqRel) && (api.update)(context) & 1 != 0 {
                let mut size = [WIDTH as c_int, HEIGHT as c_int];
                let mut stride = WIDTH * 4;
                let mut params = [
                    Param::new(17, size.as_mut_ptr()),
                    Param::new(18, c"rgb0".as_ptr().cast_mut()),
                    Param::new(19, &mut stride),
                    Param::new(20, pixels.as_mut_ptr()),
                    Param::end(),
                ];
                if (api.render)(context, params.as_mut_ptr()) < 0 {
                    let _ = events.try_send(Event::Unavailable("Erro ao desenhar vídeo".into()));
                    break;
                }
                let mut bytes = Vec::with_capacity(WIDTH * HEIGHT * 4);
                for pixel in &pixels {
                    let mut rgba = pixel.to_ne_bytes();
                    rgba[3] = 255;
                    bytes.extend_from_slice(&rgba);
                }
                *frames.lock().unwrap() = Some(bytes);
            }
            thread::sleep(Duration::from_millis(4));
        }
        (api.callback)(context, None, std::ptr::null_mut());
        (api.render_free)(context);
    }
}

// O último proprietário libera o core somente depois de liberar o renderer.
struct Core {
    api: Arc<Api>,
    handle: usize,
}
impl Drop for Core {
    fn drop(&mut self) {
        // SAFETY: nenhum renderer ou controlador usa o handle após o último Arc.
        unsafe { (self.api.destroy)(self.handle as *mut c_void) };
    }
}

pub struct Player {
    pub commands: mpsc::Sender<Command>,
    pub frames: Frames,
    join: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}
impl Player {
    #[cfg(test)]
    pub(crate) fn test_channel() -> (Self, mpsc::Receiver<Command>) {
        let (commands, receiver) = mpsc::channel();
        (
            Self {
                commands,
                frames: Frames::default(),
                join: None,
                stop: Arc::new(AtomicBool::new(false)),
            },
            receiver,
        )
    }
    pub fn start(events: tokio::sync::mpsc::Sender<Event>, origin: Instant) -> Self {
        Self::start_with_renderer(events, origin, None)
    }
    pub(crate) fn start_with_renderer(
        events: tokio::sync::mpsc::Sender<Event>,
        origin: Instant,
        gl: Option<gl::Bridge>,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let control_stop = stop.clone();
        let frames = Frames::default();
        let output = frames.clone();
        let join = thread::spawn(move || {
            if let Err(error) = run(rx, output, &events, origin, gl, control_stop) {
                let _ = events.try_send(Event::Unavailable(error));
            }
        });
        Self {
            commands,
            frames,
            join: Some(join),
            stop,
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
fn run(
    rx: mpsc::Receiver<Command>,
    frames: Frames,
    events: &tokio::sync::mpsc::Sender<Event>,
    origin: Instant,
    gl: Option<gl::Bridge>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let api = Api::load()?;
    // SAFETY: handle pertence à thread de controle; renderização usa apenas API
    // render. Destruímos o renderer antes de terminar o handle.
    let handle = unsafe { (api.create)() };
    if handle.is_null() {
        return Err("Não foi possível iniciar libmpv".into());
    }
    let core = Arc::new(Core {
        api: api.clone(),
        handle: handle as usize,
    });
    (|| {
        for (name, value) in [
            (c"vo", c"libmpv"),
            (c"config", c"no"),
            (c"terminal", c"no"),
            (c"input-default-bindings", c"no"),
            (c"input-vo-keyboard", c"no"),
            (c"keep-open", c"yes"),
            (c"pause", c"yes"),
            (c"hwdec", if gl.is_some() { c"auto-copy" } else { c"no" }),
            (c"video-timing-offset", c"0"),
        ] {
            // SAFETY: strings C estáticas, handle válido.
            if unsafe { (api.option)(handle, name.as_ptr(), value.as_ptr()) } < 0 {
                return Err("Erro de configuração libmpv".into());
            }
        }
        if gl.is_none() {
            // Os testes por software usam dois players em runners pequenos.
            // Evita que cada decoder crie threads para todos os CPUs do host.
            // A janela mantém o paralelismo automático do libmpv.
            // SAFETY: opção C estática, handle válido e ainda não inicializado.
            if unsafe { (api.option)(handle, c"vd-lavc-threads".as_ptr(), c"2".as_ptr()) } < 0 {
                return Err("Erro de configuração do decoder por software".into());
            }
        }
        // libmpv 0.37, API 2.2 do Ubuntu 24.04, produziu vídeo preto com
        // rgba16f em Mesa/llvmpipe. RGBA16 inteiro passou pelos mesmos testes.
        // APIs novas continuam usando a escolha automática do mpv.
        // SAFETY: função sem argumentos e opção C estática; core não inicializado.
        if gl.is_some()
            && unsafe { (api.version)() } < ((2 << 16) | 3)
            && unsafe { (api.option)(handle, c"fbo-format".as_ptr(), c"rgba16".as_ptr()) } < 0
        {
            return Err("Erro de configuração do framebuffer compatível".into());
        }
        if unsafe { (api.initialize)(handle) } < 0 {
            return Err("Erro ao inicializar libmpv".into());
        }
        let render = if let Some(bridge) = gl {
            bridge.initialize(core.clone(), events.clone(), &stop)?;
            None
        } else {
            let (ready_tx, ready_rx) = mpsc::channel();
            let render_api = api.clone();
            let render_stop = stop.clone();
            let render_events = events.clone();
            let address = handle as usize;
            let render = thread::spawn(move || {
                renderer(
                    render_api,
                    address,
                    frames,
                    render_stop,
                    ready_tx,
                    render_events,
                )
            });
            let initialized = ready_rx
                .recv()
                .map_err(|_| "Renderizador encerrou inesperadamente".to_owned())
                .and_then(|r| r);
            if let Err(error) = initialized {
                let _ = render.join();
                return Err(error);
            }
            Some(render)
        };
        let mut loaded = false;
        let mut generation = 0;
        let mut last = Instant::now();
        let mut preparation: Option<(u64, f64, bool, bool)> = None;
        let mut prepared: Option<u64> = None;
        let mut scheduled: Option<(u64, Instant)> = None;
        'control: while !stop.load(Ordering::Acquire) {
            while let Ok(command) = rx.try_recv() {
                let result = match command {
                    Command::Shutdown => break 'control,
                    Command::Load(path, id) => {
                        scheduled = None;
                        preparation = None;
                        prepared = None;
                        generation = id;
                        loaded = false;
                        let _ = api.command(handle, &["set", "pause", "yes"]);
                        match path.to_str() {
                            Some(path) => api.command(handle, &["loadfile", path, "replace"]),
                            None => Err("O nome do arquivo não é UTF-8".into()),
                        }
                    }
                    Command::Pause(paused) => {
                        scheduled = None;
                        preparation = None;
                        prepared = None;
                        api.command(handle, &["set", "pause", if paused { "yes" } else { "no" }])
                    }
                    Command::Prepare { id, position } => {
                        scheduled = None;
                        prepared = None;
                        preparation = Some((id, position, false, false));
                        api.command(handle, &["set", "pause", "yes"])
                            .and_then(|_| api.command(handle, &["set", "speed", "1"]))
                            .and_then(|_| {
                                api.command(
                                    handle,
                                    &["seek", &position.to_string(), "absolute+exact"],
                                )
                            })
                    }
                    Command::Schedule { id, at } => {
                        if prepared == Some(id) && Instant::now() < at {
                            scheduled = Some((id, at));
                        } else {
                            let _ = events.blocking_send(Event::Missed { id });
                        }
                        Ok(())
                    }
                    Command::Seek(position) => {
                        preparation = None;
                        prepared = None;
                        scheduled = None;
                        api.command(handle, &["seek", &position.to_string(), "absolute+exact"])
                    }
                    Command::Speed(speed) => {
                        api.command(handle, &["set", "speed", &speed.to_string()])
                    }
                    Command::Volume(volume) => {
                        api.command(handle, &["set", "volume", &volume.to_string()])
                    }
                    Command::Audio => api.command(handle, &["cycle", "aid"]),
                    Command::Subtitle => api.command(handle, &["cycle", "sid"]),
                    Command::AudioTrack(id) => {
                        api.command(handle, &["set", "aid", &id.to_string()])
                    }
                    Command::SubtitleTrack(id) => api.command(
                        handle,
                        &[
                            "set",
                            "sid",
                            &id.map_or_else(|| "no".into(), |id| id.to_string()),
                        ],
                    ),
                };
                if let Err(error) = result {
                    let _ = events.blocking_send(Event::Error(error));
                }
            }
            loop {
                // SAFETY: evento permanece válido até a próxima chamada wait.
                let event = unsafe { &*(api.wait)(handle, 0.0) };
                if event.id == 0 {
                    break;
                }
                if let Some((_, _, seek_seen, restarted)) = &mut preparation {
                    if event.id == 20 {
                        *seek_seen = true;
                    }
                    if event.id == 21 && *seek_seen {
                        *restarted = true;
                    }
                }
                if event.id == 8 {
                    loaded = true;
                }
                if event.id == 7 && !event.data.is_null() {
                    let end = unsafe { &*event.data.cast::<EndFile>() };
                    if end.error < 0 {
                        loaded = false;
                        let _ = events.blocking_send(Event::Error(
                            "Não foi possível reproduzir o arquivo".into(),
                        ));
                    }
                }
            }
            if let Some((id, target, _, true)) = preparation
                && loaded
                && !api.flag(handle, c"seeking")
                && api.flag(handle, c"pause")
                && !api.flag(handle, c"paused-for-cache")
                && (!api.flag(handle, c"eof-reached")
                    || target >= api.double(handle, c"duration") - 0.15)
                && (api.double(handle, c"time-pos") - target).abs() <= 0.15
                && events.blocking_send(Event::Prepared { id }).is_ok()
            {
                prepared = Some(id);
                preparation = None;
            }
            if let Some((id, at)) = scheduled
                && Instant::now() >= at
            {
                scheduled = None;
                if Instant::now().duration_since(at) > Duration::from_millis(80)
                    || !loaded
                    || api.flag(handle, c"seeking")
                    || api.flag(handle, c"paused-for-cache")
                    || api.flag(handle, c"eof-reached")
                {
                    let _ = events.blocking_send(Event::Missed { id });
                } else if api.command(handle, &["set", "pause", "no"]).is_ok() {
                    let _ = events.blocking_send(Event::Started {
                        id,
                        at: Instant::now(),
                    });
                } else {
                    let _ = events.blocking_send(Event::Missed { id });
                }
            }
            if last.elapsed() >= Duration::from_millis(50) {
                let position = api.double(handle, c"time-pos");
                let measured_ms = origin.elapsed().as_secs_f64() * 1000.0;
                let status = Status {
                    generation,
                    measured_ms,
                    position,
                    speed: api.double(handle, c"speed"),
                    seeking: api.flag(handle, c"seeking"),
                    loaded,
                    duration: api.double(handle, c"duration"),
                    paused: api.flag(handle, c"pause"),
                    ended: api.flag(handle, c"eof-reached"),
                    tracks: tracks::read(&api, handle),
                    blocked: api.flag(handle, c"paused-for-cache")
                        || api.flag(handle, c"eof-reached"),
                };
                if events.is_closed() {
                    break;
                }
                let _ = events.try_send(Event::Status(status));
                last = Instant::now();
            }
            thread::sleep(Duration::from_millis(2));
        }
        stop.store(true, Ordering::Release);
        if let Some(render) = render {
            let _ = render.join();
        }
        Ok(())
    })()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extrapolates_from_measurement_time_and_actual_speed() {
        let mut s = Status {
            loaded: true,
            position: 10.0,
            duration: 120.0,
            measured_ms: 1000.0,
            paused: false,
            speed: 1.02,
            ..Default::default()
        };
        assert!((s.position_at(1100.0) - 10.102).abs() < 1e-9);
        s.paused = true;
        assert_eq!(s.position_at(1100.0), 10.0);
        s.paused = false;
        s.seeking = true;
        assert_eq!(s.position_at(1100.0), 10.0);
    }
}
