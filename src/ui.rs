//! Telas sem acesso a arquivos, ao player ou à rede.

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::model::{Action, SessionState};
pub use crate::player_ui::{PlayerUi, Video};
mod entry;
pub(crate) mod kit;
mod preparation;
mod visuals;

#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    background: Color32,
    surface: Color32,
    inset: Color32,
    raised: Color32,
    hover: Color32,
    disabled: Color32,
    action: Color32,
    action_top: Color32,
    border: Color32,
    border_strong: Color32,
    text: Color32,
    muted: Color32,
    faint: Color32,
    accent: Color32,
    accent_strong: Color32,
    success: Color32,
    warning: Color32,
    error: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                background: Color32::from_rgb(13, 14, 18),
                surface: Color32::from_rgb(21, 22, 28),
                inset: Color32::from_rgb(17, 18, 23),
                raised: Color32::from_rgb(29, 31, 38),
                hover: Color32::from_rgb(37, 39, 48),
                disabled: Color32::from_rgb(25, 27, 33),
                action: Color32::from_rgb(70, 104, 240),
                action_top: Color32::from_rgb(101, 133, 255),
                border: Color32::from_rgb(38, 40, 49),
                border_strong: Color32::from_rgb(60, 63, 76),
                text: Color32::from_rgb(240, 242, 247),
                muted: Color32::from_rgb(150, 155, 170),
                faint: Color32::from_rgb(98, 103, 118),
                accent: Color32::from_rgb(122, 156, 255),
                accent_strong: Color32::from_rgb(160, 184, 255),
                success: Color32::from_rgb(92, 214, 164),
                warning: Color32::from_rgb(244, 192, 104),
                error: Color32::from_rgb(255, 134, 134),
            }
        } else {
            Self {
                dark,
                background: Color32::from_rgb(246, 247, 250),
                surface: Color32::WHITE,
                inset: Color32::from_rgb(243, 244, 248),
                raised: Color32::WHITE,
                hover: Color32::from_rgb(236, 238, 244),
                disabled: Color32::from_rgb(240, 241, 245),
                action: Color32::from_rgb(60, 92, 228),
                action_top: Color32::from_rgb(88, 120, 248),
                border: Color32::from_rgb(226, 229, 236),
                border_strong: Color32::from_rgb(200, 205, 216),
                text: Color32::from_rgb(17, 19, 27),
                muted: Color32::from_rgb(96, 102, 118),
                faint: Color32::from_rgb(150, 155, 168),
                accent: Color32::from_rgb(58, 90, 226),
                accent_strong: Color32::from_rgb(36, 64, 190),
                success: Color32::from_rgb(16, 128, 88),
                warning: Color32::from_rgb(164, 100, 8),
                error: Color32::from_rgb(200, 48, 62),
            }
        }
    }
}

/// Aplica o tema também usado pelos testes de layout.
pub fn configure_theme(ctx: &egui::Context, light: bool) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        (
            "sans",
            include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice(),
        ),
        (
            "sans-semibold",
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(data).into());
    }
    let proportional = fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap();
    proportional.insert(0, "sans".into());
    let mut semibold = proportional.clone();
    semibold.insert(0, "sans-semibold".into());
    fonts
        .families
        .insert(egui::FontFamily::Name("sans-semibold".into()), semibold);
    ctx.set_fonts(fonts);
    let colors = Palette::new(!light);
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::new(1.0, colors.border);
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.menu_corner_radius = egui::CornerRadius::same(12);
    let shadow = Color32::from_black_alpha(if light { 28 } else { 110 });
    visuals.window_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: shadow,
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow,
    };
    visuals.extreme_bg_color = colors.inset;
    visuals.text_edit_bg_color = Some(colors.inset);
    visuals.faint_bg_color = colors.surface;
    visuals.code_bg_color = colors.inset;
    visuals.override_text_color = Some(colors.text);
    visuals.hyperlink_color = colors.accent;
    visuals.weak_text_color = Some(colors.muted);
    visuals.selection.bg_fill = colors
        .accent
        .gamma_multiply(if light { 0.16 } else { 0.24 });
    visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    visuals.text_cursor.stroke = Stroke::new(2.0, colors.accent);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.active,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
        widget.bg_fill = colors.raised;
        widget.weak_bg_fill = colors.raised;
        widget.bg_stroke = Stroke::new(1.0, colors.border);
        widget.fg_stroke = Stroke::new(1.5, colors.text);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_fill = colors.surface;
    visuals.widgets.noninteractive.weak_bg_fill = colors.surface;
    visuals.widgets.noninteractive.fg_stroke.color = colors.muted;
    visuals.widgets.hovered.bg_fill = colors.hover;
    visuals.widgets.hovered.weak_bg_fill = colors.hover;
    visuals.widgets.hovered.bg_stroke.color = colors.border_strong;
    visuals.widgets.active.bg_fill = colors.hover;
    visuals.widgets.active.weak_bg_fill = colors.hover;
    visuals.widgets.active.bg_stroke.color = colors.accent;
    visuals.widgets.open.weak_bg_fill = colors.hover;
    visuals.widgets.open.bg_stroke.color = colors.border_strong;
    ctx.set_visuals(visuals);
    ctx.style_mut_of(
        if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        },
        |style| {
            style.spacing.item_spacing = egui::vec2(12.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
            style.spacing.interact_size.y = 34.0;
            style.spacing.menu_margin = egui::Margin::same(10);
            style.spacing.menu_spacing = 4.0;
            style.spacing.scroll = egui::style::ScrollStyle::floating();
            style.animation_time = 0.14;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        },
    );
}

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
pub fn draw(ui: &mut egui::Ui, state: SessionState, demo: bool) -> Vec<Action> {
    draw_with_inputs(ui, state, demo, &mut Inputs::default())
}
pub fn draw_with_inputs(
    ui: &mut egui::Ui,
    state: SessionState,
    demo: bool,
    inputs: &mut Inputs,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if demo {
        let colors = Palette::new(ui.visuals().dark_mode);
        egui::Panel::top("demo_controls")
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    egui::Frame::new()
                        .fill(colors.warning.gamma_multiply(0.14))
                        .stroke(Stroke::new(1.0, colors.warning.gamma_multiply(0.4)))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.label(
                                strong(ui, "DEMO OFFLINE")
                                    .size(10.5)
                                    .extra_letter_spacing(0.6)
                                    .color(colors.warning),
                            );
                        });
                    ui.menu_button(
                        RichText::new(state.label()).size(13.5).color(colors.muted),
                        |ui| {
                            ui.label(
                                RichText::new("Cenários fictícios, sem player ou rede")
                                    .small()
                                    .color(colors.muted),
                            );
                            ui.separator();
                            for candidate in SessionState::ALL {
                                if ui
                                    .selectable_label(state == candidate, candidate.label())
                                    .clicked()
                                {
                                    inputs.screen = Screen::Home;
                                    inputs.room_panel = false;
                                    actions.push(Action::SetDemoState(candidate));
                                    ui.close();
                                }
                            }
                        },
                    );
                });
            });
    }
    let view = demo_view(state);
    if state == SessionState::Browsing {
        inputs.screen = Screen::Browse;
    }
    let _ = draw_flow(ui, &view, inputs, None, demo);
    actions
}
fn demo_view(state: SessionState) -> crate::runtime::View {
    use crate::runtime::{JoinRequest, Role, View};
    let mut view = View::default();
    if state == SessionState::Idle {
        return view;
    }
    if matches!(
        state,
        SessionState::Connecting | SessionState::PendingApproval
    ) {
        view.role = Role::Guest;
        view.peer_name = Some("Ana".into());
        view.room_address = Some("192.168.1.10:7842".parse().unwrap());
        view.waiting_approval = state == SessionState::PendingApproval;
        return view;
    }
    if state == SessionState::Browsing {
        view.rooms.push(crate::discovery::Room {
            name: "Ana".into(),
            address: "100.64.0.2:7842".parse().unwrap(),
        });
        return view;
    }
    view.role = Role::Host;
    view.file = Some("Filme de demonstração.mkv".into());
    view.verified = true;
    view.player.loaded = true;
    view.player.paused = state != SessionState::Synchronized;
    view.player.duration = 5400.0;
    view.player.position = 754.0;
    view.connected = !matches!(
        state,
        SessionState::Waiting | SessionState::Approval | SessionState::Reconnecting
    );
    view.peer_name = Some("notebook-ana".into());
    view.peer_ready = view.connected;
    view.peer_verified = view.connected;
    view.peer_file = view
        .connected
        .then(|| "Filme.de.demonstração.1080p.mkv".into());
    view.room_address = Some("192.168.1.10:7842".parse().unwrap());
    view.matched = view.connected;
    view.ready = view.connected;
    view.watching = matches!(
        state,
        SessionState::Synchronized | SessionState::Paused | SessionState::Reconnecting
    );
    view.status = state.label().into();
    if state == SessionState::Approval {
        view.join_request = Some(JoinRequest {
            id: 1,
            name: "notebook-ana".into(),
            address: "100.64.0.2:45678".parse().unwrap(),
        });
    }
    match state {
        SessionState::Choosing => {
            view.file = None;
            view.verified = false;
            view.player.loaded = false;
            view.peer_file = None;
            view.peer_verified = false;
            view.peer_ready = false;
            view.matched = false;
            view.ready = false;
        }
        SessionState::Verifying => {
            view.loading = true;
            view.pending_file = view.file.take();
            view.verification = Some(crate::protocol::Verification {
                bytes: 42,
                total: 100,
            });
            view.peer_loading = true;
            view.peer_verification = Some(crate::protocol::Verification {
                bytes: 73,
                total: 100,
            });
            view.verified = false;
            view.peer_verified = false;
            view.ready = false;
            view.matched = false;
        }
        SessionState::Mismatch => {
            view.peer_file = Some("Filme.de.demonstração.720p.mp4".into());
            view.ready = false;
            view.matched = false;
        }
        SessionState::Stabilizing => {
            view.ready = false;
        }
        SessionState::PlayerError => {
            view.error = Some(
                "Não foi possível abrir o vídeo. Confira o arquivo e tente escolher outra cópia."
                    .into(),
            );
            view.player.blocked = true;
            view.ready = false;
        }
        _ => {}
    }
    view
}
fn card(colors: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(16)
        .inner_margin(24)
        .shadow(egui::Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if colors.dark { 40 } else { 10 }),
        })
}
/// Família semibold quando as fontes já foram carregadas; senão, a padrão.
fn semibold_family(ctx: &egui::Context) -> egui::FontFamily {
    let family = egui::FontFamily::Name("sans-semibold".into());
    if ctx.fonts(|fonts| fonts.families().contains(&family)) {
        family
    } else {
        egui::FontFamily::Proportional
    }
}
pub(crate) fn strong(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .strong()
        .family(semibold_family(ui.ctx()))
}
/// Título de tela, com espaçamento entre letras levemente reduzido.
fn title(ui: &egui::Ui, text: impl Into<String>, size: f32) -> RichText {
    strong(ui, text)
        .size(size)
        .extra_letter_spacing(-0.02 * size)
}
fn feedback(ui: &mut egui::Ui, view: &crate::runtime::View) {
    let colors = Palette::new(ui.visuals().dark_mode);
    if let Some(error) = &view.error {
        egui::Frame::new()
            .fill(kit::lerp(colors.surface, colors.error, 0.08))
            .stroke(Stroke::new(1.0, colors.error.gamma_multiply(0.35)))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
                    kit::draw(
                        ui.painter(),
                        kit::Glyph::Alert,
                        rect.center(),
                        17.0,
                        colors.error,
                        0.0,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(strong(ui, "Não foi possível continuar").color(colors.error));
                        ui.add(egui::Label::new(RichText::new(error).color(colors.text)).wrap());
                    });
                });
            });
    }
    if let Some(notice) = &view.notice {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
            kit::draw(
                ui.painter(),
                kit::Glyph::Info,
                rect.center(),
                15.0,
                colors.muted,
                0.0,
            );
            ui.add(egui::Label::new(RichText::new(notice).color(colors.muted)).wrap());
        });
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
        inputs.room_panel = false;
        inputs.player.fullscreen = false;
    }
    inputs.last_role = view.role;
    if view.role == Role::Local
        && !view.loading
        && view.file.is_none()
        && inputs.screen == Screen::Preparation
    {
        inputs.screen = Screen::Home;
    }
    if view.role != Role::Local {
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
        egui::Panel::top("app_header")
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(20, 10)),
            )
            .show(ui, |ui| {
                header(ui, colors, inputs, view.role);
                let rect = ui.clip_rect();
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, colors.border),
                );
            });
        if inputs.screen == Screen::Preparation
            && sticky_preparation
            && view.join_request.is_none()
            && !(view.role == Role::Guest && !view.connected)
        {
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
                        preparation::action(ui, colors, view, true, &mut actions);
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.background).inner_margin(
                if inputs.screen == Screen::Preparation {
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
                        let home = inputs.screen == Screen::Home;
                        let connecting = inputs.screen == Screen::Preparation
                            && view.role == Role::Guest
                            && !view.connected;
                        ui.add_space(if home || connecting {
                            ((bounds.height() - 440.0) * 0.5).clamp(0.0, 100.0)
                        } else {
                            8.0
                        });
                        if home {
                            entry::home(ui, colors, view, inputs, &mut actions);
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.set_max_width(ui.available_width().min(
                                    if inputs.screen == Screen::Preparation {
                                        680.0
                                    } else {
                                        600.0
                                    },
                                ));
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    match inputs.screen {
                                        Screen::Browse => {
                                            entry::browse(ui, colors, view, inputs, &mut actions)
                                        }
                                        Screen::Preparation if connecting => {
                                            entry::connecting(ui, colors, view, &mut actions)
                                        }
                                        Screen::Preparation => preparation::draw(
                                            ui,
                                            colors,
                                            view,
                                            sticky_preparation,
                                            &mut actions,
                                        ),
                                        Screen::Home | Screen::Watching => {}
                                    }
                                });
                            });
                        }
                    });
            });
    }

    if demo {
        actions.clear();
    }
    actions
}
fn quiet(ui: &mut egui::Ui, colors: Palette, text: &str) -> egui::Response {
    kit::Button::new(kit::Kind::Ghost, text)
        .size(kit::Size::Medium)
        .show(ui, colors)
}
fn secondary(ui: &mut egui::Ui, colors: Palette, text: &str, enabled: bool) -> egui::Response {
    kit::Button::new(kit::Kind::Secondary, text)
        .fill_width(ui)
        .enabled(enabled)
        .show(ui, colors)
}
fn header(ui: &mut egui::Ui, colors: Palette, inputs: &mut Inputs, role: crate::runtime::Role) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        visuals::brand(ui, colors);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let settings = kit::icon_button(ui, colors, kit::Glyph::Gear, "Configurações", 36.0);
            egui::Popup::menu(&settings)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| settings_menu(ui, colors, inputs, role));
        });
    });
}
fn section_label(ui: &mut egui::Ui, colors: Palette, text: &str) {
    ui.label(
        strong(ui, text.to_uppercase())
            .size(11.0)
            .extra_letter_spacing(0.8)
            .color(colors.faint),
    );
}
fn settings_menu(
    ui: &mut egui::Ui,
    colors: Palette,
    inputs: &mut Inputs,
    role: crate::runtime::Role,
) {
    ui.set_width(250.0);
    ui.spacing_mut().item_spacing.y = 8.0;
    section_label(ui, colors, "Aparência");
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(colors.inset)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(10)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.columns(2, |columns| {
                for (column, (label, light)) in
                    columns.iter_mut().zip([("Escuro", false), ("Claro", true)])
                {
                    let selected = dark != light;
                    let (rect, response) = column.allocate_exact_size(
                        egui::vec2(column.available_width(), 30.0),
                        egui::Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            selected,
                            label,
                        )
                    });
                    if selected {
                        column.painter().rect(
                            rect,
                            8,
                            colors.raised,
                            Stroke::new(1.0, colors.border_strong),
                            egui::StrokeKind::Inside,
                        );
                    } else if response.hovered() {
                        column.painter().rect_filled(rect, 8, colors.hover);
                    }
                    column.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::new(13.5, semibold_family(column.ctx())),
                        if selected { colors.text } else { colors.muted },
                    );
                    if response.clicked() && !selected {
                        configure_theme(column.ctx(), light);
                    }
                }
            });
        });
    ui.add_space(4.0);
    section_label(ui, colors, "Porta da sala");
    ui.add_enabled(
        role == crate::runtime::Role::Local,
        egui::DragValue::new(&mut inputs.port).range(1..=65535),
    );
    if role != crate::runtime::Role::Local {
        ui.label(
            RichText::new("Altere antes de criar uma nova sala.")
                .small()
                .color(colors.muted),
        );
    }
    ui.add_space(4.0);
    section_label(ui, colors, "Atalhos do player");
    egui::Grid::new("shortcuts")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            for (key, action) in [
                ("Espaço", "play / pausa"),
                ("← / →", "voltar / avançar 10 s"),
                ("F", "tela cheia"),
                ("M", "silenciar"),
            ] {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(52.0, 22.0), egui::Sense::hover());
                ui.painter().rect(
                    rect,
                    6,
                    colors.inset,
                    Stroke::new(1.0, colors.border),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    key,
                    egui::FontId::proportional(12.0),
                    colors.text,
                );
                ui.label(RichText::new(action).color(colors.muted));
                ui.end_row();
            }
        });
}
fn room_contents(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    actions: &mut Vec<crate::runtime::Command>,
) {
    use crate::runtime::{Command, Role};
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(title(ui, "Sala", 20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, colors, kit::Glyph::Close, "Fechar", 32.0).clicked() {
                    inputs.room_panel = false;
                }
            });
        });
        ui.add_space(14.0);
        let name = view
            .peer_name
            .as_deref()
            .unwrap_or(if view.role == Role::Local {
                "Assistindo sozinho"
            } else {
                "Outra pessoa"
            });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            kit::avatar(ui, colors, name, 36.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(strong(ui, name)).truncate());
                let (label, color) = if view.connected {
                    ("Conectado", colors.success)
                } else if view.role == Role::Local {
                    ("Reprodução local", colors.muted)
                } else {
                    ("Desconectado", colors.warning)
                };
                kit::status(ui, None, label, color);
            });
        });
        ui.add_space(18.0);
        section_label(ui, colors, "Seu arquivo");
        ui.add(egui::Label::new(view.file.as_deref().unwrap_or("Nenhum arquivo")).truncate())
            .on_hover_text(view.file.as_deref().unwrap_or_default());
        if view.role != Role::Local {
            ui.add_space(4.0);
            section_label(ui, colors, "Arquivo da outra pessoa");
            ui.add(
                egui::Label::new(view.peer_file.as_deref().unwrap_or("Aguardando arquivo"))
                    .truncate(),
            )
            .on_hover_text(view.peer_file.as_deref().unwrap_or_default());
        }
        ui.add_space(12.0);
        if kit::Button::new(kit::Kind::Secondary, "Trocar filme")
            .size(kit::Size::Medium)
            .fill_width(ui)
            .show(ui, colors)
            .clicked()
        {
            inputs.room_panel = false;
            inputs.player.fullscreen = false;
            actions.push(Command::Open);
        }
        ui.add_space(12.0);
        feedback(ui, view);
        connection_details(ui, view, actions);
        ui.add_space(12.0);
        if quiet(ui, colors, preparation::leave_label(view.role)).clicked() {
            inputs.player.fullscreen = false;
            inputs.room_panel = false;
            actions.push(Command::Leave);
        }
    });
}
/// O título preserva o nome do usuário, omitindo apenas a extensão de vídeo.
pub(crate) fn media_title(file: &str) -> &str {
    if let Some((name, extension)) = file.rsplit_once('.')
        && [
            "mkv", "mp4", "avi", "mov", "webm", "m4v", "mpeg", "mpg", "ts",
        ]
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
        && !name.is_empty()
    {
        name
    } else {
        file
    }
}

fn connection_details(
    ui: &mut egui::Ui,
    view: &crate::runtime::View,
    actions: &mut Vec<crate::runtime::Command>,
) {
    egui::CollapsingHeader::new("Detalhes da conexão").show(ui, |ui| {
        let addresses = room_addresses(view);
        if let Some(address) = view.room_address.filter(|addr| addr.ip().is_unspecified()) {
            ui.label(format!("Porta: {}", address.port()));
        }
        for address in addresses {
            ui.label(format!("Endereço: {address}"));
            preparation::copy_button(ui, "Copiar endereço", &address.to_string());
        }
        if let Some(error) = &view.discovery_error {
            ui.label(error);
        }
        ui.label(&view.status);
        if let Some(drift) = view.peer_drift {
            ui.label(format!(
                "Diferença estimada entre players: {:.0} ms",
                drift.abs() * 1000.0
            ));
        }
        if view.connected && view.role == crate::runtime::Role::Guest {
            ui.label(format!("Ida e volta: {:.0} ms", view.rtt_ms));
        }
        if view.role == crate::runtime::Role::Local && ui.button("Atualizar salas").clicked() {
            actions.push(crate::runtime::Command::Discover);
        }
    });
}

fn room_addresses(view: &crate::runtime::View) -> Vec<std::net::SocketAddr> {
    if view.room_addresses.is_empty() {
        view.room_address
            .into_iter()
            .filter(|addr| !addr.ip().is_unspecified())
            .collect()
    } else {
        view.room_addresses.clone()
    }
}
