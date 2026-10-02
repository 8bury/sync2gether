//! Telas sem acesso a arquivos, ao player ou à rede.

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::model::{Action, SessionState};

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    surface: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: Color32::from_rgb(17, 21, 25),
                surface: Color32::from_rgb(25, 31, 36),
                border: Color32::from_rgb(43, 53, 59),
                text: Color32::from_rgb(235, 240, 238),
                muted: Color32::from_rgb(156, 169, 172),
                accent: Color32::from_rgb(157, 224, 195),
            }
        } else {
            Self {
                background: Color32::from_rgb(242, 245, 242),
                surface: Color32::WHITE,
                border: Color32::from_rgb(214, 223, 217),
                text: Color32::from_rgb(30, 47, 40),
                muted: Color32::from_rgb(91, 109, 100),
                accent: Color32::from_rgb(32, 112, 80),
            }
        }
    }
}

/// Aplica o tema também usado pelos testes de layout.
pub fn configure_theme(ctx: &egui::Context, light: bool) {
    let colors = Palette::new(!light);
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = colors.background;
    visuals.override_text_color = Some(colors.text);
    visuals.selection.bg_fill = colors.accent.gamma_multiply(0.18);
    visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    ctx.set_visuals(visuals);
    ctx.style_mut_of(
        if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        },
        |style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(16.0, 10.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        },
    );
}

pub fn draw(ui: &mut egui::Ui, state: SessionState, demo: bool) -> Vec<Action> {
    let mut actions = Vec::new();
    let colors = Palette::new(ui.visuals().dark_mode);
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(colors.background).inner_margin(24))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(ui.available_width().min(840.0));
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        header(ui, colors, demo);
                        ui.add_space(28.0);
                        if demo {
                            demo_picker(ui, colors, state, &mut actions);
                            ui.add_space(20.0);
                        }
                        if demo && state != SessionState::Idle {
                            session(ui, colors, state);
                        } else {
                            welcome(ui, colors);
                        }
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Uma cópia do filme em cada PC")
                                    .small()
                                    .color(colors.muted),
                            );
                            ui.label(RichText::new("·").color(colors.muted));
                            ui.label(
                                RichText::new("Conexão pelo Tailscale")
                                    .small()
                                    .color(colors.muted),
                            );
                        });
                    });
                });
            });
        });
    actions
}

fn header(ui: &mut egui::Ui, colors: Palette, demo: bool) {
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 10, colors.accent);
        for offset in [-5.0, 5.0] {
            let center = rect.center() + egui::vec2(offset, 0.0);
            painter.line_segment(
                [
                    center + egui::vec2(-2.0, -6.0),
                    center + egui::vec2(-2.0, 6.0),
                ],
                Stroke::new(3.0, colors.background),
            );
        }
        ui.label(RichText::new("sync2gether").size(21.0).strong());
        ui.add_space(8.0);
        badge(
            ui,
            colors,
            if demo {
                "DEMO OFFLINE"
            } else {
                "EM DESENVOLVIMENTO"
            },
        );
    });
}

fn badge(ui: &mut egui::Ui, colors: Palette, text: &str) {
    egui::Frame::new()
        .fill(colors.accent.gamma_multiply(0.10))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(9, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.0).strong().color(colors.accent));
        });
}

fn card(colors: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(14)
        .inner_margin(20)
}

fn welcome(ui: &mut egui::Ui, colors: Palette) {
    ui.label(
        RichText::new("Sua próxima sessão\ncomeça aqui.")
            .size(36.0)
            .strong(),
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("Escolham um filme. Cada um no seu sofá.")
            .size(18.0)
            .color(colors.muted),
    );
    ui.add_space(24.0);

    if ui.available_width() >= 640.0 {
        ui.columns(2, |columns| {
            room_card(&mut columns[0], colors, true);
            room_card(&mut columns[1], colors, false);
        });
    } else {
        room_card(ui, colors, true);
        ui.add_space(8.0);
        room_card(ui, colors, false);
    }
    ui.add_space(20.0);
    ui.label(RichText::new("Ainda estamos preparando a sala.").strong());
    ui.label(RichText::new("Criar salas, abrir filmes e reproduzir em sincronia estarão disponíveis nas próximas etapas.").color(colors.muted));
}

fn room_card(ui: &mut egui::Ui, colors: Palette, host: bool) {
    card(colors).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(if host { "01 / CRIAR" } else { "02 / ENTRAR" })
                .size(11.0)
                .color(colors.accent)
                .strong(),
        );
        ui.add_space(14.0);
        ui.label(
            RichText::new(if host {
                "Convide alguém"
            } else {
                "Já tem uma sala?"
            })
            .size(22.0)
            .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(if host {
                "A sala fica no seu PC. Compartilhe seu endereço do Tailscale com a outra pessoa."
            } else {
                "Use o endereço do Tailscale de quem criou a sala para assistir junto."
            })
            .color(colors.muted),
        );
        ui.add_space(18.0);
        let button = egui::Button::new(if host { "Criar sala" } else { "Entrar na sala" })
            .min_size(egui::vec2(ui.available_width(), 42.0))
            .corner_radius(8);
        ui.add_enabled(false, button)
            .on_disabled_hover_text("Disponível quando a conexão entre os PCs for implementada.");
    });
}

fn demo_picker(ui: &mut egui::Ui, colors: Palette, state: SessionState, actions: &mut Vec<Action>) {
    ui.label(
        RichText::new("Prévia da interface · estados e dados fictícios")
            .small()
            .color(colors.muted),
    );
    ui.horizontal_wrapped(|ui| {
        for candidate in SessionState::ALL {
            if ui
                .selectable_label(state == candidate, candidate.label())
                .clicked()
            {
                actions.push(Action::SetDemoState(candidate));
            }
        }
    });
}

fn session(ui: &mut egui::Ui, colors: Palette, state: SessionState) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("No mesmo momento.").size(28.0).strong());
        badge(ui, colors, state.label());
    });
    ui.label(
        RichText::new(match state {
            SessionState::Waiting => "Tudo pronto por aqui. Falta a outra pessoa chegar.",
            SessionState::Synchronized => "Os dois participantes estão na mesma posição do filme.",
            SessionState::Paused => "Uma pausa para os dois. O filme espera por vocês.",
            SessionState::Reconnecting => "O filme está pausado enquanto a conexão volta.",
            SessionState::Idle => "",
        })
        .color(colors.muted),
    );
    ui.add_space(16.0);
    movie_preview(ui, state);
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Filme de demonstração.mkv").strong());
        ui.label(
            RichText::new("00:12:34 / 01:30:00")
                .small()
                .color(colors.muted),
        );
    });
    ui.add(
        egui::ProgressBar::new(754.0 / 5400.0)
            .fill(colors.accent)
            .desired_height(4.0),
    );
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled(
            false,
            egui::Button::new(if state == SessionState::Synchronized {
                "Pausar"
            } else {
                "Reproduzir"
            }),
        )
        .on_disabled_hover_text("O modo demo não reproduz vídeo.");
        ui.add_enabled(false, egui::Button::new("Abrir filme"));
        ui.label(
            RichText::new("Controles ilustrativos")
                .small()
                .color(colors.muted),
        );
    });
    ui.add_space(16.0);
    card(colors).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            participant(ui, colors, "Você", "Filme pronto");
            ui.add_space(24.0);
            participant(
                ui,
                colors,
                "Outra pessoa",
                match state {
                    SessionState::Waiting => "Aguardando entrada",
                    SessionState::Reconnecting => "Reconectando",
                    _ => "Filme pronto",
                },
            );
        });
    });
}

fn participant(ui: &mut egui::Ui, colors: Palette, name: &str, status: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(name).strong());
        ui.label(RichText::new(status).small().color(colors.muted));
    });
}

fn movie_preview(ui: &mut egui::Ui, state: SessionState) {
    let width = ui.available_width();
    let height = (width * 0.34).clamp(160.0, 240.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 14, Color32::from_rgb(11, 17, 20));
    let center = rect.center();
    for radius in [65.0, 100.0, 140.0, 185.0] {
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0, Color32::from_rgb(28, 43, 43)),
        );
    }
    painter.circle_filled(
        center - egui::vec2(0.0, 22.0),
        24.0,
        Color32::from_rgb(35, 53, 49),
    );
    let icon_center = center - egui::vec2(0.0, 22.0);
    if state == SessionState::Synchronized {
        for offset in [-4.0, 4.0] {
            let start = icon_center + egui::vec2(offset, -7.0);
            painter.line_segment(
                [start, start + egui::vec2(0.0, 14.0)],
                Stroke::new(3.0, Color32::from_rgb(157, 224, 195)),
            );
        }
    } else {
        painter.add(egui::Shape::convex_polygon(
            vec![
                icon_center + egui::vec2(-4.0, -8.0),
                icon_center + egui::vec2(8.0, 0.0),
                icon_center + egui::vec2(-4.0, 8.0),
            ],
            Color32::from_rgb(157, 224, 195),
            Stroke::NONE,
        ));
    }
    painter.text(
        center + egui::vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        "O filme aparece aqui",
        egui::FontId::proportional(17.0),
        Color32::from_rgb(226, 234, 231),
    );
    painter.text(
        center + egui::vec2(0.0, 47.0),
        egui::Align2::CENTER_CENTER,
        "Prévia sem reprodução de vídeo",
        egui::FontId::proportional(12.0),
        Color32::from_rgb(148, 164, 160),
    );
}
