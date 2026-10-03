//! Verificação com janela real e fixture sintética de scripts/test-player-gl.sh.
use eframe::egui;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use sync2gether::{
    player::GlRenderer,
    protocol::Control,
    runtime::{Command, Runtime, View},
    ui,
};

type ResultSlot = Arc<Mutex<Option<Result<(), String>>>>;
fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/player-gl-fixture.mkv")
}
struct Check {
    runtime: Runtime,
    renderer: GlRenderer,
    view: View,
    inputs: ui::Inputs,
    started: Instant,
    stage_started: Instant,
    stage: u8,
    waiting_shot: bool,
    first_frame: Vec<u8>,
    result: ResultSlot,
    selected_tracks: bool,
    previous_generation: u64,
}
impl Check {
    fn finish(&self, ctx: &egui::Context, result: Result<(), String>) {
        *self.result.lock().unwrap() = Some(result);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn send(&self, command: Command) {
        self.runtime.commands.try_send(command).unwrap();
    }
    fn screenshot(&mut self, ctx: &egui::Context) {
        if !self.waiting_shot {
            self.waiting_shot = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
    fn check_image(&mut self, image: &egui::ColorImage, screen_width: f32) -> Result<(), String> {
        let rect = self.inputs.player.video_rect.unwrap();
        let scale = image.size[0] as f32 / screen_width;
        let mut pixels = Vec::new();
        // Só vídeo no centro: texto e botões não podem fazer esta verificação passar.
        for y in ((rect.top() + rect.height() * 0.35) * scale) as usize
            ..((rect.top() + rect.height() * 0.55) * scale) as usize
        {
            for x in ((rect.left() + rect.width() * 0.25) * scale) as usize
                ..((rect.left() + rect.width() * 0.75) * scale) as usize
            {
                let color = image.pixels
                    [y.min(image.size[1] - 1) * image.size[0] + x.min(image.size[0] - 1)];
                pixels.extend_from_slice(&color.to_array());
            }
        }
        if !pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|c| c[0].abs_diff(c[1]) > 60 || c[1].abs_diff(c[2]) > 60)
        {
            return Err("O screenshot não contém o vídeo colorido da fixture".into());
        }
        if self.stage == 1 {
            self.first_frame = pixels;
        } else if self.stage == 2 && self.first_frame == pixels {
            return Err("O vídeo não avançou entre capturas".into());
        }
        Ok(())
    }
}
impl eframe::App for Check {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A troca de faixas com vídeo pausado não garante um callback de novo frame.
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        if self.started.elapsed() > Duration::from_secs(25) {
            self.finish(
                ui.ctx(),
                Err(format!(
                    "Timeout na etapa {}: {}, tamanho={:?}, fullscreen={:?}, pausado={}, legenda selecionada={}",
                    self.stage, self.view.status, ui.input(|i| i.content_rect().size()),
                    ui.input(|i| i.viewport().fullscreen), self.view.player.paused,
                    self.view.player.tracks.iter().any(|t| t.kind == sync2gether::player::TrackKind::Subtitle && t.selected)
                )),
            );
            return;
        }
        while let Ok(view) = self.runtime.views.try_recv() {
            self.view = view;
        }
        if let Some(error) = &self.view.error {
            self.finish(ui.ctx(), Err(error.clone()));
            return;
        }
        if !matches!(self.stage, 5 | 6) {
            self.renderer.prepare(ui);
        }
        let actions = ui::draw_live(
            ui,
            &self.view,
            &mut self.inputs,
            // Simula uma janela oculta: nenhum callback GL atende o mpv,
            // enquanto a interface e o controle continuam sendo exercitados.
            if matches!(self.stage, 5 | 6) {
                None
            } else {
                Some(ui::Video::OpenGl(self.renderer.callback()))
            },
        );
        for action in actions {
            self.send(action);
        }
        let shot = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            if let Err(error) = self.check_image(&image, ui.input(|i| i.content_rect().width())) {
                #[cfg(feature = "demo")]
                image::save_buffer_with_format(
                    format!(".cache/player-gl-failed-stage-{}.png", self.stage),
                    image.as_raw(),
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .unwrap();
                self.finish(ui.ctx(), Err(format!("Etapa {}: {error}", self.stage)));
                return;
            }
            #[cfg(feature = "demo")]
            {
                let path = format!(".cache/player-gl-stage-{}.png", self.stage);
                image::save_buffer_with_format(
                    path,
                    image.as_raw(),
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .unwrap();
            }
            self.waiting_shot = false;
            self.stage += 1;
            self.stage_started = Instant::now();
            if self.stage == 3 {
                self.inputs.player.fullscreen = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            } else if self.stage == 4 {
                self.inputs.player.fullscreen = false;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(640.0, 480.0)));
                self.send(Command::Control(Control::Pause));
                self.send(Command::SubtitleTrack(None));
            } else if self.stage == 5 {
                self.previous_generation = self.view.player.generation;
                self.send(Command::OpenPath(fixture()));
            } else if self.stage == 8 {
                // Fecha também com um seek em andamento para exercitar a liberação.
                self.send(Command::Control(Control::Seek(8.0)));
                self.finish(ui.ctx(), Ok(()));
            }
        }
        match self.stage {
            0 if self.view.player.loaded => {
                let audios: Vec<_> = self
                    .view
                    .player
                    .tracks
                    .iter()
                    .filter(|t| t.kind == sync2gether::player::TrackKind::Audio)
                    .collect();
                let subtitle = self
                    .view
                    .player
                    .tracks
                    .iter()
                    .find(|t| t.kind == sync2gether::player::TrackKind::Subtitle);
                if audios.len() < 2 || subtitle.is_none() {
                    self.finish(
                        ui.ctx(),
                        Err(
                            "Execute scripts/test-player-gl.sh para gerar duas faixas e legendas"
                                .into(),
                        ),
                    );
                    return;
                }
                let subtitle = subtitle.unwrap();
                if !self.selected_tracks {
                    self.send(Command::AudioTrack(audios[1].id));
                    self.send(Command::SubtitleTrack(Some(subtitle.id)));
                    self.selected_tracks = true;
                    return;
                }
                if !audios[1].selected || !subtitle.selected {
                    return;
                }
                self.send(Command::Control(Control::Play));
                self.stage = 1;
                self.stage_started = Instant::now();
            }
            1 if self.view.player.position > 1.5 => self.screenshot(ui.ctx()),
            2 if self.view.player.position > 4.5 => {
                if self.inputs.player.controls_visible {
                    self.finish(ui.ctx(), Err("Controles não se ocultaram".into()));
                    return;
                }
                self.screenshot(ui.ctx());
            }
            3 if self.stage_started.elapsed() > Duration::from_millis(300)
                && ui.input(|i| i.viewport().fullscreen == Some(true)) =>
            {
                self.screenshot(ui.ctx())
            }
            // Alguns compositores restauram o tamanho anterior ao confirmar a
            // saída de tela cheia. Peça o resize depois dessa confirmação também.
            4 if ui.input(|i| {
                i.viewport().fullscreen != Some(true) && i.content_rect().width() >= 700.0
            }) =>
            {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(640.0, 480.0)));
            }
            4 if self.stage_started.elapsed() > Duration::from_millis(300)
                && self.view.player.paused
                && !self
                    .view
                    .player
                    .tracks
                    .iter()
                    .any(|t| t.kind == sync2gether::player::TrackKind::Subtitle && t.selected)
                && ui.input(|i| i.content_rect().width() < 700.0) =>
            {
                if !self.inputs.player.controls_visible {
                    self.finish(ui.ctx(), Err("Controles não reapareceram na pausa".into()));
                    return;
                }
                self.screenshot(ui.ctx());
            }
            5 if self.stage_started.elapsed() > Duration::from_secs(2)
                && self.view.player.generation > self.previous_generation
                && self.view.player.loaded =>
            {
                self.send(Command::Control(Control::Seek(8.0)));
                self.stage = 6;
                self.stage_started = Instant::now();
            }
            6 if self.stage_started.elapsed() > Duration::from_secs(2)
                && self.view.player.paused
                && !self.view.player.seeking
                && (self.view.player.position - 8.0).abs() < 0.15 =>
            {
                self.stage = 7;
                self.stage_started = Instant::now();
            }
            7 if self.stage_started.elapsed() > Duration::from_millis(300) => {
                self.screenshot(ui.ctx());
            }
            _ => {}
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.renderer.destroy();
        self.runtime.shutdown();
    }
}
fn main() -> eframe::Result {
    let result: ResultSlot = Arc::default();
    let output = result.clone();
    eframe::run_native(
        "Verificação do player OpenGL",
        sync2gether::window::options(),
        Box::new(move |cc| {
            ui::configure_theme(&cc.egui_ctx, false);
            let (runtime, renderer) = Runtime::start_gl(cc)?;
            runtime
                .commands
                .try_send(Command::OpenPath(fixture()))
                .unwrap();
            Ok(Box::new(Check {
                runtime,
                renderer,
                view: View::default(),
                inputs: ui::Inputs::default(),
                started: Instant::now(),
                stage_started: Instant::now(),
                stage: 0,
                waiting_shot: false,
                first_frame: Vec::new(),
                result: output,
                selected_tracks: false,
                previous_generation: 0,
            }))
        }),
    )?;
    result
        .lock()
        .unwrap()
        .take()
        .expect("Janela fechada antes da verificação")
        .expect("Verificação OpenGL falhou");
    println!(
        "OpenGL: vídeo, faixas, controles, tela cheia, resize, comandos sem pintura e encerramento verificados."
    );
    Ok(())
}
