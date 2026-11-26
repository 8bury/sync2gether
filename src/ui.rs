use eframe::egui::{self, Color32, Stroke};

pub use crate::player_ui::{PlayerUi, Video};
mod browse;
mod copies;
mod demo;
mod entry;
pub(crate) mod kit;
mod preparation;
mod room;
mod settings;
mod stream;
mod theme;
pub(crate) mod visuals;

pub use demo::{draw, draw_with_inputs};
pub(crate) use room::{media_title, preparation_leave_label, request_leave};
pub use theme::configure_theme;
pub(crate) use theme::{Palette, strong};
use {
    room::{confirm_leave, room_addresses, room_contents},
    settings::header,
    theme::{feedback, medium_family, messages, quiet, section_label, title},
};

pub fn web_base<'a>(explicit: Option<&'a str>, relay: &str) -> Option<&'a str> {
    use crate::network::relay::{DEFAULT_RELAY, DEFAULT_WEB_URL, relay_address};
    match explicit.map(str::trim) {
        Some("") => None,
        Some(url) => Some(url),
        None => (relay_address(relay).as_deref() == Ok(DEFAULT_RELAY)).then_some(DEFAULT_WEB_URL),
    }
}

pub fn web_link(base: Option<&str>, view: &crate::runtime::View) -> Option<String> {
    let base = base?.trim().trim_end_matches('/');
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|prefix| base.starts_with(prefix));
    if !(base.starts_with("https://") || local) || base.contains('#') {
        return None;
    }
    let code = view.room_code.as_deref()?;
    (view.internet && view.role == crate::runtime::Role::Host).then(|| format!("{base}/#{code}"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Home,
    Join,
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
    pub focus_code: bool,
    pub dismissed_error: Option<String>,
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
            focus_code: false,
            dismissed_error: None,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Body {
    Home,
    Join,
    Browse,
    Room,
    Connecting,
    Preparation,
}

fn back_label(view: &crate::runtime::View, body: Body) -> Option<&'static str> {
    match body {
        Body::Home | Body::Join | Body::Browse => None,
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
    if view.error.is_none() {
        inputs.dismissed_error = None;
    }
    if inputs.last_role != Role::Local && view.role == Role::Local {
        inputs.screen = if inputs.joining {
            Screen::Join
        } else {
            Screen::Home
        };
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
                    .frame(egui::Frame::new().fill(colors.background).inner_margin(20))
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
        let body = if joining || inputs.screen == Screen::Join {
            Body::Join
        } else if inputs.screen == Screen::Home {
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
        let narrow = ui.available_width() < 860.0;
        let gutter: i8 = if narrow { 16 } else { 28 };
        let short = ui.available_height() < 560.0;
        egui::Panel::top("app_header")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(gutter, if short { 6 } else { 14 })),
            )
            .show(ui, |ui| {
                header(ui, colors, inputs, view, body, &mut actions);
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
                .exact_size(132.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(colors.background)
                        .inner_margin(egui::Margin::symmetric(gutter, 12)),
                )
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(ui.available_width().min(680.0));
                        preparation::action(ui, colors, view, &mut actions);
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin {
                        left: gutter,
                        right: gutter,
                        top: 0,
                        bottom: if short { 8 } else { gutter },
                    }),
            )
            .show(ui, |ui| {
                let bounds = ui.max_rect();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        centered_content(ui, body, bounds.height(), |ui| {
                            draw_body(
                                ui,
                                colors,
                                view,
                                inputs,
                                body,
                                preparation::Fit {
                                    sticky: sticky_preparation,
                                    available: bounds.height(),
                                },
                                &mut actions,
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

fn centered_content(
    ui: &mut egui::Ui,
    body: Body,
    available: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.id().with(("centered_content", body));
    let measured = ui.ctx().data(|data| data.get_temp::<f32>(id));
    ui.add_space(((available - measured.unwrap_or(available)) * 0.46).max(0.0));
    let height = ui.scope(add).response.rect.height();
    if measured.is_none_or(|previous| (previous - height).abs() > 1.0) {
        ui.ctx().data_mut(|data| data.insert_temp(id, height));
        ui.ctx().request_discard("center content");
    }
}

fn draw_body(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    body: Body,
    fit: preparation::Fit,
    actions: &mut Vec<crate::runtime::Command>,
) {
    match body {
        Body::Home => entry::home(ui, colors, view, inputs, actions),
        Body::Join => entry::join(ui, colors, view, inputs, actions),
        Body::Browse | Body::Room | Body::Connecting | Body::Preparation => {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width().min(match body {
                    Body::Preparation => 1120.0,
                    Body::Room => 760.0,
                    Body::Connecting => 460.0,
                    _ => 640.0,
                }));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| match body {
                    Body::Browse => browse::browse(ui, colors, view, inputs, actions),
                    Body::Connecting => browse::connecting(ui, colors, view, actions),
                    Body::Room => preparation::room(ui, colors, view, actions),
                    _ => preparation::draw(ui, colors, view, fit, actions),
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Role, View};

    #[test]
    fn browser_links_need_a_safe_base_and_an_internet_room_hosted_here() {
        let view = View {
            role: Role::Host,
            internet: true,
            room_code: Some("4821-gato-lua".into()),
            ..View::default()
        };
        assert_eq!(
            web_link(Some("https://assistir.example/"), &view).as_deref(),
            Some("https://assistir.example/#4821-gato-lua")
        );
        assert_eq!(
            web_link(Some("http://localhost:8080"), &view).as_deref(),
            Some("http://localhost:8080/#4821-gato-lua")
        );
        assert_eq!(web_link(None, &view), None);
        assert_eq!(web_link(Some("http://assistir.example"), &view), None);
        assert_eq!(web_link(Some("https://a.example/#x"), &view), None);
        let guest = View {
            role: Role::Guest,
            ..view.clone()
        };
        assert_eq!(web_link(Some("https://assistir.example"), &guest), None);
    }

    #[test]
    fn the_published_page_is_the_default_only_for_the_default_relay() {
        use crate::network::relay::{DEFAULT_RELAY, DEFAULT_WEB_URL};
        assert_eq!(web_base(None, DEFAULT_RELAY), Some(DEFAULT_WEB_URL));
        assert_eq!(
            web_base(None, "relay.pedrosales.dev"),
            Some(DEFAULT_WEB_URL)
        );
        assert_eq!(web_base(None, "relay.example.com:7843"), None);
        assert_eq!(
            web_base(Some("https://a.example"), "relay.example.com"),
            Some("https://a.example")
        );
        assert_eq!(web_base(Some(""), DEFAULT_RELAY), None);
    }
}
