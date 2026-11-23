use eframe::egui::{self, Color32, Stroke};

pub use crate::player_ui::{PlayerUi, Video};
mod demo;
mod entry;
pub(crate) mod kit;
mod preparation;
mod room;
mod settings;
mod stream;
mod theme;
mod visuals;

pub use demo::{draw, draw_with_inputs};
pub(crate) use room::{media_title, preparation_leave_label, request_leave};
pub use theme::configure_theme;
pub(crate) use theme::{Palette, strong};
use {
    room::{confirm_leave, connection_details, room_addresses, room_contents},
    settings::header,
    theme::{card, feedback, quiet, secondary, section_label, semibold_family, title},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Home,
    Browse,
    Preparation,
    Watching,
}

pub struct Inputs {
    pub address: String,
    pub port: u16,
    pub code: String,
    pub relay: String,
    pub lan: bool,
    pub code_entry: bool,
    pub code_edited: bool,
    pub joining: bool,
    pub confirm_leave: bool,
    pub screen: Screen,
    pub room_panel: bool,
    pub volume: f64,
    pub seek: Option<f64>,
    pub player: PlayerUi,
    pub unmuted_volume: f64,
    last_role: crate::runtime::Role,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            address: String::new(),
            port: crate::protocol::PORT,
            code: String::new(),
            relay: crate::network::relay::DEFAULT_RELAY.into(),
            lan: false,
            code_entry: false,
            code_edited: false,
            joining: false,
            confirm_leave: false,
            screen: Screen::Home,
            room_panel: false,
            volume: 100.0,
            seek: None,
            player: PlayerUi::default(),
            unmuted_volume: 100.0,
            last_role: crate::runtime::Role::Local,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    Home,
    Browse,
    Room,
    Connecting,
    Preparation,
}

fn back_label(view: &crate::runtime::View, body: Body) -> Option<&'static str> {
    match body {
        Body::Home | Body::Browse => None,
        _ => Some(preparation::leave_label(view.role)),
    }
}

pub fn draw_live(
    ui: &mut egui::Ui,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    video: Option<Video<'_>>,
) -> Vec<crate::runtime::Command> {
    draw_flow(ui, view, inputs, video, false)
}

fn draw_flow(
    ui: &mut egui::Ui,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    video: Option<Video<'_>>,
    demo: bool,
) -> Vec<crate::runtime::Command> {
    use crate::runtime::Role;
    let mut actions = Vec::new();
    let colors = Palette::new(ui.visuals().dark_mode);
    let sticky_preparation = ui.ctx().content_rect().height() < 600.0;
    if inputs.last_role != Role::Local && view.role == Role::Local {
        inputs.screen = Screen::Home;
        inputs.joining = false;
        inputs.confirm_leave = false;
        inputs.room_panel = false;
        inputs.player.fullscreen = false;
    }
    inputs.last_role = view.role;
    if view.connected || view.role == Role::Host {
        inputs.joining = false;
    }
    if view.role == Role::Local
        && !view.loading
        && view.file.is_none()
        && inputs.screen == Screen::Preparation
    {
        inputs.screen = Screen::Home;
    }
    if view.role != Role::Local && !(inputs.joining && !view.connected) {
        if inputs.screen == Screen::Watching && !view.watching {
            inputs.player.fullscreen = false;
        }
        inputs.screen = if view.watching {
            Screen::Watching
        } else {
            Screen::Preparation
        };
    } else if view.loading || view.watching {
        inputs.screen = if view.player.loaded && !view.loading {
            Screen::Watching
        } else {
            Screen::Preparation
        };
    }
    if inputs.player.fullscreen || inputs.screen == Screen::Watching {
        if inputs.room_panel {
            if ui.available_width() < 720.0 {
                let mut open = true;
                egui::Window::new("Sala")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .default_width(280.0)
                    .show(ui.ctx(), |ui| {
                        room_contents(ui, colors, view, inputs, &mut actions)
                    });
                inputs.room_panel &= open;
            } else {
                egui::Panel::right("room_panel")
                    .exact_size(280.0)
                    .resizable(false)
                    .frame(egui::Frame::new().fill(colors.surface).inner_margin(20))
                    .show(ui, |ui| {
                        let rect = ui.clip_rect();
                        ui.painter().vline(
                            rect.left() + 0.5,
                            rect.y_range(),
                            Stroke::new(1.0, colors.border),
                        );
                        room_contents(ui, colors, view, inputs, &mut actions)
                    });
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                if demo {
                    visuals::demo_video(ui.painter(), rect);
                }
                actions.extend(crate::player_ui::draw(ui, rect, view, inputs, video, demo));
            });
    } else {
        inputs.player.video_rect = None;
        let joining = inputs.joining && view.role == Role::Guest && !view.connected;
        let body = if inputs.screen == Screen::Home || joining {
            Body::Home
        } else if inputs.screen == Screen::Browse {
            Body::Browse
        } else if view.role == Role::Host && !view.connected {
            Body::Room
        } else if view.role == Role::Guest && !view.connected {
            Body::Connecting
        } else {
            Body::Preparation
        };
        let step = match body {
            Body::Home | Body::Browse => None,
            _ if view.role == Role::Local => None,
            Body::Room | Body::Connecting => Some(0),
            Body::Preparation => Some(1),
        };
        egui::Panel::top("app_header")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                header(ui, colors, inputs, view, step, body, &mut actions);
                if step.is_some() {
                    let rect = ui.clip_rect();
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        Stroke::new(1.0, colors.border),
                    );
                }
            });
        if back_label(view, body).is_some()
            && !inputs.confirm_leave
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.ctx().memory(|m| m.focused().is_none())
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            request_leave(view, inputs, &mut actions);
        }
        if body == Body::Preparation && sticky_preparation && view.join_request.is_none() {
            egui::Panel::bottom("preparation_action")
                .exact_size(148.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(colors.background).inner_margin(
                    egui::Margin::symmetric(if ui.available_width() < 600.0 { 24 } else { 40 }, 16),
                ))
                .show(ui, |ui| {
                    let rect = ui.clip_rect();
                    ui.painter().hline(
                        rect.x_range(),
                        rect.top() + 0.5,
                        Stroke::new(1.0, colors.border),
                    );
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(ui.available_width().min(680.0));
                        preparation::action(ui, colors, view, &mut actions);
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.background).inner_margin(
                if body == Body::Preparation {
                    egui::Margin::symmetric(24, 16)
                } else {
                    egui::Margin::same(if ui.available_width() < 600.0 { 24 } else { 40 })
                },
            ))
            .show(ui, |ui| {
                let bounds = ui.max_rect();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let height = match body {
                            Body::Home => 430.0,
                            Body::Room => 400.0,
                            Body::Connecting => 440.0,
                            Body::Browse | Body::Preparation => bounds.height(),
                        };
                        ui.add_space(((bounds.height() - height) * 0.4).clamp(0.0, 90.0));
                        if body == Body::Home {
                            entry::home(ui, colors, view, inputs, &mut actions);
                            return;
                        }
                        ui.vertical_centered(|ui| {
                            ui.set_max_width(ui.available_width().min(match body {
                                Body::Preparation => 680.0,
                                Body::Room => 460.0,
                                _ => 600.0,
                            }));
                            ui.with_layout(
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| match body {
                                    Body::Browse => {
                                        entry::browse(ui, colors, view, inputs, &mut actions)
                                    }
                                    Body::Connecting => {
                                        entry::connecting(ui, colors, view, &mut actions)
                                    }
                                    Body::Room => preparation::room(ui, colors, view, &mut actions),
                                    Body::Preparation => preparation::draw(
                                        ui,
                                        colors,
                                        view,
                                        sticky_preparation,
                                        &mut actions,
                                    ),
                                    Body::Home => {}
                                },
                            );
                        });
                    });
            });
    }

    if inputs.confirm_leave {
        confirm_leave(ui.ctx(), colors, view, inputs, &mut actions);
    }
    if demo {
        actions.clear();
    }
    actions
}
