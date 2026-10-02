//! Estado da janela. Ações são aplicadas depois do desenho.

use eframe::egui;

use crate::model::{Action, SessionState};

#[derive(Default)]
pub struct App {
    state: SessionState,
    demo: bool,
    #[cfg(feature = "demo")]
    capture: Option<crate::demo::Capture>,
}

impl App {
    #[cfg(feature = "demo")]
    pub fn demo(state: SessionState, screenshot: Option<std::path::PathBuf>) -> Self {
        Self {
            state,
            demo: true,
            capture: screenshot.map(crate::demo::Capture::new),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let actions = crate::ui::draw(ui, self.state, self.demo);
        for action in actions {
            match action {
                Action::SetDemoState(state) if self.demo => self.state = state,
                Action::SetDemoState(_) => {}
            }
        }
        #[cfg(feature = "demo")]
        if let Some(capture) = &mut self.capture {
            capture.tick(ui.ctx());
        }
    }
}
