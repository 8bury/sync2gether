use super::ACCENT;
use crate::{
    runtime::{Command, Role, View},
    stream::{StreamPhase, file_bitrate_bps, format_bps},
    ui::{
        Palette,
        kit::{self, Glyph},
        visuals::{self, Presence},
    },
};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

const VOLUME_STEP: f64 = 5.0;

pub(super) struct EffectiveStream {
    pub cached_bytes: u64,
    pub total_bytes: u64,
    pub transfer_bps: u64,
    pub buffered_ahead_bytes: u64,
    pub file_bitrate_bps: Option<u64>,
    pub reconnecting: bool,
    pub connecting: bool,
    pub complete: bool,
    pub error: Option<String>,
    pub cached_ranges: Vec<(u16, u16)>,
}

impl EffectiveStream {
    pub(super) fn from_view(view: &View) -> Option<Self> {
        if view.role == Role::Host
            && let Some(peer) = &view.peer_stream
        {
            let bitrate = view
                .stream
                .file_bitrate_bps
                .or_else(|| file_bitrate_bps(peer.total_bytes, view.player.duration));
            let error = match &view.stream.phase {
                StreamPhase::Error(err) => Some(err.clone()),
                _ => None,
            };
            return Some(Self {
                cached_bytes: peer.cached_bytes,
                total_bytes: peer.total_bytes,
                transfer_bps: peer.transfer_bps,
                buffered_ahead_bytes: peer.buffered_ahead_bytes,
                file_bitrate_bps: bitrate,
                reconnecting: peer.reconnecting || view.stream.phase == StreamPhase::Reconnecting,
                connecting: matches!(
                    view.stream.phase,
                    StreamPhase::Offering | StreamPhase::Connecting | StreamPhase::Probing
                ),
                complete: peer.complete || view.stream.phase == StreamPhase::Complete,
                error,
                cached_ranges: peer.cached_ranges.clone(),
            });
        }
        let s = &view.stream;
        let has_error = matches!(s.phase, StreamPhase::Error(_));
        if !s.active && !has_error {
            return None;
        }
        let error = match &s.phase {
            StreamPhase::Error(err) => Some(err.clone()),
            _ => None,
        };
        let bitrate = s
            .file_bitrate_bps
            .or_else(|| file_bitrate_bps(s.total_bytes, view.player.duration));
        Some(Self {
            cached_bytes: s.cached_bytes,
            total_bytes: s.total_bytes,
            transfer_bps: s.transfer_bps,
            buffered_ahead_bytes: s.buffered_ahead_bytes,
            file_bitrate_bps: bitrate,
            reconnecting: s.phase == StreamPhase::Reconnecting,
            connecting: matches!(
                s.phase,
                StreamPhase::Offering | StreamPhase::Connecting | StreamPhase::Probing
            ),
            complete: s.phase == StreamPhase::Complete,
            error,
            cached_ranges: s.cached_ranges.clone(),
        })
    }

    pub(super) fn cache_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.cached_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub(super) fn buffered_seconds(&self) -> Option<f64> {
        let bitrate = self.file_bitrate_bps.filter(|&b| b > 0)?;
        Some((self.buffered_ahead_bytes as f64 * 8.0) / bitrate as f64)
    }
}

pub(super) fn is_center_loading_visible(view: &View) -> bool {
    if !view.player.loaded {
        return false;
    }
    let stream = EffectiveStream::from_view(view);
    let stream_reconnecting = stream.as_ref().is_some_and(|s| s.reconnecting);
    let stream_connecting = stream.as_ref().is_some_and(|s| s.connecting);
    let stream_error = stream.as_ref().is_some_and(|s| s.error.is_some());
    let buffering_now = view.buffering || view.peer_buffering || view.waiting_buffer;
    buffering_now
        || stream_connecting
        || ((stream_reconnecting || stream_error) && view.player.paused)
}

pub(super) fn effective_cached_ranges(view: &View) -> Vec<(f32, f32)> {
    if let Some(stream) = EffectiveStream::from_view(view) {
        if !stream.cached_ranges.is_empty() {
            return stream
                .cached_ranges
                .into_iter()
                .map(|(s, e)| (f32::from(s) / 1000.0, f32::from(e) / 1000.0))
                .collect();
        }
        if stream.total_bytes > 0 && stream.cached_bytes > 0 {
            return vec![(0.0, stream.cache_fraction())];
        }
    }
    if view.player.loaded
        && view.player.duration > 0.0
        && view.player.cache_time > view.player.position
    {
        let start = (view.player.position / view.player.duration).clamp(0.0, 1.0) as f32;
        let end = (view.player.cache_time / view.player.duration).clamp(start as f64, 1.0) as f32;
        if end > start {
            return vec![(start, end)];
        }
    }
    Vec::new()
}

pub(super) fn peer_position(view: &View) -> Option<f64> {
    (view.role != Role::Local && view.connected)
        .then_some(view.peer_drift)
        .flatten()
        .map(|drift| view.player.position - drift)
}

pub(super) fn seek_bar(
    ui: &mut egui::Ui,
    response: &egui::Response,
    seek: f64,
    duration: f64,
    enabled: bool,
    cached_ranges: &[(f32, f32)],
    peer: Option<f64>,
) {
    let rect = response.rect;
    let active = enabled && (response.hovered() || response.dragged());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id.with("seek_hover"), active, 0.12);
    let radius = rect.height() / 2.5;
    let range = rect.x_range().shrink(radius);
    let fraction = |position: f64| {
        if duration > 0.0 {
            (position / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    };
    let x = range.min + range.span() * fraction(seek);
    let height = 2.0 + 2.0 * hover;
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), height));
    let painter = ui.painter();
    painter.rect_filled(track, 0, Color32::from_white_alpha(56));
    draw_cached_ranges(painter, track, height, cached_ranges);
    let mut filled = track;
    filled.max.x = x.max(track.left());
    painter.rect_filled(
        filled,
        0,
        if enabled {
            ACCENT
        } else {
            ACCENT.gamma_multiply(0.6)
        },
    );
    if let Some(peer) = peer {
        let peer_x = range.min + range.span() * fraction(peer);
        painter.circle_filled(
            egui::pos2(peer_x + 7.0, rect.center().y),
            6.0,
            Color32::WHITE.gamma_multiply(0.58),
        );
    }
    painter.circle_filled(
        egui::pos2(x, rect.center().y),
        6.0 + hover,
        Color32::WHITE.gamma_multiply(if enabled { 0.86 } else { 0.5 }),
    );
    if enabled && let Some(pointer) = response.hover_pos() {
        let at = ((pointer.x - range.min) / range.span().max(1.0)).clamp(0.0, 1.0) as f64;
        let text = super::timestamp(if response.dragged() {
            seek
        } else {
            at * duration
        });
        let galley = painter.layout_no_wrap(text, egui::FontId::monospace(12.0), Color32::WHITE);
        let size = galley.size() + egui::vec2(14.0, 8.0);
        let anchor = if response.dragged() { x } else { pointer.x };
        let left = (anchor - size.x * 0.5).clamp(rect.left(), rect.right() - size.x);
        let bubble = egui::Rect::from_min_size(egui::pos2(left, rect.top() - size.y - 6.0), size);
        painter.rect_filled(bubble, 6, Color32::from_rgba_unmultiplied(22, 22, 22, 225));
        painter.galley(bubble.min + egui::vec2(7.0, 4.0), galley, Color32::WHITE);
    }
}

pub(super) fn volume_bar_id() -> egui::Id {
    egui::Id::new("player_volume_bar")
}

pub(super) fn volume_bar(ui: &mut egui::Ui, volume: &mut f64, enabled: bool) -> bool {
    let sense = if enabled {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::hover()
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(86.0, 20.0), egui::Sense::hover());
    let response = ui.interact(rect, volume_bar_id(), sense);
    let response = if enabled {
        response.on_hover_and_drag_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let mut next = None;
    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        next = Some(
            (((pointer.x - rect.left() - 5.0) / (rect.width() - 10.0)).clamp(0.0, 1.0) * 100.0)
                as f64,
        );
    }
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            );
        });
        let steps = ui.input(|i| {
            i.num_presses(egui::Key::ArrowRight) as i32 - i.num_presses(egui::Key::ArrowLeft) as i32
        });
        if steps != 0 {
            next = Some((*volume + f64::from(steps) * VOLUME_STEP).clamp(0.0, 100.0));
        }
    }
    let changed = next.is_some_and(|next| (next - *volume).abs() > f64::EPSILON);
    if changed {
        *volume = next.unwrap_or(*volume);
    }
    let value = *volume;
    response.widget_info(|| egui::WidgetInfo::slider(enabled, value, "Volume neste PC"));
    let fraction = (*volume / 100.0).clamp(0.0, 1.0) as f32;
    let x = rect.left() + 5.0 + (rect.width() - 10.0) * fraction;
    let y = rect.center().y;
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        y,
        Stroke::new(2.0, Color32::from_white_alpha(62)),
    );
    painter.hline(
        rect.left()..=x,
        y,
        Stroke::new(2.0, Color32::from_rgb(238, 238, 238)),
    );
    painter.circle_filled(egui::pos2(x, y), 5.0, Color32::WHITE);
    if response.has_focus() {
        painter.circle_stroke(egui::pos2(x, y), 8.0, Stroke::new(2.0, Color32::WHITE));
    }
    changed
}

enum CenterNotice {
    Paused,
    Lost,
}

fn center_notice(view: &View) -> Option<CenterNotice> {
    if !view.player.loaded
        || is_center_loading_visible(view)
        || view.error.is_some()
        || view.preparing
        || view.scheduled
        || view.player.blocked
    {
        return None;
    }
    if view.role != Role::Local && !view.connected {
        Some(CenterNotice::Lost)
    } else if view.player.paused {
        Some(CenterNotice::Paused)
    } else {
        None
    }
}

pub(super) fn video_dim(view: &View) -> u8 {
    match center_notice(view) {
        Some(CenterNotice::Lost) => 178,
        Some(CenterNotice::Paused) => 97,
        None => 0,
    }
}

pub(super) fn draw_center_notice(
    ui: &mut egui::Ui,
    safe_rect: egui::Rect,
    view: &View,
    position: String,
) {
    let Some(notice) = center_notice(view) else {
        return;
    };
    let solo = view.role == Role::Local;
    let peer_label = view.peer_name.as_deref().unwrap_or("Outra pessoa");
    let peer = view.peer_name.as_deref().unwrap_or("a outra pessoa");
    let (state, title, detail, diameter) = match notice {
        CenterNotice::Paused => (
            (!solo).then_some(Presence::Together),
            "Pausado".to_owned(),
            format!("em {position}"),
            22.0,
        ),
        CenterNotice::Lost if view.role == Role::Host => (
            Some(Presence::Lost),
            format!("{peer_label} perdeu a conexão"),
            format!("Esperando {peer} voltar…"),
            30.0,
        ),
        CenterNotice::Lost => (
            Some(Presence::Lost),
            "Conexão perdida".to_owned(),
            format!("Reconectando com {peer}…"),
            30.0,
        ),
    };
    let colors = Palette::new(true);
    let max_width = (safe_rect.width() - 32.0).max(120.0);
    let size = (safe_rect.width() * 0.024).clamp(22.0, 34.0);
    let title = kit::single_line(
        ui,
        RichText::new(title)
            .size(size)
            .extra_letter_spacing(-0.03 * size)
            .color(Color32::WHITE),
        max_width,
        egui::TextStyle::Heading,
    );
    let detail = kit::single_line(
        ui,
        RichText::new(detail)
            .size(15.0)
            .color(Color32::from_white_alpha(200)),
        max_width,
        egui::TextStyle::Body,
    );
    let pair_height = state.map_or(0.0, |_| diameter + 10.0);
    let height = pair_height + title.size().y + 5.0 + detail.size().y;
    let mut y = safe_rect.center().y - height * 0.5;
    let width = title.size().x.max(detail.size().x);
    let plate = egui::Rect::from_center_size(
        egui::pos2(safe_rect.center().x, safe_rect.center().y),
        egui::vec2(width + 40.0, height + 24.0),
    );
    let painter = ui.painter();
    painter.add(
        egui::Shadow {
            offset: [0, 0],
            blur: 48,
            spread: 4,
            color: Color32::from_black_alpha(72),
        }
        .as_shape(plate, 24),
    );
    if let Some(state) = state {
        let left = safe_rect.center().x - visuals::pair_width(state, diameter) * 0.5;
        visuals::paint_pair(
            painter,
            egui::pos2(left, y + diameter * 0.5),
            diameter,
            state,
            colors,
        );
        y += pair_height;
    }
    let title_height = title.size().y;
    painter.galley(
        egui::pos2(safe_rect.center().x - title.size().x * 0.5, y),
        title,
        Color32::WHITE,
    );
    y += title_height + 5.0;
    painter.galley(
        egui::pos2(safe_rect.center().x - detail.size().x * 0.5, y),
        detail,
        Color32::WHITE,
    );
}

fn draw_cached_ranges(
    painter: &egui::Painter,
    track: egui::Rect,
    height: f32,
    spans: &[(f32, f32)],
) {
    let color = Color32::from_white_alpha(126);
    let track_span = track.width().max(1.0);
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for &(start, end) in spans {
        let s = start.clamp(0.0, 1.0);
        let e = end.clamp(s, 1.0);
        if e <= s {
            continue;
        }
        let left = track.left() + track_span * s;
        let right = track.left() + track_span * e;
        if let Some(last) = merged.last_mut()
            && left <= last.1 + 2.0
        {
            last.1 = last.1.max(right);
        } else {
            merged.push((left, right));
        }
    }
    let cap = (height * 0.5).round() as u8;
    for (left, right) in merged {
        let width = (right - left).max(1.5);
        let right = (left + width).min(track.right());
        let left = (right - width).max(track.left());
        let left_cap = if left <= track.left() + 1.0 { cap } else { 0 };
        let right_cap = if right >= track.right() - 1.0 { cap } else { 0 };
        let rounding = CornerRadius {
            nw: left_cap,
            sw: left_cap,
            ne: right_cap,
            se: right_cap,
        };
        let span_rect = egui::Rect::from_min_max(
            egui::pos2(left, track.top()),
            egui::pos2(right, track.bottom()),
        );
        painter.rect_filled(span_rect, rounding, color);
    }
}

pub(super) fn should_keep_controls(view: &View) -> bool {
    view.buffering
        || view.peer_buffering
        || view.waiting_buffer
        || matches!(
            view.stream.phase,
            StreamPhase::Offering
                | StreamPhase::Connecting
                | StreamPhase::Probing
                | StreamPhase::Reconnecting
                | StreamPhase::Error(_)
        )
        || view.peer_stream.as_ref().is_some_and(|s| s.reconnecting)
}

pub(super) fn stream_hud_label(view: &View, wide: bool) -> Option<(String, Color32)> {
    let stream = EffectiveStream::from_view(view)?;
    let colors = Palette::new(true);
    let neutral = Color32::from_gray(225);
    let pct = (stream.cache_fraction() * 100.0).round() as u32;
    let card_visible = is_center_loading_visible(view);
    if stream.error.is_some() {
        let text = if card_visible {
            format!("{pct}% em cache")
        } else {
            format!("P2P sem conexão · {pct}%")
        };
        return Some((text, colors.warning));
    }
    if stream.reconnecting {
        let text = if card_visible {
            format!("{pct}% em cache")
        } else {
            format!("Reconectando P2P · {pct}%")
        };
        return Some((text, colors.warning));
    }
    if stream.connecting {
        return (!card_visible).then(|| ("Conectando P2P…".into(), colors.accent));
    }
    if stream.complete {
        return Some(("P2P · 100%".into(), colors.success));
    }
    let speed = format_bps(stream.transfer_bps);
    let mut parts = vec![format!("P2P {pct}%"), speed];
    if wide && let Some(secs) = stream.buffered_seconds() {
        parts.push(format!("{secs:.0}s à frente"));
    }
    let stalled = stream.transfer_bps == 0 || view.buffering || view.peer_buffering;
    Some((
        parts.join(" · "),
        if stalled { colors.warning } else { neutral },
    ))
}

pub(super) fn draw_stream_hud(ui: &mut egui::Ui, text: &str, color: Color32, max_width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(max_width, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(22, 22, 22, 170))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(7, 2))
                .show(ui, |ui| {
                    ui.set_max_width((max_width - 14.0).max(20.0));
                    ui.add(
                        egui::Label::new(RichText::new(text).size(12.0).color(color)).truncate(),
                    );
                });
        },
    );
}

pub(super) fn draw_center_loading(
    ui: &mut egui::Ui,
    safe_rect: egui::Rect,
    view: &View,
    actions: &mut Vec<Command>,
) -> Option<egui::Rect> {
    if !is_center_loading_visible(view) || safe_rect.height() < 40.0 {
        return None;
    }
    let stream = EffectiveStream::from_view(view);
    let stream_reconnecting = stream.as_ref().is_some_and(|s| s.reconnecting);
    let stream_connecting = stream.as_ref().is_some_and(|s| s.connecting);
    let stream_error = stream.as_ref().and_then(|s| s.error.as_deref());
    let colors = Palette::new(true);

    let peer = view.peer_name.as_deref().unwrap_or("a outra pessoa");
    let (title, glyph, accent) = if stream_error.is_some() {
        (
            "Conexão P2P interrompida".to_owned(),
            Glyph::Alert,
            colors.warning,
        )
    } else if stream_reconnecting {
        (
            if view.role == Role::Host {
                format!("Reconectando streaming P2P com {peer}…")
            } else {
                "Reconectando streaming P2P…".to_owned()
            },
            Glyph::Spinner,
            colors.warning,
        )
    } else if stream_connecting {
        (
            "Conectando streaming P2P…".to_owned(),
            Glyph::Spinner,
            colors.accent,
        )
    } else if view.buffering {
        (
            "Carregando vídeo…".to_owned(),
            Glyph::Spinner,
            colors.accent,
        )
    } else if view.peer_buffering {
        (
            format!("Aguardando {peer} carregar o vídeo…"),
            Glyph::Spinner,
            colors.accent,
        )
    } else {
        (
            "Carregando para retomar…".to_owned(),
            Glyph::Spinner,
            colors.accent,
        )
    };

    let detail = if let Some(err) = stream_error {
        Some(err.to_owned())
    } else if let Some(s) = &stream {
        if s.total_bytes > 0 {
            let pct = (s.cache_fraction() * 100.0).round() as u32;
            let mut metrics = vec![format!("{pct}% em cache"), format_bps(s.transfer_bps)];
            if let Some(secs) = s.buffered_seconds() {
                metrics.push(format!("{secs:.0}s à frente"));
            }
            Some(metrics.join(" · "))
        } else {
            None
        }
    } else {
        None
    };

    let bounds = safe_rect.shrink(8.0);
    let card_width = bounds.width().clamp(200.0, 380.0);
    let card_id = ui.id().with("player_loading_card");
    let default_h = if stream_error.is_some() && view.can_stream {
        118.0
    } else if detail.is_some() {
        80.0
    } else {
        52.0
    };
    let prev_size = ui
        .ctx()
        .data(|d| d.get_temp::<egui::Vec2>(card_id))
        .unwrap_or_else(|| egui::vec2(card_width, default_h));
    let left = (bounds.center().x - prev_size.x * 0.5).clamp(bounds.left(), bounds.right());
    let top = (bounds.center().y - prev_size.y * 0.5).clamp(bounds.top(), bounds.bottom());
    let host_rect = egui::Rect::from_min_max(
        egui::pos2(left, top),
        egui::pos2((left + card_width).min(bounds.right()), bounds.bottom()),
    );
    let card_rect = ui
        .scope_builder(egui::UiBuilder::new().max_rect(host_rect), |ui| {
            let frame = egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(22, 22, 22, 232))
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_max_width((card_width - 32.0).max(140.0));
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (icon, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        kit::draw(
                            ui.painter(),
                            glyph,
                            icon.center(),
                            16.0,
                            accent,
                            ui.input(|i| i.time),
                        );
                        if glyph == Glyph::Spinner {
                            ui.ctx().request_repaint();
                        }
                        ui.add(
                            egui::Label::new(
                                crate::ui::strong(ui, &title)
                                    .size(15.0)
                                    .color(Color32::WHITE),
                            )
                            .wrap(),
                        );
                    });
                    if let Some(detail) = &detail {
                        ui.add(
                            egui::Label::new(RichText::new(detail).size(13.0).color(colors.muted))
                                .wrap(),
                        );
                    }
                    if stream_error.is_some() && view.can_stream {
                        ui.add_space(2.0);
                        if kit::Button::new(kit::Kind::Primary, "Reconectar P2P")
                            .glyph(Glyph::Refresh)
                            .size(kit::Size::Small)
                            .show(ui, colors)
                            .clicked()
                        {
                            actions.push(Command::StartStream);
                        }
                    }
                });
            frame.response.rect
        })
        .inner;
    let actual_size = card_rect.size();
    if (actual_size - prev_size).length() > 1.0 {
        ui.ctx().data_mut(|d| d.insert_temp(card_id, actual_size));
        ui.ctx().request_repaint();
    }
    Some(card_rect)
}
