//! Opções compartilhadas pela janela do aplicativo e pela verificação OpenGL.
use eframe::{NativeOptions, egui, egui_glow};

pub fn options() -> NativeOptions {
    NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 480.0]),
        // No Wayland, o compositor pode reter callbacks de frames de janelas
        // cobertas. Esperar VSync em eglSwapBuffers bloqueia também a thread
        // que precisa responder aos pings e eventos da janela.
        glow_options: egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    }
}
