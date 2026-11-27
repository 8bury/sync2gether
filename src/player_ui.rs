mod chrome;
mod icons;
mod overlay;

use crate::ui::kit::{self, Glyph};
use crate::{
    player::TrackKind,
    protocol::Control,
    runtime::{Command, Role, View},
    stream::StreamPhase,
};
use eframe::egui::{self, Color32, RichText, Stroke};
use icons::{Icon, icon_button};

const ACCENT: Color32 = Color32::from_rgb(
    sync2gether_core::theme::DARK.accent.0,
    sync2gether_core::theme::DARK.accent.1,
    sync2gether_core::theme::DARK.accent.2,
);
const WIDE_PLAYER: f32 = sync2gether_core::theme::PLAYER_WIDE_PX as f32;
const CONTROLS_HEIGHT: f32 = sync2gether_core::theme::PLAYER_CONTROLS_H_PX as f32;

pub enum Video<'a> {
    Texture(&'a egui::TextureHandle),
    OpenGl(egui::PaintCallback),
}
#[derive(Default)]
pub struct PlayerUi {
    pub fullscreen: bool,
    pub controls_visible: bool,
    pub video_rect: Option<egui::Rect>,
    loading_card_rect: Option<egui::Rect>,
    last_activity: f64,
    last_pointer: Option<egui::Pos2>,
    last_generation: u64,
    keyboard_navigation: bool,
}
impl PlayerUi {
    fn update(
        &mut self,
        now: f64,
        pointer: Option<egui::Pos2>,
        rect: egui::Rect,
        keep: bool,
        activity: bool,
    ) -> bool {
        if keep
            || activity
            || (pointer != self.last_pointer && pointer.is_some_and(|p| rect.contains(p)))
        {
            self.last_activity = now;
        }
        self.last_pointer = pointer;
        self.controls_visible = keep || now - self.last_activity < 3.0;
        self.controls_visible
    }
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: &View,
    inputs: &mut crate::ui::Inputs,
    video: Option<Video<'_>>,
    demo: bool,
) -> Vec<Command> {
    let t = &crate::ui::tr(ui).player;
    let mut actions = Vec::new();
    if !demo {
        ui.painter().rect_filled(rect, 0, Color32::BLACK);
        match video {
            Some(Video::OpenGl(mut callback)) => {
                callback.rect = rect;
                ui.painter().add(egui::Shape::Callback(callback));
            }
            Some(Video::Texture(texture)) => {
                let image = fit_video(rect, texture.size_vec2());
                ui.painter().image(
                    texture.id(),
                    image,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {}
        }
        if !view.player.loaded {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if view.loading {
                    t.preparing_movie
                } else {
                    t.open_local_copy
                },
                egui::FontId::proportional(18.0),
                Color32::from_gray(190),
            );
        }
    }
    let response = ui.interact(rect, ui.id().with("video"), egui::Sense::click());
    let can_seek = !demo && view.player.loaded && (view.role == Role::Local || view.matched);
    let can_toggle = !demo
        && view.player.loaded
        && (!view.player.paused
            || view.preparing
            || view.scheduled
            || view.waiting_buffer
            || view.role == Role::Local
            || view.ready);
    let shortcut_focus = (response.hovered() || response.has_focus() || inputs.player.fullscreen)
        && !inputs.room_panel
        && !ui.ctx().text_edit_focused()
        && !egui::Popup::is_any_open(ui.ctx());
    if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
        inputs.player.keyboard_navigation = true;
    }
    if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed()) {
        inputs.player.keyboard_navigation = false;
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let over_loading_card = inputs
        .player
        .loading_card_rect
        .is_some_and(|card| pointer.is_some_and(|p| card.contains(p)));
    let volume_focused = ui.memory(|m| m.has_focus(overlay::volume_bar_id()));
    let mut activity = response.clicked()
        || ui.input(|i| {
            i.key_pressed(egui::Key::Tab)
                || volume_focused
                    && (i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowRight))
        });
    if response.clicked() && !over_loading_card {
        response.request_focus();
        if can_toggle {
            actions.push(Command::Control(toggle(view)));
        }
    }
    if shortcut_focus {
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Space) && can_toggle {
                actions.push(Command::Control(toggle(view)));
                activity = true;
            }
            for (key, delta) in [(egui::Key::ArrowLeft, -10.0), (egui::Key::ArrowRight, 10.0)] {
                if !volume_focused && i.consume_key(egui::Modifiers::NONE, key) && can_seek {
                    actions.push(Command::Control(Control::Seek(
                        (view.player.position + delta).clamp(0.0, view.player.duration),
                    )));
                    activity = true;
                }
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::F) {
                inputs.player.fullscreen = !inputs.player.fullscreen;
                activity = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::M) && !demo {
                mute(inputs);
                actions.push(Command::Volume(inputs.volume));
                activity = true;
            }
        });
    }
    if inputs.player.fullscreen
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        inputs.player.fullscreen = false;
        activity = true;
    }
    if inputs.player.last_generation != view.player.generation {
        inputs.seek = None;
        inputs.player.last_generation = view.player.generation;
        activity = true;
    }
    inputs.player.video_rect = Some(rect);
    let now = ui.input(|i| i.time);
    let controls_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CONTROLS_HEIGHT),
        rect.right_bottom(),
    );
    let keep = inputs.room_panel
        || !view.player.loaded
        || view.player.paused
        || view.player.blocked
        || view.preparing
        || view.scheduled
        || view.role != Role::Local && !view.connected
        || overlay::should_keep_controls(view)
        || inputs.seek.is_some()
        || egui::Popup::is_any_open(ui.ctx())
        || pointer.is_some_and(|p| controls_rect.contains(p))
        || inputs.player.keyboard_navigation
            && ui
                .ctx()
                .memory(|m| m.focused().is_some_and(|id| id != response.id));
    let visible = inputs.player.update(now, pointer, rect, keep, activity);
    if !visible {
        inputs.player.loading_card_rect =
            overlay::draw_center_loading(ui, rect, view, &mut actions);
        if response.hovered() && !over_loading_card {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        return actions;
    }
    if !keep {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                (inputs.player.last_activity + 3.0 - now).max(0.01),
            ));
    }
    let hard_blocked = view.player.blocked && !view.buffering && !view.player.buffering;
    let suppress_stream_notice = matches!(
        view.stream.phase,
        StreamPhase::Reconnecting | StreamPhase::Error(_)
    );
    let has_notice = view.error.is_some()
        || view.preparing
        || view.scheduled
        || hard_blocked
        || (view.notice.is_some() && !suppress_stream_notice);
    let dim = overlay::video_dim(view);
    if dim > 0 {
        ui.painter()
            .rect_filled(rect, 0, Color32::from_black_alpha(dim));
    }
    let bar_height: f32 = if rect.height() < 420.0 { 52.0 } else { 64.0 };
    let side = if rect.width() < WIDE_PLAYER {
        16.0
    } else {
        28.0
    };
    gradient(
        ui.painter(),
        egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), (bar_height * 1.4).min(rect.height())),
        ),
        true,
    );
    gradient(
        ui.painter(),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), controls_rect.top() - 26.0),
            rect.max,
        ),
        false,
    );
    chrome::top_bar(
        ui,
        egui::Rect::from_min_max(
            rect.min + egui::vec2(side, 0.0),
            egui::pos2(rect.right() - side, rect.top() + bar_height),
        ),
        view,
        inputs,
        &mut actions,
    );
    if has_notice {
        let notice = egui::Rect::from_min_max(
            egui::pos2(rect.left() + side, rect.top() + bar_height + 4.0),
            egui::pos2(rect.right() - side, rect.top() + bar_height + 100.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(notice), |ui| {
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            let status = if view.preparing || view.scheduled {
                crate::ui::tr(ui).common.preparing_playback
            } else if hard_blocked {
                t.interrupted
            } else {
                &view.status
            };
            let glyph = if view.error.is_some() || hard_blocked {
                Glyph::Alert
            } else if view.preparing || view.scheduled {
                Glyph::Spinner
            } else {
                Glyph::Info
            };
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(22, 22, 22, 225))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(520.0));
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (icon, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
                        kit::draw(
                            ui.painter(),
                            glyph,
                            icon.center(),
                            15.0,
                            if glyph == Glyph::Alert {
                                crate::ui::Palette::new(true).warning
                            } else {
                                ACCENT
                            },
                            ui.input(|i| i.time),
                        );
                        if glyph == Glyph::Spinner {
                            ui.ctx().request_repaint();
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    view.error
                                        .as_deref()
                                        .or(view.notice.as_deref())
                                        .unwrap_or(status),
                                )
                                .color(Color32::WHITE),
                            )
                            .wrap(),
                        );
                    });
                });
        });
    }
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(controls_rect.shrink2(egui::vec2(side, 8.0))),
        |ui| {
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            ui.visuals_mut().selection.bg_fill = ACCENT;
            ui.visuals_mut().widgets.inactive.bg_fill = Color32::WHITE;
            ui.visuals_mut().widgets.hovered.bg_fill = Color32::WHITE;
            ui.visuals_mut().widgets.active.bg_fill = Color32::WHITE;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 6.0);
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
            ui.spacing_mut().slider_width = ui.available_width();
            ui.spacing_mut().interact_size.y = 18.0;
            ui.visuals_mut().handle_shape = egui::style::HandleShape::Circle;
            let mut seek = inputs.seek.unwrap_or(view.player.position);
            let response = ui
                .scope(|ui| {
                    let visuals = ui.visuals_mut();
                    visuals.selection.bg_fill = Color32::TRANSPARENT;
                    for widget in [
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                    ] {
                        widget.bg_fill = Color32::TRANSPARENT;
                        widget.fg_stroke = Stroke::NONE;
                        widget.expansion = 0.0;
                    }
                    ui.add_enabled(
                        can_seek,
                        egui::Slider::new(&mut seek, 0.0..=view.player.duration.max(0.1))
                            .show_value(false)
                            .trailing_fill(true),
                    )
                })
                .inner;
            let cached_ranges = overlay::effective_cached_ranges(view);
            overlay::seek_bar(
                ui,
                &response,
                seek,
                view.player.duration,
                can_seek,
                &cached_ranges,
                overlay::peer_position(view),
            );
            if response.dragged() {
                inputs.seek = Some(seek);
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                actions.push(Command::Control(Control::Seek(seek)));
                inputs.seek = None;
            }
            ui.horizontal(|ui| {
                let cancelling = view.preparing || view.scheduled || view.waiting_buffer;
                let icon = if cancelling {
                    Icon::Cancel
                } else if view.player.paused {
                    Icon::Play
                } else {
                    Icon::Pause
                };
                let tooltip = if cancelling {
                    t.cancel_operation
                } else if view.player.paused {
                    t.play
                } else {
                    t.pause
                };
                if icon_button(ui, icon, can_toggle, false, tooltip).clicked() {
                    actions.push(Command::Control(toggle(view)));
                }
                if icon_button(ui, Icon::Back, can_seek, false, t.back).clicked() {
                    actions.push(Command::Control(Control::Seek(
                        (view.player.position - 10.0).max(0.0),
                    )));
                }
                if icon_button(ui, Icon::Forward, can_seek, false, t.forward).clicked() {
                    actions.push(Command::Control(Control::Seek(
                        (view.player.position + 10.0).min(view.player.duration),
                    )));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let font = egui::FontId::monospace(13.0);
                    ui.label(
                        RichText::new(timestamp(seek))
                            .font(font.clone())
                            .color(ACCENT),
                    );
                    ui.label(
                        RichText::new(format!(" / {}", timestamp(view.player.duration)))
                            .font(font)
                            .color(Color32::from_rgb(164, 164, 161)),
                    );
                });
                let wide = rect.width() >= WIDE_PLAYER;
                let hud_max_w = ui.available_width() - if wide { 240.0 } else { 136.0 };
                if hud_max_w >= 88.0
                    && let Some((hud_text, hud_color)) =
                        overlay::stream_hud_label(crate::ui::tr(ui), view, hud_max_w >= 210.0)
                {
                    ui.add_space(6.0);
                    overlay::draw_stream_hud(ui, &hud_text, hud_color, hud_max_w - 6.0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(
                        ui,
                        Icon::Fullscreen,
                        true,
                        false,
                        if inputs.player.fullscreen {
                            t.exit_fullscreen
                        } else {
                            t.fullscreen
                        },
                    )
                    .clicked()
                    {
                        inputs.player.fullscreen = !inputs.player.fullscreen;
                    }
                    let subtitled = view
                        .player
                        .tracks
                        .iter()
                        .any(|t| t.kind == TrackKind::Subtitle && t.selected);
                    let tracks =
                        icon_button(ui, Icon::Captions, true, subtitled, t.audio_subtitles);
                    egui::Popup::menu(&tracks).show(|ui| track_menu(ui, view, demo, &mut actions));
                    if wide {
                        ui.add_space(6.0);
                        if overlay::volume_bar(ui, &mut inputs.volume, !demo) {
                            actions.push(Command::Volume(inputs.volume));
                        }
                        ui.add_space(2.0);
                    }
                    let volume = icon_button(
                        ui,
                        Icon::Volume(inputs.volume == 0.0),
                        true,
                        false,
                        t.volume,
                    );
                    if wide {
                        if volume.clicked() && !demo {
                            mute(inputs);
                            actions.push(Command::Volume(inputs.volume));
                        }
                    } else {
                        egui::Popup::menu(&volume).show(|ui| {
                            ui.label(crate::ui::tr(ui).common.volume_here);
                            ui.spacing_mut().slider_width = 160.0;
                            ui.add_enabled_ui(!demo, |ui| {
                                if ui
                                    .add(
                                        egui::Slider::new(&mut inputs.volume, 0.0..=100.0)
                                            .suffix(" %"),
                                    )
                                    .changed()
                                {
                                    actions.push(Command::Volume(inputs.volume));
                                }
                                if ui
                                    .button(if inputs.volume == 0.0 {
                                        t.unmute
                                    } else {
                                        t.mute
                                    })
                                    .clicked()
                                {
                                    mute(inputs);
                                    actions.push(Command::Volume(inputs.volume));
                                }
                            });
                        });
                    }
                });
            });
        },
    );
    let safe_top = rect.top() + bar_height + if has_notice { 76.0 } else { 0.0 };
    let safe_bottom = (controls_rect.top() - 4.0).max(safe_top + 48.0);
    let safe_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), safe_top),
        egui::pos2(rect.right(), safe_bottom.min(rect.bottom())),
    );
    overlay::draw_center_notice(ui, safe_rect, view, timestamp(view.player.position));
    inputs.player.loading_card_rect =
        overlay::draw_center_loading(ui, safe_rect, view, &mut actions);
    actions
}
fn track_menu(ui: &mut egui::Ui, view: &View, demo: bool, actions: &mut Vec<Command>) {
    let t = &crate::ui::tr(ui).player;
    ui.set_max_width(280.0);
    if demo {
        ui.label(t.demo_tracks);
    }
    ui.add_enabled_ui(!demo && view.player.loaded, |ui| {
        for (kind, title) in [
            (TrackKind::Audio, t.audio_here),
            (TrackKind::Subtitle, t.subtitles_here),
        ] {
            ui.label(RichText::new(title).strong());
            if kind == TrackKind::Subtitle
                && ui
                    .selectable_label(
                        !view
                            .player
                            .tracks
                            .iter()
                            .any(|t| t.kind == kind && t.selected),
                        t.subtitles_off,
                    )
                    .clicked()
            {
                actions.push(Command::SubtitleTrack(None));
                ui.close();
            }
            let mut any = false;
            for track in view.player.tracks.iter().filter(|t| t.kind == kind) {
                any = true;
                if ui
                    .selectable_label(track.selected, track.label(t.track))
                    .clicked()
                {
                    actions.push(match kind {
                        TrackKind::Audio => Command::AudioTrack(track.id),
                        TrackKind::Subtitle => Command::SubtitleTrack(Some(track.id)),
                    });
                    ui.close();
                }
            }
            if !any {
                ui.label(t.no_tracks);
            }
            ui.separator();
        }
        if view.role == Role::Host {
            ui.label(RichText::new(t.shared_subtitle).strong());
            if let Some(name) = &view.shared_subtitle {
                ui.add(egui::Label::new(name).truncate())
                    .on_hover_text(name);
                if ui.button(t.change_shared_subtitle).clicked() {
                    actions.push(Command::ShareSubtitle);
                    ui.close();
                }
                if ui.button(t.stop_sharing_subtitle).clicked() {
                    actions.push(Command::ClearSharedSubtitle);
                    ui.close();
                }
            } else if ui.button(t.share_subtitle).clicked() {
                actions.push(Command::ShareSubtitle);
                ui.close();
            }
        }
    });
}
fn toggle(view: &View) -> Control {
    if view.player.paused && !view.preparing && !view.scheduled && !view.waiting_buffer {
        Control::Play
    } else {
        Control::Pause
    }
}
fn mute(inputs: &mut crate::ui::Inputs) {
    if inputs.volume > 0.0 {
        inputs.unmuted_volume = inputs.volume;
        inputs.volume = 0.0;
    } else {
        inputs.volume = inputs.unmuted_volume.max(1.0);
    }
}
fn timestamp(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
fn fit_video(rect: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    egui::Rect::from_center_size(rect.center(), size * scale)
}
fn gradient(painter: &egui::Painter, rect: egui::Rect, top: bool) {
    let scrim = |alpha| Color32::from_rgba_unmultiplied(8, 8, 8, alpha);
    let mut mesh = egui::Mesh::default();
    for (pos, alpha) in [
        (rect.left_top(), if top { 178 } else { 0 }),
        (rect.right_top(), if top { 178 } else { 0 }),
        (rect.right_bottom(), if top { 0 } else { 199 }),
        (rect.left_bottom(), if top { 0 } else { 199 }),
    ] {
        mesh.colored_vertex(pos, scrim(alpha));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_mouse_hides_controls_movement_and_interaction_restore_them() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 360.0));
        let pointer = Some(egui::pos2(200.0, 100.0));
        let mut state = PlayerUi::default();
        assert!(state.update(1.0, pointer, rect, false, false));
        assert!(!state.update(4.1, pointer, rect, false, false));
        assert!(state.update(4.2, Some(egui::pos2(201.0, 100.0)), rect, false, false));
        assert!(state.update(20.0, pointer, rect, true, false));
        assert!(state.update(25.0, pointer, rect, false, true));
        assert!(!state.update(29.0, Some(egui::pos2(800.0, 100.0)), rect, false, false));
    }
}
