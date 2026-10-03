//! Controles do player: apenas estado egui e comandos devolvidos à aplicação.
use crate::{
    player::TrackKind,
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, Color32, RichText};

pub enum Video<'a> {
    Texture(&'a egui::TextureHandle),
    OpenGl(egui::PaintCallback),
}
#[derive(Default)]
pub struct PlayerUi {
    pub fullscreen: bool,
    pub controls_visible: bool,
    pub video_rect: Option<egui::Rect>,
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
                    "Preparando seu filme…"
                } else {
                    "Abra uma cópia local do filme"
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
    let mut activity = response.clicked() || ui.input(|i| i.key_pressed(egui::Key::Tab));
    if response.clicked() {
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
                if i.consume_key(egui::Modifiers::NONE, key) && can_seek {
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
    // Escape também funciona com um controle focado; um menu aberto consome primeiro.
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
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let panel_height = if rect.width() < 560.0 { 132.0 } else { 106.0 };
    let controls_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - panel_height),
        rect.right_bottom(),
    );
    let keep = inputs.room_panel
        || !view.player.loaded
        || view.player.paused
        || view.player.blocked
        || view.preparing
        || view.scheduled
        || inputs.seek.is_some()
        || egui::Popup::is_any_open(ui.ctx())
        || pointer.is_some_and(|p| controls_rect.contains(p))
        || inputs.player.keyboard_navigation
            && ui
                .ctx()
                .memory(|m| m.focused().is_some_and(|id| id != response.id));
    let visible = inputs.player.update(now, pointer, rect, keep, activity);
    if !visible {
        if response.hovered() {
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
    let has_notice = view.error.is_some()
        || view.preparing
        || view.scheduled
        || view.player.blocked
        || view.notice.is_some()
        || (view.role != Role::Local && !view.connected);
    let top_height: f32 = if has_notice { 160.0 } else { 80.0 };
    gradient(
        ui.painter(),
        egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), top_height.min(rect.height())),
        ),
        true,
    );
    gradient(
        ui.painter(),
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), controls_rect.top() - 32.0),
            rect.max,
        ),
        false,
    );
    let top = egui::Rect::from_min_max(
        rect.min + egui::vec2(16.0, 12.0),
        egui::pos2(rect.right() - 16.0, rect.top() + top_height - 10.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        ui.horizontal(|ui| {
            let title_width = (ui.available_width() - 78.0).max(20.0);
            ui.allocate_ui_with_layout(
                egui::vec2(title_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(title_width);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                view.file
                                    .as_deref()
                                    .map(crate::ui::media_title)
                                    .unwrap_or("Seu filme, juntos"),
                            )
                            .size(18.0)
                            .strong()
                            .color(Color32::WHITE),
                        )
                        .truncate(),
                    );
                },
            );
            if ui
                .add(
                    egui::Button::new("Sala")
                        .frame(false)
                        .min_size(egui::vec2(64.0, 32.0)),
                )
                .clicked()
            {
                inputs.room_panel = !inputs.room_panel;
            }
        });
        if has_notice {
            let status = if view.role != Role::Local && !view.connected {
                if view.role == Role::Host {
                    "Aguardando a outra pessoa voltar. O filme está pausado."
                } else {
                    "Reconectando ao anfitrião… O filme está pausado."
                }
            } else if view.preparing || view.scheduled {
                "Preparando a reprodução…"
            } else if view.player.blocked {
                "A reprodução foi interrompida. Aguardando o player…"
            } else {
                &view.status
            };
            ui.add(
                egui::Label::new(
                    view.error
                        .as_deref()
                        .or(view.notice.as_deref())
                        .unwrap_or(status),
                )
                .wrap(),
            );
            if view.role != Role::Local
                && !view.connected
                && ui.small_button("Opções da sala").clicked()
            {
                inputs.room_panel = true;
            }
        }
    });
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(controls_rect.shrink2(egui::vec2(16.0, 8.0))),
        |ui| {
            // Controles claros sobre o vídeo, também no tema claro da janela.
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            ui.visuals_mut().selection.bg_fill = Color32::from_rgb(104, 145, 255);
            ui.visuals_mut().widgets.inactive.bg_fill = Color32::WHITE;
            ui.visuals_mut().widgets.hovered.bg_fill = Color32::WHITE;
            ui.visuals_mut().widgets.active.bg_fill = Color32::WHITE;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 6.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 8.0);
            ui.spacing_mut().slider_width = ui.available_width();
            ui.spacing_mut().interact_size.y = 18.0;
            ui.visuals_mut().handle_shape = egui::style::HandleShape::Circle;
            let mut seek = inputs.seek.unwrap_or(view.player.position);
            let response = ui
                .add_enabled(
                    can_seek,
                    egui::Slider::new(&mut seek, 0.0..=view.player.duration.max(0.1))
                        .show_value(false)
                        .trailing_fill(true),
                )
                .on_hover_text("Posição no filme");
            if response.dragged() {
                inputs.seek = Some(seek);
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                actions.push(Command::Control(Control::Seek(seek)));
                inputs.seek = None;
            }
            ui.horizontal(|ui| {
                let icon = if view.player.paused && !view.preparing && !view.scheduled {
                    Icon::Play
                } else {
                    Icon::Pause
                };
                let tooltip = if view.preparing || view.scheduled {
                    "Cancelar operação"
                } else if view.player.paused {
                    "Reproduzir · Espaço"
                } else {
                    "Pausar · Espaço"
                };
                if icon_button(ui, icon, can_toggle, tooltip).clicked() {
                    actions.push(Command::Control(toggle(view)));
                }
                if icon_button(ui, Icon::Back, can_seek, "Voltar 10 segundos · ←").clicked() {
                    actions.push(Command::Control(Control::Seek(
                        (view.player.position - 10.0).max(0.0),
                    )));
                }
                if icon_button(ui, Icon::Forward, can_seek, "Avançar 10 segundos · →").clicked()
                {
                    actions.push(Command::Control(Control::Seek(
                        (view.player.position + 10.0).min(view.player.duration),
                    )));
                }
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        timestamp(seek),
                        timestamp(view.player.duration)
                    ))
                    .size(12.0)
                    .color(Color32::from_gray(205)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(
                        ui,
                        Icon::Fullscreen,
                        true,
                        if inputs.player.fullscreen {
                            "Sair da tela cheia · Esc"
                        } else {
                            "Tela cheia · F"
                        },
                    )
                    .clicked()
                    {
                        inputs.player.fullscreen = !inputs.player.fullscreen;
                    }
                    let tracks = icon_button(ui, Icon::Captions, true, "Áudio e legendas");
                    egui::Popup::menu(&tracks).show(|ui| track_menu(ui, view, demo, &mut actions));
                    let volume = icon_button(
                        ui,
                        Icon::Volume(inputs.volume == 0.0),
                        true,
                        "Volume · M para silenciar",
                    );
                    egui::Popup::menu(&volume).show(|ui| {
                        ui.label("Volume neste PC");
                        ui.spacing_mut().slider_width = 160.0;
                        ui.add_enabled_ui(!demo, |ui| {
                            if ui
                                .add(
                                    egui::Slider::new(&mut inputs.volume, 0.0..=100.0).suffix(" %"),
                                )
                                .changed()
                            {
                                actions.push(Command::Volume(inputs.volume));
                            }
                            if ui
                                .button(if inputs.volume == 0.0 {
                                    "Ativar som"
                                } else {
                                    "Silenciar"
                                })
                                .clicked()
                            {
                                mute(inputs);
                                actions.push(Command::Volume(inputs.volume));
                            }
                        });
                    });
                });
            });
        },
    );
    actions
}
fn track_menu(ui: &mut egui::Ui, view: &View, demo: bool, actions: &mut Vec<Command>) {
    ui.set_max_width(280.0);
    if demo {
        ui.label("Seleção ilustrativa, sem reprodução");
    }
    ui.add_enabled_ui(!demo && view.player.loaded, |ui| {
        for (kind, title) in [
            (TrackKind::Audio, "Áudio neste PC"),
            (TrackKind::Subtitle, "Legendas neste PC"),
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
                        "Desativadas",
                    )
                    .clicked()
            {
                actions.push(Command::SubtitleTrack(None));
                ui.close();
            }
            let mut any = false;
            for track in view.player.tracks.iter().filter(|t| t.kind == kind) {
                any = true;
                if ui.selectable_label(track.selected, track.label()).clicked() {
                    actions.push(match kind {
                        TrackKind::Audio => Command::AudioTrack(track.id),
                        TrackKind::Subtitle => Command::SubtitleTrack(Some(track.id)),
                    });
                    ui.close();
                }
            }
            if !any {
                ui.label("Nenhuma faixa disponível");
            }
            ui.separator();
        }
    });
}
fn toggle(view: &View) -> Control {
    if view.player.paused && !view.preparing && !view.scheduled {
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
    let mut mesh = egui::Mesh::default();
    for (pos, alpha) in [
        (rect.left_top(), if top { 210 } else { 0 }),
        (rect.right_top(), if top { 210 } else { 0 }),
        (rect.right_bottom(), if top { 0 } else { 230 }),
        (rect.left_bottom(), if top { 0 } else { 230 }),
    ] {
        mesh.colored_vertex(pos, Color32::from_black_alpha(alpha));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}
enum Icon {
    Play,
    Pause,
    Back,
    Forward,
    Volume(bool),
    Captions,
    Fullscreen,
}
fn icon_button(ui: &mut egui::Ui, icon: Icon, enabled: bool, tooltip: &str) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2(36.0, 36.0)),
        )
        .on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
    let c = response.rect.center();
    if response.hovered() && enabled || response.has_focus() {
        ui.painter()
            .circle_filled(c, 18.0, Color32::from_white_alpha(28));
    }
    let color = if enabled {
        Color32::WHITE
    } else {
        Color32::from_gray(100)
    };
    match icon {
        Icon::Play => {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(-4.0, -7.0),
                    c + egui::vec2(7.0, 0.0),
                    c + egui::vec2(-4.0, 7.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [-4.0, 4.0] {
                ui.painter().line_segment(
                    [c + egui::vec2(x, -7.0), c + egui::vec2(x, 7.0)],
                    egui::Stroke::new(3.0, color),
                );
            }
        }
        Icon::Back | Icon::Forward => {
            let direction = if matches!(icon, Icon::Back) {
                -1.0
            } else {
                1.0
            };
            let arc = (0..=24)
                .map(|i| {
                    let angle = -std::f32::consts::FRAC_PI_2
                        + i as f32 / 24.0 * std::f32::consts::TAU * 0.82;
                    c + egui::vec2(angle.cos() * -direction, angle.sin()) * 11.0
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(arc, egui::Stroke::new(1.5, color)));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(direction * 5.0, -11.0),
                    c + egui::vec2(-direction * 1.0, -15.0),
                    c + egui::vec2(-direction * 1.0, -7.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                "10",
                egui::FontId::proportional(10.0),
                color,
            );
        }
        Icon::Volume(muted) => {
            ui.painter().add(egui::Shape::closed_line(
                vec![
                    c + egui::vec2(-10.0, -3.0),
                    c + egui::vec2(-6.0, -3.0),
                    c + egui::vec2(0.0, -8.0),
                    c + egui::vec2(0.0, 8.0),
                    c + egui::vec2(-6.0, 3.0),
                    c + egui::vec2(-10.0, 3.0),
                ],
                egui::Stroke::new(1.5, color),
            ));
            if muted {
                for y in [-1.0, 1.0] {
                    ui.painter().line_segment(
                        [c + egui::vec2(4.0, y * 4.0), c + egui::vec2(10.0, -y * 4.0)],
                        egui::Stroke::new(1.5, color),
                    );
                }
            } else {
                for radius in [7.0, 11.0] {
                    ui.painter().add(egui::Shape::line(
                        (0..=12)
                            .map(|i| {
                                let a = -0.7 + i as f32 / 12.0 * 1.4;
                                c + egui::vec2(a.cos(), a.sin()) * radius
                            })
                            .collect(),
                        egui::Stroke::new(1.5, color),
                    ));
                }
            }
        }
        Icon::Captions => {
            ui.painter().rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(22.0, 17.0)),
                3,
                egui::Stroke::new(1.5, color),
                egui::StrokeKind::Inside,
            );
            for y in [-2.0, 3.0] {
                for (left, right) in [(-6.0, -1.0), (2.0, 6.0)] {
                    ui.painter().line_segment(
                        [c + egui::vec2(left, y), c + egui::vec2(right, y)],
                        egui::Stroke::new(1.5, color),
                    );
                }
            }
        }
        Icon::Fullscreen => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = c + egui::vec2(x * 7.0, y * 6.0);
                ui.painter().line_segment(
                    [corner, corner - egui::vec2(x * 4.0, 0.0)],
                    egui::Stroke::new(1.5, color),
                );
                ui.painter().line_segment(
                    [corner, corner - egui::vec2(0.0, y * 4.0)],
                    egui::Stroke::new(1.5, color),
                );
            }
        }
    }
    response
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
