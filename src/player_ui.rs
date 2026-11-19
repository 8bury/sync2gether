use crate::ui::kit::{self, Glyph};
use crate::{
    player::TrackKind,
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, Color32, RichText, Stroke};

const ACCENT: Color32 = Color32::from_rgb(122, 156, 255);

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
        || view.notice.is_some();
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
            let back = crate::ui::preparation_leave_label(view.role);
            if glass_icon(ui, Glyph::ArrowLeft, back).clicked() {
                crate::ui::request_leave(view, inputs, &mut actions);
            }
            let pill = room_pill(view);
            let pill_width = glass_width(ui, &pill.label);
            let title_width = (ui.available_width() - pill_width - 16.0).max(20.0);
            ui.allocate_ui_with_layout(
                egui::vec2(title_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(title_width);
                    ui.add(
                        egui::Label::new(
                            crate::ui::strong(
                                ui,
                                view.file
                                    .as_deref()
                                    .map(crate::ui::media_title)
                                    .unwrap_or("Seu filme, juntos"),
                            )
                            .size(16.5)
                            .color(Color32::WHITE),
                        )
                        .truncate(),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if glass(ui, &pill, inputs.room_panel).clicked() {
                    inputs.room_panel = !inputs.room_panel;
                }
            });
        });
        if has_notice {
            let status = if view.preparing || view.scheduled {
                "Preparando a reprodução…"
            } else if view.player.blocked {
                "A reprodução foi interrompida. Aguardando o player…"
            } else {
                &view.status
            };
            let glyph = if view.error.is_some() || view.player.blocked {
                Glyph::Alert
            } else if view.preparing || view.scheduled {
                Glyph::Spinner
            } else {
                Glyph::Info
            };
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(Color32::from_black_alpha(165))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
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
                                Color32::from_rgb(255, 170, 150)
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
        }
    });
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(controls_rect.shrink2(egui::vec2(16.0, 8.0))),
        |ui| {
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            ui.visuals_mut().selection.bg_fill = ACCENT;
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
            seek_bar(ui, &response, seek, view.player.duration, can_seek);
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
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{} / {}",
                        timestamp(seek),
                        timestamp(view.player.duration)
                    ))
                    .size(12.5)
                    .color(Color32::from_gray(215)),
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
fn seek_bar(ui: &egui::Ui, response: &egui::Response, seek: f64, duration: f64, enabled: bool) {
    let rect = response.rect;
    let active = enabled && (response.hovered() || response.dragged());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id.with("seek_hover"), active, 0.12);
    let radius = rect.height() / 2.5;
    let range = rect.x_range().shrink(radius);
    let fraction = if duration > 0.0 {
        (seek / duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let x = range.min + range.span() * fraction;
    let height = 4.0 + 2.0 * hover;
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), height));
    let painter = ui.painter();
    painter.rect_filled(track, height * 0.5, Color32::from_white_alpha(56));
    let mut filled = track;
    filled.max.x = x.max(track.left() + height);
    painter.rect_filled(
        filled,
        height * 0.5,
        if enabled {
            ACCENT
        } else {
            ACCENT.gamma_multiply(0.6)
        },
    );
    if enabled {
        let center = egui::pos2(x, rect.center().y);
        painter.circle_filled(
            center + egui::vec2(0.0, 1.0),
            5.0 + 2.5 * hover,
            Color32::from_black_alpha(90),
        );
        painter.circle_filled(center, 4.5 + 2.5 * hover, Color32::WHITE);
    }
    if enabled && let Some(pointer) = response.hover_pos() {
        let at = ((pointer.x - range.min) / range.span().max(1.0)).clamp(0.0, 1.0) as f64;
        let text = timestamp(if response.dragged() {
            seek
        } else {
            at * duration
        });
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::WHITE);
        let size = galley.size() + egui::vec2(14.0, 8.0);
        let anchor = if response.dragged() { x } else { pointer.x };
        let left = (anchor - size.x * 0.5).clamp(rect.left(), rect.right() - size.x);
        let bubble = egui::Rect::from_min_size(egui::pos2(left, rect.top() - size.y - 6.0), size);
        painter.rect_filled(bubble, 6, Color32::from_black_alpha(200));
        painter.galley(bubble.min + egui::vec2(7.0, 4.0), galley, Color32::WHITE);
    }
}

struct Pill {
    label: String,
    glyph: Option<Glyph>,
    warning: bool,
}

fn room_pill(view: &View) -> Pill {
    let peer = view.peer_name.as_deref().unwrap_or("a outra pessoa");
    if view.role == Role::Local {
        return Pill {
            label: "Sala".into(),
            glyph: Some(Glyph::People),
            warning: false,
        };
    }
    if !view.connected {
        return Pill {
            label: if view.role == Role::Host {
                format!("Esperando {peer} voltar…")
            } else {
                format!("Reconectando com {peer}…")
            },
            glyph: Some(Glyph::Spinner),
            warning: true,
        };
    }
    Pill {
        label: format!(
            "{} · em sincronia",
            view.peer_name.as_deref().unwrap_or("Outra pessoa")
        ),
        glyph: None,
        warning: false,
    }
}

fn glass_icon(ui: &mut egui::Ui, glyph: Glyph, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let painter = ui.painter();
    painter.circle(
        rect.center(),
        16.0,
        kit::lerp(
            Color32::from_black_alpha(130),
            Color32::from_white_alpha(40),
            hover,
        ),
        Stroke::new(1.0, Color32::from_white_alpha(40)),
    );
    if response.has_focus() {
        painter.circle_stroke(rect.center(), 18.0, Stroke::new(2.0, ACCENT));
    }
    kit::draw(painter, glyph, rect.center(), 15.0, Color32::WHITE, 0.0);
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

const PILL_FONT: f32 = 13.5;
const PILL_MAX: f32 = 280.0;

fn glass_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley = egui::WidgetText::from(crate::ui::strong(ui, label).size(PILL_FONT)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    (galley.size().x + 12.0 + 20.0 + 8.0 + 14.0).min(PILL_MAX)
}

fn glass(ui: &mut egui::Ui, pill: &Pill, selected: bool) -> egui::Response {
    let width = glass_width(ui, &pill.label);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let label = pill.label.clone();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let response = response
        .on_hover_text("Detalhes da sala")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let warning = Color32::from_rgb(244, 192, 104);
    let ink = if pill.warning {
        warning
    } else {
        Color32::WHITE
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        16,
        if selected {
            Color32::from_white_alpha(60)
        } else {
            kit::lerp(
                Color32::from_black_alpha(130),
                Color32::from_white_alpha(40),
                hover,
            )
        },
        Stroke::new(
            1.0,
            if pill.warning {
                warning.gamma_multiply(0.55)
            } else {
                Color32::from_white_alpha(40)
            },
        ),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            18,
            Stroke::new(2.0, ACCENT),
            egui::StrokeKind::Outside,
        );
    }
    let slot = egui::pos2(rect.left() + 6.0 + 10.0, rect.center().y);
    match pill.glyph {
        Some(glyph) => {
            kit::draw(painter, glyph, slot, 15.0, ink, ui.input(|i| i.time));
            if glyph == Glyph::Spinner {
                ui.ctx().request_repaint();
            }
        }
        None => {
            let green = Color32::from_rgb(92, 214, 164);
            painter.circle_filled(slot, 7.0, green.gamma_multiply(0.22));
            painter.circle_filled(slot, 3.5, green);
        }
    }
    let galley = egui::WidgetText::from(
        crate::ui::strong(ui, &pill.label)
            .size(PILL_FONT)
            .color(ink),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        width - 6.0 - 20.0 - 8.0 - 14.0,
        egui::TextStyle::Button,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y * 0.5),
        galley,
        ink,
    );
    response
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
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let c = response.rect.center();
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered() && enabled || response.has_focus(),
        0.12,
    );
    if hover > 0.0 {
        ui.painter()
            .circle_filled(c, 18.0, Color32::from_white_alpha((30.0 * hover) as u8));
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
                    c + egui::vec2(-5.0, -8.5),
                    c + egui::vec2(9.0, 0.0),
                    c + egui::vec2(-5.0, 8.5),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [-4.5, 4.5] {
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(c + egui::vec2(x, 0.0), egui::vec2(3.5, 16.0)),
                    1.5,
                    color,
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
