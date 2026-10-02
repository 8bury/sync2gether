//! Cenários e captura da interface, sem leitura de mídia ou acesso à rede.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;

use crate::model::SessionState;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Scenario {
    Idle,
    Waiting,
    Synchronized,
    Paused,
    Reconnecting,
}

impl From<Scenario> for SessionState {
    fn from(scenario: Scenario) -> Self {
        match scenario {
            Scenario::Idle => Self::Idle,
            Scenario::Waiting => Self::Waiting,
            Scenario::Synchronized => Self::Synchronized,
            Scenario::Paused => Self::Paused,
            Scenario::Reconnecting => Self::Reconnecting,
        }
    }
}

pub struct Capture {
    path: PathBuf,
    started: Instant,
    requested: bool,
}

impl Capture {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            started: Instant::now(),
            requested: false,
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(screenshot) = screenshot {
            let result = image::save_buffer_with_format(
                &self.path,
                screenshot.as_raw(),
                screenshot.size[0] as u32,
                screenshot.size[1] as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            );
            if let Err(error) = result {
                eprintln!("Falha ao salvar captura: {error}");
                std::process::exit(1);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.started.elapsed() > Duration::from_secs(15) {
            eprintln!("A captura não foi recebida em 15 segundos.");
            std::process::exit(1);
        } else if !self.requested && self.started.elapsed() >= Duration::from_millis(300) {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
