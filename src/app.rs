//! Estado da janela. Ações são aplicadas depois do desenho.
use crate::{
    diagnostics::{self, Stage},
    model::{Action, SessionState},
    runtime::{Runtime, View},
};
use eframe::egui;

pub struct App {
    state: SessionState,
    demo: bool,
    runtime: Option<Runtime>,
    view: View,
    inputs: crate::ui::Inputs,
    texture: Option<egui::TextureHandle>,
    renderer: Option<crate::player::GlRenderer>,
    fullscreen_applied: bool,
    #[cfg(feature = "demo")]
    capture: Option<crate::demo::Capture>,
}
impl Default for App {
    fn default() -> Self {
        Self::new(Some(Runtime::start()), None)
    }
}
impl App {
    fn new(runtime: Option<Runtime>, renderer: Option<crate::player::GlRenderer>) -> Self {
        Self {
            state: SessionState::Idle,
            demo: false,
            runtime,
            renderer,
            view: View::default(),
            inputs: crate::ui::Inputs::default(),
            texture: None,
            fullscreen_applied: false,
            #[cfg(feature = "demo")]
            capture: None,
        }
    }
    pub fn with_commands_gl(
        cc: &eframe::CreationContext<'_>,
        commands: Vec<crate::runtime::Command>,
    ) -> Result<Self, String> {
        let (runtime, renderer) = Runtime::start_gl(cc)?;
        Ok(Self::new(Some(runtime), Some(renderer)).queue_commands(commands))
    }
    pub fn with_commands(commands: Vec<crate::runtime::Command>) -> Self {
        Self::default().queue_commands(commands)
    }
    fn queue_commands(mut self, commands: Vec<crate::runtime::Command>) -> Self {
        for command in &commands {
            match command {
                crate::runtime::Command::Host(address) => self.inputs.address = address.clone(),
                crate::runtime::Command::HostRoom(address)
                | crate::runtime::Command::RequestJoin(address) => {
                    self.inputs.address = address.clone()
                }
                crate::runtime::Command::Join(address, _) => {
                    self.inputs.address = address.clone();
                }
                _ => {}
            }
        }
        if let Some(runtime) = &self.runtime {
            for command in commands {
                let _ = runtime.commands.try_send(command);
            }
        }
        self
    }

    #[cfg(feature = "demo")]
    pub fn demo(state: SessionState, screenshot: Option<std::path::PathBuf>) -> Self {
        Self {
            state,
            demo: true,
            runtime: None,
            view: View::default(),
            inputs: crate::ui::Inputs::default(),
            texture: None,
            renderer: None,
            fullscreen_applied: false,
            capture: screenshot.map(crate::demo::Capture::new),
        }
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        diagnostics::mark(Stage::Logic);
        diagnostics::window(ctx, self.view.player.loaded);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        diagnostics::mark(Stage::Exit);
        if let Some(renderer) = &self.renderer {
            renderer.destroy();
        }
        self.runtime.take();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        diagnostics::mark(Stage::ReadViews);
        if let Some(renderer) = &self.renderer {
            renderer.prepare(ui);
        }
        if let Some(runtime) = &self.runtime {
            while let Ok(view) = runtime.views.try_recv() {
                self.view = view;
            }
            let frame = runtime
                .frames
                .try_lock()
                .ok()
                .and_then(|mut frame| frame.take());
            if let Some(frame) = frame {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [crate::player::WIDTH, crate::player::HEIGHT],
                    &frame,
                );
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ui.ctx().load_texture(
                        "movie",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            let video = self
                .renderer
                .as_ref()
                .map(|renderer| crate::ui::Video::OpenGl(renderer.callback()))
                .or_else(|| self.texture.as_ref().map(crate::ui::Video::Texture));
            let mut display = self.view.clone();
            display.player.position = runtime.position_now(&self.view);
            diagnostics::mark(Stage::DrawUi);
            let actions = crate::ui::draw_live(ui, &display, &mut self.inputs, video);
            for action in actions {
                if runtime.commands.try_send(action).is_err() {
                    self.view.error = Some("O app está ocupado. Tente novamente.".into());
                }
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(
                    if self.renderer.is_some() { 100 } else { 16 },
                ));
        } else {
            diagnostics::mark(Stage::DrawUi);
            for action in crate::ui::draw_with_inputs(ui, self.state, self.demo, &mut self.inputs) {
                match action {
                    Action::SetDemoState(state) if self.demo => self.state = state,
                    Action::SetDemoState(_) => {}
                }
            }
        }
        if self.fullscreen_applied != self.inputs.player.fullscreen {
            self.fullscreen_applied = self.inputs.player.fullscreen;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen_applied));
        }
        #[cfg(feature = "demo")]
        if let Some(capture) = &mut self.capture {
            capture.tick(ui.ctx());
        }
        diagnostics::mark(Stage::UiDone);
    }
}
