use super::{Palette, kit, section_label, strong};
use crate::{
    runtime::{Command, Role, View},
    stream::{BitrateAdvice, StreamPhase, format_bps},
};
use eframe::egui::{self, RichText, Stroke};

pub(super) fn preparation_strip(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    actions: &mut Vec<Command>,
) {
    if view.role == Role::Local || !view.connected {
        return;
    }
    if view.stream.active {
        active_card(ui, colors, view, actions);
        return;
    }
    if let StreamPhase::Error(err) = &view.stream.phase {
        egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1.0, colors.warning.gamma_multiply(0.5)))
            .corner_radius(14)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(strong(ui, "Streaming P2P direto indisponível").size(13.5));
                        ui.add(
                            egui::Label::new(RichText::new(err).size(12.5).color(colors.muted))
                                .wrap(),
                        );
                    });
                    if view.can_stream {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::Button::new(kit::Kind::Secondary, "Tentar novamente")
                                .size(kit::Size::Small)
                                .show(ui, colors)
                                .clicked()
                            {
                                actions.push(Command::StartStream);
                            }
                        });
                    }
                });
            });
        return;
    }
    if !view.can_stream || view.matched {
        return;
    }
    let (title, hint, button_label, kind) = if view.role == Role::Guest {
        (
            "Não tem sua cópia do filme?",
            "Receba direto do anfitrião por túnel QUIC P2P cifrado, sem passar vídeo pelo servidor.",
            "Assistir por streaming P2P",
            kit::Kind::Primary,
        )
    } else {
        (
            "A outra pessoa não tem o arquivo?",
            "Transmita direto do seu PC por túnel QUIC P2P cifrado com cache progressivo.",
            "Transmitir meu filme (P2P)",
            kit::Kind::Secondary,
        )
    };
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(14)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let wide = ui.available_width() >= 520.0;
            if wide {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let button = kit::Button::new(kind, button_label).size(kit::Size::Medium);
                    let bw = button.natural_width(ui);
                    let text_w = (ui.available_width() - bw - 16.0).max(120.0);
                    ui.allocate_ui(egui::vec2(text_w, 40.0), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(strong(ui, title).size(13.5));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(hint).size(12.5).color(colors.muted),
                                )
                                .wrap(),
                            );
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button.show(ui, colors).clicked() {
                            actions.push(Command::StartStream);
                        }
                    });
                });
            } else {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(strong(ui, title).size(13.5));
                    ui.add(
                        egui::Label::new(RichText::new(hint).size(12.5).color(colors.muted)).wrap(),
                    );
                    if kit::Button::new(kind, button_label)
                        .size(kit::Size::Medium)
                        .fill_width(ui)
                        .show(ui, colors)
                        .clicked()
                    {
                        actions.push(Command::StartStream);
                    }
                });
            }
        });
}

fn active_card(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    let s = &view.stream;
    let (phase_label, busy) = match &s.phase {
        StreamPhase::Offering => ("Preparando túnel P2P…", true),
        StreamPhase::Connecting => ("Abrindo conexão P2P direta…", true),
        StreamPhase::Probing => ("Medindo velocidade do enlace…", true),
        StreamPhase::Streaming => (
            if s.is_sender {
                "Transmitindo direto (P2P cifrado)"
            } else {
                "Recebendo direto (P2P cifrado)"
            },
            false,
        ),
        StreamPhase::Idle | StreamPhase::Error(_) => ("Streaming P2P", false),
    };
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.accent.gamma_multiply(0.45)))
        .corner_radius(14)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                let glyph = if busy {
                    Some(kit::Glyph::Spinner)
                } else {
                    Some(kit::Glyph::Check)
                };
                let color = if busy { colors.accent } else { colors.success };
                kit::status(ui, glyph, phase_label, color);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::Button::new(kit::Kind::Ghost, "Parar")
                        .size(kit::Size::Small)
                        .show(ui, colors)
                        .clicked()
                    {
                        actions.push(Command::StopStream);
                    }
                });
            });
            if s.total_bytes > 0 {
                kit::progress(ui, colors, s.cache_fraction(), ui.available_width());
                let pct = (s.cache_fraction() * 100.0).round() as u32;
                let mut metrics = vec![format!("{pct}% em cache")];
                if let Some(secs) = s.buffered_seconds() {
                    metrics.push(format!("{secs:.0}s à frente"));
                }
                if let Some(link) = s.link_bps {
                    metrics.push(format!("Enlace {}", format_bps(link)));
                }
                if let Some(bitrate) = s.file_bitrate_bps {
                    metrics.push(format!("Filme {}", format_bps(bitrate)));
                }
                if s.transfer_bps > 0 {
                    metrics.push(format!("Agora {}", format_bps(s.transfer_bps)));
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(metrics.join(" · "))
                            .size(12.5)
                            .color(colors.muted),
                    )
                    .wrap(),
                );
            }
            if let Some((text, color)) = advice_line(s.advice, colors) {
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
            }
        });
}

fn advice_line(advice: BitrateAdvice, colors: Palette) -> Option<(String, egui::Color32)> {
    match advice {
        BitrateAdvice::Unknown => None,
        BitrateAdvice::Ready => Some((
            "Conexão folgada para este filme — pronto para assistir direto.".into(),
            colors.success,
        )),
        BitrateAdvice::Tight => Some((
            "Conexão justa para a taxa deste filme — aguardar alguns segundos de cache ajuda."
                .into(),
            colors.warning,
        )),
        BitrateAdvice::Prebuffer { estimated_secs } => {
            let mins = estimated_secs.div_ceil(60);
            Some((
                format!(
                    "Enlace abaixo da taxa do filme — pré-carregando (~{mins} min para assistir sem pausas)."
                ),
                colors.warning,
            ))
        }
    }
}

pub(super) fn room_section(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    actions: &mut Vec<Command>,
) {
    if !view.stream.active {
        return;
    }
    let s = &view.stream;
    ui.add_space(10.0);
    section_label(ui, colors, "Streaming P2P direto");
    if s.total_bytes > 0 {
        kit::progress(ui, colors, s.cache_fraction(), ui.available_width());
        let pct = (s.cache_fraction() * 100.0).round() as u32;
        ui.label(
            RichText::new(format!(
                "{pct}% · {}",
                if s.transfer_bps > 0 {
                    format_bps(s.transfer_bps)
                } else if let Some(link) = s.link_bps {
                    format_bps(link)
                } else {
                    "conectado".into()
                }
            ))
            .size(12.5)
            .color(colors.muted),
        );
    }
    if let Some(addr) = s.peer_addr {
        ui.label(
            RichText::new(format!("Direto: {addr}"))
                .size(12.0)
                .color(colors.faint),
        );
    }
    if kit::Button::new(kit::Kind::Ghost, "Parar streaming")
        .size(kit::Size::Small)
        .show(ui, colors)
        .clicked()
    {
        actions.push(Command::StopStream);
    }
}
