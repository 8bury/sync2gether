use super::{Palette, kit, section_label, strong};
use crate::{
    runtime::{Command, Role, View},
    stream::{BitrateAdvice, StreamPhase, file_bitrate_bps, format_bps},
};
use eframe::egui::{self, RichText, Stroke};

pub(super) enum CardStream {
    Busy(&'static str),
    Ready(String),
}

pub(super) fn card_stream_status(view: &View, local: bool) -> Option<CardStream> {
    if view.role == Role::Local || !view.connected {
        return None;
    }
    let is_receiver_card = match view.role {
        Role::Host => {
            !local && (view.stream.active || view.peer_streaming || view.peer_stream.is_some())
        }
        Role::Guest => {
            local
                && ((view.stream.active && !view.stream.is_sender)
                    || view.stream.phase == StreamPhase::Reconnecting)
        }
        Role::Local => false,
    };
    if !is_receiver_card {
        return None;
    }
    let reconnecting = view.stream.phase == StreamPhase::Reconnecting
        || (view.role == Role::Host && view.peer_stream.as_ref().is_some_and(|s| s.reconnecting));
    let connecting = matches!(
        view.stream.phase,
        StreamPhase::Offering | StreamPhase::Connecting | StreamPhase::Probing
    );
    if reconnecting {
        return Some(CardStream::Busy("Reconectando streaming P2P…"));
    }
    if connecting {
        return Some(CardStream::Busy("Conectando streaming P2P…"));
    }
    let buffering = if local {
        view.buffering || view.player.buffering
    } else {
        view.peer_buffering
    };
    if buffering {
        return Some(CardStream::Busy("Carregando streaming P2P…"));
    }
    let verified = if local {
        view.verified && !view.loading
    } else {
        view.peer_verified && !view.peer_loading
    };
    if verified {
        return Some(CardStream::Ready("Via streaming P2P".into()));
    }
    Some(CardStream::Busy("Preparando streaming P2P…"))
}

struct PanelMetrics {
    phase: StreamPhase,
    is_sender: bool,
    cached_bytes: u64,
    total_bytes: u64,
    transfer_bps: u64,
    link_bps: Option<u64>,
    file_bitrate_bps: Option<u64>,
    buffered_ahead_bytes: u64,
    advice: BitrateAdvice,
    peer_addr: Option<std::net::SocketAddr>,
    from_peer_report: bool,
}

impl PanelMetrics {
    fn from_view(view: &View) -> Self {
        let s = &view.stream;
        if view.role == Role::Host
            && let Some(peer) = &view.peer_stream
            && peer.total_bytes > 0
        {
            let phase = if peer.reconnecting || s.phase == StreamPhase::Reconnecting {
                StreamPhase::Reconnecting
            } else if peer.complete || s.phase == StreamPhase::Complete {
                StreamPhase::Complete
            } else if matches!(s.phase, StreamPhase::Error(_)) {
                s.phase.clone()
            } else {
                StreamPhase::Streaming
            };
            return Self {
                phase,
                is_sender: true,
                cached_bytes: peer.cached_bytes,
                total_bytes: peer.total_bytes,
                transfer_bps: peer.transfer_bps,
                link_bps: peer.link_bps.or(s.link_bps),
                file_bitrate_bps: s
                    .file_bitrate_bps
                    .or_else(|| file_bitrate_bps(peer.total_bytes, view.player.duration)),
                buffered_ahead_bytes: peer.buffered_ahead_bytes,
                advice: s.advice,
                peer_addr: s.peer_addr,
                from_peer_report: true,
            };
        }
        Self {
            phase: s.phase.clone(),
            is_sender: s.is_sender,
            cached_bytes: s.cached_bytes,
            total_bytes: s.total_bytes,
            transfer_bps: s.transfer_bps,
            link_bps: s.link_bps,
            file_bitrate_bps: s.file_bitrate_bps,
            buffered_ahead_bytes: s.buffered_ahead_bytes,
            advice: s.advice,
            peer_addr: s.peer_addr,
            from_peer_report: false,
        }
    }

    fn cache_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.cached_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }

    fn buffered_seconds(&self) -> Option<f64> {
        let bitrate = self.file_bitrate_bps.filter(|&b| b > 0)?;
        Some((self.buffered_ahead_bytes as f64 * 8.0) / bitrate as f64)
    }
}

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
            .stroke(Stroke::new(1.0, colors.warning.gamma_multiply(0.5)))
            .corner_radius(8)
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
    let (title, button_label, kind) = if view.role == Role::Guest {
        (
            "Não tem sua cópia do filme?",
            "Assistir por streaming P2P",
            kit::Kind::Primary,
        )
    } else {
        (
            "A outra pessoa não tem o arquivo?",
            "Transmitir meu filme (P2P)",
            kit::Kind::Secondary,
        )
    };
    egui::Frame::new()
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(8)
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
                        ui.label(strong(ui, title).size(13.5));
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
    let m = PanelMetrics::from_view(view);
    let (phase_label, busy, warn) = match &m.phase {
        StreamPhase::Offering => ("Preparando streaming P2P…", true, false),
        StreamPhase::Connecting => ("Abrindo conexão P2P direta…", true, false),
        StreamPhase::Probing => ("Medindo velocidade do enlace…", true, false),
        StreamPhase::Reconnecting => ("Reconectando streaming P2P…", true, true),
        StreamPhase::Streaming => (
            if m.is_sender {
                "Transmitindo direto (P2P cifrado)"
            } else {
                "Recebendo direto (P2P cifrado)"
            },
            false,
            false,
        ),
        StreamPhase::Complete => ("Filme 100% em cache local (P2P verificado)", false, false),
        StreamPhase::Idle | StreamPhase::Error(_) => ("Streaming P2P", false, false),
    };
    egui::Frame::new()
        .stroke(Stroke::new(
            1.0,
            if warn {
                colors.warning.gamma_multiply(0.45)
            } else {
                colors.border
            },
        ))
        .corner_radius(8)
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
                let color = if warn {
                    colors.warning
                } else if busy {
                    colors.accent
                } else {
                    colors.success
                };
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
            if m.total_bytes > 0 {
                kit::progress(ui, colors, m.cache_fraction(), ui.available_width());
                let pct = (m.cache_fraction() * 100.0).round() as u32;
                let progress_label = if m.is_sender && !m.from_peer_report {
                    format!("{pct}% enviados")
                } else {
                    format!("{pct}% em cache")
                };
                let mut metrics = vec![progress_label];
                if let Some(secs) = m.buffered_seconds() {
                    metrics.push(format!("{secs:.0}s à frente"));
                }
                if let Some(link) = m.link_bps {
                    metrics.push(format!("Enlace {}", format_bps(link)));
                }
                if let Some(bitrate) = m.file_bitrate_bps {
                    metrics.push(format!("Filme {}", format_bps(bitrate)));
                }
                metrics.push(format!("Agora {}", format_bps(m.transfer_bps)));
                ui.add(
                    egui::Label::new(
                        RichText::new(metrics.join(" · "))
                            .size(12.5)
                            .color(colors.muted),
                    )
                    .wrap(),
                );
            }
            if let Some((text, color)) = advice_line(m.advice, colors) {
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
    let has_error = matches!(view.stream.phase, StreamPhase::Error(_));
    if !view.stream.active && view.peer_stream.is_none() && !has_error {
        return;
    }
    let m = PanelMetrics::from_view(view);
    ui.add_space(10.0);
    section_label(ui, colors, "Streaming P2P direto");
    if let StreamPhase::Error(err) = &view.stream.phase {
        ui.add(egui::Label::new(RichText::new(err).size(12.5).color(colors.warning)).wrap());
    } else if m.phase == StreamPhase::Reconnecting {
        kit::status(
            ui,
            Some(kit::Glyph::Spinner),
            "Reconectando streaming P2P…",
            colors.warning,
        );
    }
    if m.total_bytes > 0 {
        kit::progress(ui, colors, m.cache_fraction(), ui.available_width());
        let pct = (m.cache_fraction() * 100.0).round() as u32;
        let mut parts = vec![format!("{pct}% em cache")];
        if m.phase != StreamPhase::Complete {
            parts.push(format_bps(m.transfer_bps));
            if let Some(secs) = m.buffered_seconds() {
                parts.push(format!("{secs:.0}s à frente"));
            }
        }
        ui.label(
            RichText::new(parts.join(" · "))
                .size(12.5)
                .color(colors.muted),
        );
    }
    if let Some(addr) = m.peer_addr {
        ui.label(
            RichText::new(format!("Direto: {addr}"))
                .size(12.0)
                .color(colors.faint),
        );
    }
    if has_error
        && view.can_stream
        && kit::Button::new(kit::Kind::Secondary, "Reconectar P2P")
            .size(kit::Size::Small)
            .show(ui, colors)
            .clicked()
    {
        actions.push(Command::StartStream);
    }
    if view.stream.active
        && kit::Button::new(kit::Kind::Ghost, "Parar streaming")
            .size(kit::Size::Small)
            .show(ui, colors)
            .clicked()
    {
        actions.push(Command::StopStream);
    }
}
