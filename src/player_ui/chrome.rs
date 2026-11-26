use super::overlay;
use crate::{
    runtime::{Command, Role, View},
    stream::StreamPhase,
    ui::{
        Palette,
        kit::{self, Glyph},
        visuals::{self, Presence},
    },
};
use eframe::egui::{self, Color32, RichText, Stroke};

const PRESENCE_MAX: f32 = 320.0;
const PAIR: f32 = 14.0;

pub(super) fn palette() -> Palette {
    Palette {
        muted: Color32::from_rgb(216, 216, 213),
        hover: Color32::from_white_alpha(26),
        ..Palette::new(true)
    }
}

struct PeerStatus {
    name: String,
    detail: String,
    state: Presence,
    warning: bool,
}

fn peer_status(strings: &'static crate::i18n::Strings, view: &View) -> PeerStatus {
    let t = &strings.player;
    let status = |name: &str, detail: &str, state, warning| PeerStatus {
        name: name.to_owned(),
        detail: detail.to_owned(),
        state,
        warning,
    };
    if view.role == Role::Local {
        return status(t.alone, "", Presence::Solo, false);
    }
    let peer = view
        .peer_name
        .as_deref()
        .unwrap_or(strings.common.other_person);
    if view.peer_left && !view.connected {
        return status(peer, t.peer_left, Presence::Solo, false);
    }
    if !view.connected {
        let detail = if view.role == Role::Host {
            t.peer_disconnected
        } else {
            t.peer_reconnecting
        };
        return status(peer, detail, Presence::Lost, true);
    }
    let card_visible = overlay::is_center_loading_visible(view);
    if view.peer_buffering {
        let detail = if card_visible {
            t.peer_loading
        } else {
            t.peer_waiting_video
        };
        return status(peer, detail, Presence::Waiting, true);
    }
    if view.buffering || view.waiting_buffer {
        return status(peer, t.peer_loading_video, Presence::Waiting, true);
    }
    if view.stream.phase == StreamPhase::Reconnecting
        || view.peer_stream.as_ref().is_some_and(|s| s.reconnecting)
    {
        let detail = if card_visible {
            t.peer_p2p
        } else {
            t.peer_reconnecting_p2p
        };
        return status(peer, detail, Presence::Lost, true);
    }
    status(peer, t.peer_in_sync, Presence::Together, false)
}

pub(super) fn top_bar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: &View,
    inputs: &mut crate::ui::Inputs,
    actions: &mut Vec<Command>,
) {
    let colors = palette();
    let strings = crate::ui::tr(ui);
    let label = crate::ui::preparation_leave_label(strings, view.role);
    let compact = rect.width() < 520.0;
    let back = kit::Button::new(kit::Kind::Ghost, label)
        .glyph(Glyph::ChevronLeft)
        .size(kit::Size::Small);
    let back_width = if compact {
        36.0
    } else {
        back.natural_width(ui)
    };
    let back_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + back_width * 0.5, rect.center().y),
        egui::vec2(back_width, if compact { 36.0 } else { 32.0 }),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(back_rect), |ui| {
        let clicked = if compact {
            kit::icon_button(ui, colors, Glyph::ChevronLeft, label, 36.0).clicked()
        } else {
            back.show(ui, colors).clicked()
        };
        if clicked {
            crate::ui::request_leave(view, inputs, actions);
        }
    });
    let status = peer_status(crate::ui::tr(ui), view);
    let limit = PRESENCE_MAX.min(rect.width() * 0.5);
    let width = presence_width(ui, &status).min(limit);
    let presence_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - width * 0.5, rect.center().y),
        egui::vec2(width, 36.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(presence_rect), |ui| {
        if presence(ui, colors, &status, width, inputs.room_panel).clicked() {
            inputs.room_panel = !inputs.room_panel;
        }
    });
    let side = back_width.max(width);
    let centered = rect.width() - 2.0 * side - 24.0;
    let title_rect = if centered >= 96.0 {
        egui::Rect::from_center_size(rect.center(), egui::vec2(centered, 32.0))
    } else {
        egui::Rect::from_min_max(
            egui::pos2(back_rect.right() + 8.0, rect.center().y - 16.0),
            egui::pos2(
                (presence_rect.left() - 8.0).max(back_rect.right() + 9.0),
                rect.center().y + 16.0,
            ),
        )
    };
    let builder =
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(egui::Layout::centered_and_justified(
                egui::Direction::LeftToRight,
            ));
    ui.scope_builder(builder, |ui| {
        ui.add(
            egui::Label::new(
                crate::ui::strong(
                    ui,
                    view.file
                        .as_deref()
                        .map(crate::ui::media_title)
                        .unwrap_or(crate::ui::tr(ui).player.movie_together),
                )
                .size(15.0)
                .color(Color32::WHITE),
            )
            .truncate(),
        );
    });
}

fn name_text(ui: &egui::Ui, status: &PeerStatus) -> RichText {
    crate::ui::strong(ui, &status.name)
        .size(15.0)
        .color(Color32::WHITE)
}

fn detail_text(status: &PeerStatus, colors: Palette) -> RichText {
    RichText::new(&status.detail)
        .size(13.0)
        .color(if status.warning {
            colors.warning
        } else {
            Color32::from_rgb(164, 164, 161)
        })
}

fn text_width(ui: &egui::Ui, text: RichText) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

fn presence_width(ui: &egui::Ui, status: &PeerStatus) -> f32 {
    let name = text_width(ui, name_text(ui, status));
    let detail = if status.detail.is_empty() {
        0.0
    } else {
        8.0 + text_width(ui, detail_text(status, palette()))
    };
    visuals::pair_width(status.state, PAIR) + 8.0 + name + detail + 16.0
}

fn presence(
    ui: &mut egui::Ui,
    colors: Palette,
    status: &PeerStatus,
    width: f32,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::click());
    let label = format!("{} {}", status.name, status.detail);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.trim_end())
    });
    let response = response
        .on_hover_text(crate::ui::tr(ui).player.room_details)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.15);
    let wash = hover.max(if selected { 1.0 } else { 0.0 });
    if wash > 0.0 {
        ui.painter()
            .rect_filled(rect, 6, Color32::from_white_alpha((26.0 * wash) as u8));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            8,
            Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Outside,
        );
    }
    let pair = visuals::pair_width(status.state, PAIR);
    visuals::paint_pair(
        ui.painter(),
        egui::pos2(rect.left() + 8.0, rect.center().y),
        PAIR,
        status.state,
        Palette::new(true),
    );
    let detail = (!status.detail.is_empty()).then(|| {
        kit::single_line(
            ui,
            detail_text(status, colors),
            rect.width() * 0.5,
            egui::TextStyle::Small,
        )
    });
    let detail_width = detail.as_ref().map_or(0.0, |galley| galley.size().x + 8.0);
    let name_room = rect.width() - 16.0 - pair - 8.0 - detail_width;
    let name = kit::single_line(ui, name_text(ui, status), name_room, egui::TextStyle::Body);
    let name_x = rect.left() + 8.0 + pair + 8.0;
    let detail_x = name_x + name.size().x + 8.0;
    ui.painter().galley(
        egui::pos2(name_x, rect.center().y - name.size().y * 0.5),
        name,
        Color32::WHITE,
    );
    if let Some(detail) = detail {
        ui.painter().galley(
            egui::pos2(detail_x, rect.center().y - detail.size().y * 0.5),
            detail,
            colors.muted,
        );
    }
    response
}
