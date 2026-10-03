//! Telas sem acesso a arquivos, ao player ou à rede.

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::model::{Action, SessionState};
pub use crate::player_ui::{PlayerUi, Video};
mod preparation;
mod visuals;

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    surface: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    success: Color32,
    warning: Color32,
    error: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: Color32::from_rgb(9, 11, 18),
                surface: Color32::from_rgb(19, 23, 34),
                border: Color32::from_rgb(39, 45, 61),
                text: Color32::from_rgb(245, 246, 250),
                muted: Color32::from_rgb(151, 159, 178),
                accent: Color32::from_rgb(104, 145, 255),
                success: Color32::from_rgb(110, 214, 178),
                warning: Color32::from_rgb(241, 191, 110),
                error: Color32::from_rgb(255, 149, 149),
            }
        } else {
            Self {
                background: Color32::from_rgb(245, 246, 250),
                surface: Color32::WHITE,
                border: Color32::from_rgb(221, 224, 234),
                text: Color32::from_rgb(23, 29, 47),
                muted: Color32::from_rgb(101, 110, 132),
                accent: Color32::from_rgb(49, 91, 215),
                success: Color32::from_rgb(21, 112, 81),
                warning: Color32::from_rgb(139, 84, 11),
                error: Color32::from_rgb(174, 44, 55),
            }
        }
    }
}

/// Aplica o tema também usado pelos testes de layout.
pub fn configure_theme(ctx: &egui::Context, light: bool) {
    let colors = Palette::new(!light);
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::new(1.0, colors.border);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.extreme_bg_color = colors.background;
    visuals.faint_bg_color = colors.surface;
    visuals.override_text_color = Some(colors.text);
    visuals.selection.bg_fill = colors.accent.gamma_multiply(0.18);
    visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.active,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
        widget.bg_fill = colors.surface;
        widget.weak_bg_fill = colors.surface;
        widget.bg_stroke = Stroke::new(1.0, colors.border);
        widget.fg_stroke = Stroke::new(1.5, colors.text);
    }
    visuals.widgets.noninteractive.fg_stroke.color = colors.muted;
    visuals.widgets.hovered.bg_fill = colors.border;
    visuals.widgets.hovered.weak_bg_fill = colors.border;
    visuals.widgets.hovered.bg_stroke.color = colors.muted;
    visuals.widgets.active.bg_stroke.color = colors.accent;
    visuals.widgets.open.bg_stroke.color = colors.accent;
    ctx.set_visuals(visuals);
    ctx.style_mut_of(
        if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        },
        |style| {
            style.spacing.item_spacing = egui::vec2(12.0, 10.0);
            style.spacing.button_padding = egui::vec2(18.0, 11.0);
            style.spacing.interact_size.y = 36.0;
            style.spacing.menu_margin = egui::Margin::same(12);
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            style.spacing.scroll.bar_width = 6.0;
            style.animation_time = 0.16;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
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
        egui::Panel::top("demo_controls")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("DEMO OFFLINE")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.menu_button(state.label(), |ui| {
                        ui.label(RichText::new("Cenários fictícios, sem player ou rede").small());
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
                    });
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
    if state == SessionState::Browsing {
        view.rooms.push(crate::discovery::Room {
            name: "notebook-ana".into(),
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
        .corner_radius(12)
        .inner_margin(24)
}
fn primary(ui: &mut egui::Ui, colors: Palette, text: &str, enabled: bool) -> egui::Response {
    let width = ui.available_width();
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(
            [width, 48.0],
            egui::Button::new(RichText::new(text).color(colors.background).strong())
                .fill(colors.text)
                .corner_radius(8),
        )
    })
    .inner
}
fn feedback(ui: &mut egui::Ui, view: &crate::runtime::View) {
    let colors = Palette::new(ui.visuals().dark_mode);
    if let Some(error) = &view.error {
        card(colors)
            .inner_margin(12)
            .stroke(Stroke::new(1.0, colors.error))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new("Não foi possível continuar")
                        .strong()
                        .color(colors.error),
                );
                ui.add(egui::Label::new(error).wrap());
            });
    }
    if let Some(notice) = &view.notice {
        ui.label(notice);
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
    let sticky_preparation = ui.available_height() < 760.0;
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
                        room_contents(ui, colors, view, inputs, &mut actions)
                    });
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                if demo {
                    ui.painter()
                        .rect_filled(rect, 0, Color32::from_rgb(10, 17, 33));
                    visuals::glow(
                        ui.painter(),
                        rect.center(),
                        rect.size() * 0.6,
                        Color32::from_rgba_unmultiplied(45, 78, 155, 65),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Vídeo fictício",
                        egui::FontId::proportional(20.0),
                        Color32::from_gray(156),
                    );
                }
                actions.extend(crate::player_ui::draw(ui, rect, view, inputs, video, demo));
            });
    } else {
        inputs.player.video_rect = None;
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
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(ui.available_width().min(760.0));
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
                if ui.visuals().dark_mode {
                    visuals::glow(
                        ui.painter(),
                        bounds.right_top() + egui::vec2(-80.0, 80.0),
                        egui::vec2(600.0, 400.0),
                        Color32::from_rgba_unmultiplied(35, 53, 110, 28),
                    );
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        header(ui, colors, inputs, view.role);
                        let home = inputs.screen == Screen::Home;
                        ui.add_space(if home {
                            if bounds.height() < 500.0 {
                                (bounds.height() * 0.05).clamp(12.0, 24.0)
                            } else {
                                (bounds.height() * 0.15).clamp(32.0, 100.0)
                            }
                        } else {
                            16.0
                        });
                        if home {
                            home_screen(ui, colors, view, inputs, &mut actions);
                        } else {
                            ui.vertical_centered(|ui| {
                                ui.set_max_width(ui.available_width().min(
                                    if inputs.screen == Screen::Preparation {
                                        760.0
                                    } else {
                                        600.0
                                    },
                                ));
                                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                    match inputs.screen {
                                        Screen::Browse => {
                                            browse(ui, colors, view, inputs, &mut actions)
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
    ui.add(egui::Button::new(RichText::new(text).color(colors.muted)).frame(false))
}
fn secondary(ui: &mut egui::Ui, colors: Palette, text: &str, enabled: bool) -> egui::Response {
    let width = ui.available_width();
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(
            [width, 48.0],
            egui::Button::new(text)
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border)),
        )
    })
    .inner
}
fn header(ui: &mut egui::Ui, colors: Palette, inputs: &mut Inputs, role: crate::runtime::Role) {
    ui.horizontal(|ui| {
        visuals::brand(ui, colors.text, colors.accent);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(
                RichText::new("Configurações").small().color(colors.muted),
                |ui| {
                    ui.label(RichText::new("Aparência").strong());
                    ui.horizontal(|ui| {
                        let dark = ui.visuals().dark_mode;
                        if ui.selectable_label(dark, "Escuro").clicked() {
                            configure_theme(ui.ctx(), false);
                        }
                        if ui.selectable_label(!dark, "Claro").clicked() {
                            configure_theme(ui.ctx(), true);
                        }
                    });
                    ui.separator();
                    ui.label("Porta da sala");
                    ui.add_enabled(
                        role == crate::runtime::Role::Local,
                        egui::DragValue::new(&mut inputs.port).range(1..=65535),
                    );
                    if role != crate::runtime::Role::Local {
                        ui.label(RichText::new("Altere antes de criar uma nova sala.").small());
                    }
                    ui.separator();
                    ui.label(RichText::new("Atalhos do player").strong());
                    ui.label("Espaço · play / pausa");
                    ui.label("← / → · voltar / avançar 10 s");
                    ui.label("F · tela cheia    M · silenciar");
                },
            );
        });
    });
}
fn home_screen(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    actions: &mut Vec<crate::runtime::Command>,
) {
    let wide = ui.available_width() >= 820.0;
    let width = if wide {
        460.0
    } else {
        ui.available_width().min(560.0)
    };
    if wide {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 330.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| home_content(ui, colors, view, inputs, actions),
            );
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 300.0),
                egui::Sense::hover(),
            );
            visuals::cinema(ui.painter(), rect, ui.visuals().dark_mode);
        });
    } else {
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                home_content(ui, colors, view, inputs, actions)
            });
        });
    }
}
fn home_content(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    actions: &mut Vec<crate::runtime::Command>,
) {
    use crate::runtime::Command;
    let compact = ui.ctx().content_rect().height() < 600.0 || ui.available_width() < 440.0;
    if !compact {
        ui.label(
            RichText::new("CINEMA A DOIS")
                .size(11.0)
                .extra_letter_spacing(2.0)
                .color(colors.accent),
        );
        ui.add_space(12.0);
    }
    ui.label(
        RichText::new("O mesmo filme.\nO mesmo momento.")
            .size(if compact { 36.0 } else { 48.0 })
            .strong(),
    );
    ui.add_space(12.0);
    ui.label(
        RichText::new("Uma cópia em cada PC. Um play para vocês dois.")
            .size(16.0)
            .color(colors.muted),
    );
    ui.add_space(if compact { 24.0 } else { 30.0 });
    feedback(ui, view);
    let create_label = if view.hosting {
        "Criando sala…"
    } else {
        "Criar sala"
    };
    if ui.available_width() >= 430.0 {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(174.0, 48.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if primary(ui, colors, create_label, !view.hosting).clicked() {
                        actions.push(Command::HostAuto(inputs.port));
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(226.0, 48.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if secondary(ui, colors, "Entrar em uma sala", !view.hosting).clicked() {
                        inputs.screen = Screen::Browse;
                        actions.push(Command::Discover);
                    }
                },
            );
        });
    } else {
        if primary(ui, colors, create_label, !view.hosting).clicked() {
            actions.push(Command::HostAuto(inputs.port));
        }
        if secondary(ui, colors, "Entrar em uma sala", !view.hosting).clicked() {
            inputs.screen = Screen::Browse;
            actions.push(Command::Discover);
        }
    }
    ui.add_space(12.0);
    if ui
        .add_enabled(
            !view.hosting,
            egui::Button::new(RichText::new("Abrir filme sozinho").color(colors.muted))
                .frame(false),
        )
        .clicked()
    {
        actions.push(Command::Open);
    }
    if view.player.loaded && view.file.is_some() {
        if quiet(ui, colors, "Continuar filme aberto").clicked() {
            actions.push(Command::Control(crate::protocol::Control::Play));
        }
        ui.add(
            egui::Label::new(
                RichText::new(view.file.as_deref().unwrap_or_default())
                    .small()
                    .color(colors.muted),
            )
            .truncate(),
        );
    }
    if view.hosting && quiet(ui, colors, "Cancelar").clicked() {
        actions.push(Command::Leave);
    }
}
fn browse(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &crate::runtime::View,
    inputs: &mut Inputs,
    actions: &mut Vec<crate::runtime::Command>,
) {
    use crate::runtime::Command;
    if ui
        .add(
            egui::Button::new(RichText::new("Voltar").color(colors.muted))
                .frame(false)
                .small(),
        )
        .clicked()
    {
        inputs.screen = Screen::Home;
        actions.push(Command::CancelDiscovery);
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Encontre sua sala.").size(32.0).strong());
    ui.label(RichText::new("Salas disponíveis na sua rede ou VPN.").color(colors.muted));
    ui.add_space(24.0);
    if view.rooms.is_empty() {
        card(colors).inner_margin(20).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_space(8.0);
            if view.searching {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Procurando salas…");
                });
            } else if let Some(error) = &view.discovery_error {
                ui.add(egui::Label::new(error).wrap());
            } else {
                ui.label(RichText::new("Ainda não há salas por aqui.").strong());
                ui.label(
                    RichText::new("Peça à outra pessoa para criar uma sala. Se ela já criou, conecte pelo endereço abaixo.").color(colors.muted),
                );
            }
            ui.add_space(8.0);
        });
    }
    for room in &view.rooms {
        card(colors).inner_margin(20).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(format!("Sala de {}", room.name))
                    .size(19.0)
                    .strong(),
            );
            ui.label(
                RichText::new("Uma vaga para você")
                    .small()
                    .color(colors.muted),
            );
            ui.add_space(12.0);
            if primary(ui, colors, "Solicitar entrada", true).clicked() {
                actions.push(Command::RequestJoin(room.address.to_string()));
            }
        });
        ui.add_space(8.0);
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !view.searching,
                egui::Button::new(RichText::new("Atualizar salas").color(colors.muted))
                    .frame(false)
                    .small(),
            )
            .clicked()
        {
            actions.push(Command::Discover);
        }
        if view.searching && !view.rooms.is_empty() {
            ui.spinner();
        }
    });
    ui.add_space(8.0);
    egui::CollapsingHeader::new("Conectar por endereço").show(ui, |ui| {
        ui.label(
            RichText::new("Use o endereço do anfitrião se a sala não aparecer.")
                .small()
                .color(colors.muted),
        );
        ui.label("Endereço do anfitrião");
        let address_input = ui.add(
            egui::TextEdit::singleline(&mut inputs.address)
                .hint_text("192.168.1.10:7842")
                .desired_width(ui.available_width()),
        );
        let valid = crate::runtime::parse_address(&inputs.address).is_ok();
        if !inputs.address.trim().is_empty() && !valid {
            ui.add(
                egui::Label::new(
                    RichText::new("Informe um IP, como 192.168.1.10 ou 192.168.1.10:7842.")
                        .small()
                        .color(colors.warning),
                )
                .wrap(),
            );
        }
        if secondary(ui, colors, "Solicitar entrada", valid).clicked()
            || valid && address_input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            actions.push(Command::RequestJoin(inputs.address.clone()));
        }
    });
    feedback(ui, view);
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
            ui.heading("Sala");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if quiet(ui, colors, "Fechar").clicked() {
                    inputs.room_panel = false;
                }
            });
        });
        ui.add_space(16.0);
        ui.label(
            view.peer_name
                .as_deref()
                .unwrap_or(if view.role == Role::Local {
                    "Assistindo sozinho"
                } else {
                    "Outra pessoa"
                }),
        );
        ui.label(
            RichText::new(if view.connected {
                "Conectado"
            } else if view.role == Role::Local {
                "Reprodução local"
            } else {
                "Desconectado"
            })
            .small()
            .color(colors.muted),
        );
        ui.add_space(24.0);
        ui.label(RichText::new("Seu arquivo").small().color(colors.muted));
        ui.add(egui::Label::new(view.file.as_deref().unwrap_or("Nenhum arquivo")).truncate())
            .on_hover_text(view.file.as_deref().unwrap_or_default());
        if view.role != Role::Local {
            ui.label(
                RichText::new("Arquivo da outra pessoa")
                    .small()
                    .color(colors.muted),
            );
            ui.add(
                egui::Label::new(view.peer_file.as_deref().unwrap_or("Aguardando arquivo"))
                    .truncate(),
            )
            .on_hover_text(view.peer_file.as_deref().unwrap_or_default());
        }
        ui.add_space(12.0);
        if secondary(ui, colors, "Trocar filme", true).clicked() {
            inputs.room_panel = false;
            inputs.player.fullscreen = false;
            actions.push(Command::Open);
        }
        ui.add_space(16.0);
        feedback(ui, view);
        connection_details(ui, view, actions);
        ui.add_space(20.0);
        if view.role == Role::Host {
            ui.add(
                egui::Label::new(
                    RichText::new("Encerrar a sala desconecta a outra pessoa.")
                        .small()
                        .color(colors.muted),
                )
                .wrap(),
            );
        }
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
