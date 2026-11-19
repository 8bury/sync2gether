use super::{
    Inputs, Palette, Screen, card, feedback, kit, quiet, secondary, strong, title, visuals,
};

use crate::runtime::{Command, View};
use eframe::egui::{self, RichText, Stroke};

pub(super) fn home(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    let joining = inputs.joining && view.role == crate::runtime::Role::Guest && !view.connected;
    let compact = ui.available_height() < 400.0;
    ui.vertical_centered(|ui| {
        if compact {
            ui.spacing_mut().item_spacing.y = 6.0;
        }
        ui.set_max_width(ui.available_width().min(380.0));
        if !compact {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
            visuals::app_icon(ui.painter(), rect, colors);
            ui.add_space(14.0);
        }
        ui.label(title(ui, "Assistir junto", if compact { 24.0 } else { 28.0 }));
        if !compact {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Cada pessoa abre a sua cópia do filme. Play, pause e posição ficam iguais nos dois PCs.",
                    )
                    .color(colors.muted),
                )
                .halign(egui::Align::Center)
                .wrap(),
            );
        }
        ui.add_space(if compact { 6.0 } else { 18.0 });
        if !inputs.code_entry {
            feedback(ui, view);
        }
        let busy = view.hosting || joining;
        let create = kit::Button::new(
            kit::Kind::Primary,
            if view.hosting {
                "Criando sala…"
            } else {
                "Criar sala"
            },
        )
        .glyph(kit::Glyph::Plus)
        .busy(view.hosting)
        .enabled(!busy)
        .fill_width(ui)
        .show(ui, colors);
        if create.clicked() {
            inputs.code_entry = false;
            actions.push(Command::HostInternet(inputs.relay.clone()));
        }
        ui.add_space(if compact { 2.0 } else { 6.0 });
        divider(ui, colors, "ou entre com um código");
        ui.add_space(if compact { 0.0 } else { 2.0 });
        code_entry(ui, colors, view, inputs, joining, actions);
        ui.add_space(if compact { 0.0 } else { 4.0 });
        if inputs.lan {
            let ghost = |label| {
                kit::Button::new(kit::Kind::Ghost, label)
                    .size(kit::Size::Small)
                    .enabled(!busy)
            };
            let host = "Criar sala na rede local";
            let browse = "Procurar na rede local";
            kit::centered_row(
                ui,
                &[ghost(host).natural_width(ui), ghost(browse).natural_width(ui)],
                |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if ghost(host).show(ui, colors).clicked() {
                        actions.push(Command::HostAuto(inputs.port));
                    }
                    if ghost(browse).show(ui, colors).clicked() {
                        inputs.screen = Screen::Browse;
                        actions.push(Command::Discover);
                    }
                },
            );
        }
        if joining {
            if quiet(ui, colors, "Cancelar").clicked() {
                inputs.joining = false;
                actions.push(Command::Leave);
            }
        } else if view.hosting {
            if quiet(ui, colors, "Cancelar").clicked() {
                actions.push(Command::Leave);
            }
        } else if kit::Button::new(kit::Kind::Link, "Abrir filme sozinho")
            .size(kit::Size::Medium)
            .show(ui, colors)
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
    });
}

fn divider(ui: &mut egui::Ui, colors: Palette, text: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(12.5),
        colors.faint,
    );
    let half = galley.size().x * 0.5 + 12.0;
    let y = rect.center().y;
    let stroke = Stroke::new(1.0, colors.border);
    ui.painter()
        .hline(rect.left()..=rect.center().x - half, y, stroke);
    ui.painter()
        .hline(rect.center().x + half..=rect.right(), y, stroke);
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, colors.faint);
}

fn code_entry(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    joining: bool,
    actions: &mut Vec<Command>,
) {
    let error = inputs.code_entry
        && !inputs.code_edited
        && !joining
        && view.role == crate::runtime::Role::Local;
    let error_text = view.error.as_deref().filter(|_| error);
    let parts = inputs
        .code
        .split(|c: char| c == '-' || c == '.' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .count();
    let hint = parts >= 3 && crate::network::relay::Code::parse(&inputs.code).is_none();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let valid = crate::network::relay::Code::parse(&inputs.code).is_some();
        let label = if joining { "Entrando" } else { "Entrar" };
        let button = kit::Button::new(
            if valid && !hint {
                kit::Kind::Primary
            } else {
                kit::Kind::Secondary
            },
            label,
        )
        .glyph(kit::Glyph::ArrowRight)
        .busy(joining)
        .size(kit::Size::Medium);
        let field_width = (ui.available_width() - button.natural_width(ui) - 8.0).max(80.0);
        let stroke = if error_text.is_some() || hint {
            Some(if hint { colors.warning } else { colors.error })
        } else {
            None
        };
        let field = ui.scope(|ui| {
            if let Some(color) = stroke {
                let visuals = ui.visuals_mut();
                for widget in [
                    &mut visuals.widgets.inactive,
                    &mut visuals.widgets.hovered,
                    &mut visuals.widgets.active,
                ] {
                    widget.bg_stroke = Stroke::new(1.0, color.gamma_multiply(0.7));
                }
                visuals.selection.stroke = Stroke::new(1.0, color);
            }
            ui.add_enabled_ui(!joining, |ui| {
                ui.add_sized(
                    [field_width, 40.0],
                    egui::TextEdit::singleline(&mut inputs.code)
                        .id(egui::Id::new("room_code"))
                        .hint_text(RichText::new("Ex.: 4821-gato-lua").color(colors.faint))
                        .margin(egui::vec2(12.0, 11.0)),
                )
                .on_hover_text("Número e duas palavras, como 4821-gato-lua")
            })
            .inner
        });
        let field = field.inner;
        let valid = crate::network::relay::Code::parse(&inputs.code).is_some();
        if field.changed() {
            inputs.code_edited = true;
        }
        if button.enabled(valid && !joining).show(ui, colors).clicked()
            || valid
                && !joining
                && field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            inputs.code_entry = true;
            inputs.code_edited = false;
            inputs.joining = true;
            actions.push(Command::JoinCode {
                relay: inputs.relay.clone(),
                code: inputs.code.clone(),
            });
        }
    });
    let (text, color) = if let Some(error) = error_text {
        (error, colors.error)
    } else if hint {
        (crate::runtime::CODE_HINT, colors.warning)
    } else {
        ("", colors.muted)
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 18.0),
        egui::Layout::left_to_right(egui::Align::Min),
        |ui| {
            ui.set_min_height(18.0);
            if !text.is_empty() {
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
            }
        },
    );
}

pub(super) fn browse(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    if kit::Button::new(kit::Kind::Ghost, "Voltar")
        .glyph(kit::Glyph::ArrowLeft)
        .size(kit::Size::Small)
        .show(ui, colors)
        .clicked()
    {
        inputs.screen = Screen::Home;
        actions.push(Command::CancelDiscovery);
    }
    ui.add_space(2.0);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(title(ui, "Salas disponíveis", 26.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::Button::new(kit::Kind::Ghost, "Atualizar salas")
                .maybe_glyph(Some(if view.searching {
                    kit::Glyph::Spinner
                } else {
                    kit::Glyph::Refresh
                }))
                .busy(view.searching)
                .size(kit::Size::Small)
                .show(ui, colors)
                .clicked()
            {
                actions.push(Command::Discover);
            }
        });
    });
    ui.add_space(10.0);
    card(colors).inner_margin(8).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        if view.rooms.is_empty() {
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                if view.searching {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    kit::draw(
                        ui.painter(),
                        kit::Glyph::Spinner,
                        rect.center(),
                        22.0,
                        colors.accent,
                        ui.input(|i| i.time),
                    );
                    ui.ctx().request_repaint();
                    ui.label(RichText::new("Procurando salas…").color(colors.muted));
                } else if let Some(error) = &view.discovery_error {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    kit::draw(
                        ui.painter(),
                        kit::Glyph::Alert,
                        rect.center(),
                        20.0,
                        colors.warning,
                        0.0,
                    );
                    ui.add(egui::Label::new(RichText::new(error).color(colors.muted)).wrap());
                } else {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    kit::draw(
                        ui.painter(),
                        kit::Glyph::People,
                        rect.center(),
                        22.0,
                        colors.faint,
                        0.0,
                    );
                    ui.label(strong(ui, "Nenhuma sala encontrada"));
                }
            });
            ui.add_space(18.0);
        } else {
            for (index, room) in view.rooms.iter().enumerate() {
                ui.push_id(room.address, |ui| {
                    if index > 0 {
                        let y = ui.cursor().top();
                        ui.painter().hline(
                            ui.max_rect().x_range().shrink(10.0),
                            y,
                            Stroke::new(1.0, colors.border),
                        );
                        ui.add_space(1.0);
                    }
                    room_row(ui, colors, room, actions);
                });
            }
            if let Some(error) = &view.discovery_error {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(error).small().color(colors.warning))
                                .wrap(),
                        );
                    });
            }
        }
    });
    ui.add_space(18.0);
    card(colors)
        .fill(colors.surface)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(strong(ui, "Conectar por endereço").size(15.5));
            let address_label = ui.label(
                RichText::new("Endereço do anfitrião")
                    .small()
                    .color(colors.muted),
            );
            ui.add_space(2.0);
            let wide = ui.available_width() >= 460.0;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), if wide { 48.0 } else { 106.0 }),
                if wide {
                    egui::Layout::left_to_right(egui::Align::Center)
                } else {
                    egui::Layout::top_down(egui::Align::Min)
                },
                |ui| {
                    let valid = crate::runtime::parse_address(&inputs.address).is_ok();
                    let button = kit::Button::new(
                        if valid {
                            kit::Kind::Primary
                        } else {
                            kit::Kind::Secondary
                        },
                        "Solicitar entrada",
                    );
                    let field_width = if wide {
                        ui.available_width()
                            - button.natural_width(ui)
                            - ui.spacing().item_spacing.x
                    } else {
                        ui.available_width()
                    };
                    let field = ui
                        .add_sized(
                            [field_width, 48.0],
                            egui::TextEdit::singleline(&mut inputs.address)
                                .id(egui::Id::new("host_address"))
                                .hint_text(RichText::new("192.168.1.10:7842").color(colors.faint))
                                .margin(egui::vec2(14.0, 14.0)),
                        )
                        .labelled_by(address_label.id)
                        .on_hover_text("Endereço do anfitrião: IP ou IP:porta");
                    let valid = crate::runtime::parse_address(&inputs.address).is_ok();
                    let button = if wide { button } else { button.fill_width(ui) };
                    if button.enabled(valid).show(ui, colors).clicked()
                        || valid
                            && field.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        actions.push(Command::RequestJoin(inputs.address.clone()));
                    }
                },
            );
            if !inputs.address.trim().is_empty()
                && crate::runtime::parse_address(&inputs.address).is_err()
            {
                ui.add(
                    egui::Label::new(
                        RichText::new("Informe um IP, como 192.168.1.10 ou 192.168.1.10:7842.")
                            .small()
                            .color(colors.warning),
                    )
                    .wrap(),
                );
            }
        });
    ui.add_space(8.0);
    feedback(ui, view);
}

fn room_row(
    ui: &mut egui::Ui,
    colors: Palette,
    room: &crate::discovery::Room,
    actions: &mut Vec<Command>,
) {
    let wide = ui.available_width() >= 420.0;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let button = || {
                kit::Button::new(kit::Kind::Primary, "Solicitar entrada").size(kit::Size::Medium)
            };
            let mut clicked = false;
            ui.horizontal(|ui| {
                kit::avatar(ui, colors, &room.name, 40.0);
                let button_width = if wide {
                    button().natural_width(ui)
                } else {
                    0.0
                };
                let name_width =
                    (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(1.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(name_width, 40.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(name_width);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(
                            egui::Label::new(
                                strong(ui, format!("Sala de {}", room.name)).size(15.5),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&room.name);
                        ui.label(
                            RichText::new(room.address.to_string())
                                .small()
                                .color(colors.muted),
                        );
                    },
                );
                if wide {
                    clicked = button().show(ui, colors).clicked();
                }
            });
            if !wide {
                clicked = button().fill_width(ui).show(ui, colors).clicked();
            }
            if clicked {
                actions.push(Command::RequestJoin(room.address.to_string()));
            }
        });
}

pub(super) fn connecting(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    actions: &mut Vec<Command>,
) {
    let compact = ui.available_height() < 500.0;
    let failed = view.error.is_some();
    ui.vertical_centered(|ui| {
        if compact {
            ui.spacing_mut().item_spacing.y = 4.0;
        }
        ui.set_max_width(ui.available_width().min(460.0));
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(200.0, if compact { 52.0 } else { 72.0 }),
            egui::Sense::hover(),
        );
        visuals::connection(ui.painter(), rect, colors, ui.input(|i| i.time));
        if !failed {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
        ui.add_space(if compact { 8.0 } else { 22.0 });
        ui.label(title(
            ui,
            if failed {
                "Não foi possível conectar"
            } else if view.waiting_approval {
                "Aguardando aprovação"
            } else {
                "Conectando à sala…"
            },
            if compact { 24.0 } else { 28.0 },
        ));
        ui.add_space(if compact { 8.0 } else { 20.0 });
        card(colors)
            .inner_margin(if compact { 14 } else { 18 })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let name = view
                    .peer_name
                    .as_ref()
                    .map(|name| format!("Sala de {name}"))
                    .unwrap_or_else(|| "Sala do anfitrião".into());
                ui.horizontal(|ui| {
                    kit::avatar(
                        ui,
                        colors,
                        view.peer_name.as_deref().unwrap_or("Anfitrião"),
                        40.0,
                    );
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(egui::Label::new(strong(ui, name).size(15.5)).truncate());
                        ui.label(
                            RichText::new(if view.waiting_approval {
                                "Solicitação enviada"
                            } else {
                                "Conexão pendente"
                            })
                            .small()
                            .color(colors.muted),
                        );
                    });
                });
                if failed {
                    ui.add_space(10.0);
                    feedback(ui, view);
                } else {
                    ui.add_space(if compact { 6.0 } else { 12.0 });
                    let y = ui.cursor().top();
                    ui.painter()
                        .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, colors.border));
                    ui.add_space(if compact { 6.0 } else { 12.0 });
                    if view.waiting_approval {
                        step(ui, colors, true, "Anfitrião encontrado");
                        step(ui, colors, false, "Aguardando aprovação");
                    } else if view.internet {
                        step(ui, colors, false, "Conectando pela internet…");
                    } else {
                        step(ui, colors, false, "Conectando ao anfitrião…");
                    }
                }
            });
        ui.add_space(if compact { 8.0 } else { 18.0 });
        ui.vertical_centered(|ui| {
            ui.set_max_width(ui.available_width().min(260.0));
            if secondary(ui, colors, "Cancelar solicitação", true).clicked() {
                actions.push(Command::Leave);
            }
        });
    });
}

fn step(ui: &mut egui::Ui, colors: Palette, done: bool, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
        if done {
            ui.painter()
                .circle_filled(rect.center(), 10.0, colors.success.gamma_multiply(0.16));
            kit::draw(
                ui.painter(),
                kit::Glyph::Check,
                rect.center(),
                13.0,
                colors.success,
                0.0,
            );
        } else {
            kit::draw(
                ui.painter(),
                kit::Glyph::Spinner,
                rect.center(),
                17.0,
                colors.accent,
                ui.input(|i| i.time),
            );
            ui.ctx().request_repaint();
        }
        ui.label(RichText::new(text).color(if done { colors.text } else { colors.muted }));
    });
}
