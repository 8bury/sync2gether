use super::{
    Inputs, Palette, feedback, kit, strong, title,
    visuals::{self, Presence},
};
use crate::runtime::{Command, View};
use eframe::egui::{self, RichText, Stroke};

pub(super) fn browse(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        ui.label(title("Salas disponíveis", 30.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::Button::new(kit::Kind::Ghost, "Atualizar salas")
                .glyph(kit::Glyph::Refresh)
                .busy(view.searching)
                .size(kit::Size::Small)
                .show(ui, colors)
                .clicked()
            {
                actions.push(Command::Discover);
            }
        });
    });
    ui.add_space(14.0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if view.rooms.is_empty() {
            empty_rooms(ui, colors, view);
        } else {
            for room in &view.rooms {
                ui.push_id(room.address, |ui| room_row(ui, colors, room, actions));
            }
        }
        kit::rule(ui, colors);
    });
    if !view.rooms.is_empty()
        && let Some(error) = &view.discovery_error
    {
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(error).small().color(colors.warning)).wrap());
    }
    ui.add_space(26.0);
    manual_address(ui, colors, inputs, actions);
    ui.add_space(8.0);
    feedback(ui, view);
}

fn empty_rooms(ui: &mut egui::Ui, colors: Palette, view: &View) {
    kit::rule(ui, colors);
    ui.add_space(22.0);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        if view.searching {
            visuals::pair(ui, colors, Presence::Waiting, 22.0);
            ui.label(RichText::new("Procurando salas…").color(colors.muted));
        } else if let Some(error) = &view.discovery_error {
            visuals::pair(ui, colors, Presence::Apart, 22.0);
            ui.add(egui::Label::new(RichText::new(error).color(colors.muted)).wrap());
        } else {
            visuals::pair(ui, colors, Presence::Solo, 22.0);
            ui.label(RichText::new("Nenhuma sala encontrada").color(colors.muted));
        }
    });
    ui.add_space(22.0);
}

fn manual_address(
    ui: &mut egui::Ui,
    colors: Palette,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.label(strong(ui, "Conectar por endereço").size(15.0));
    let address_label = ui.label(
        RichText::new("Endereço do anfitrião")
            .small()
            .color(colors.muted),
    );
    ui.add_space(2.0);
    let wide = ui.available_width() >= 460.0;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), if wide { 40.0 } else { 92.0 }),
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
            )
            .size(kit::Size::Medium);
            let field_width = if wide {
                ui.available_width() - button.natural_width(ui) - ui.spacing().item_spacing.x
            } else {
                ui.available_width()
            };
            let field = ui
                .add_sized(
                    [field_width, 40.0],
                    egui::TextEdit::singleline(&mut inputs.address)
                        .id(egui::Id::new("host_address"))
                        .font(egui::FontId::monospace(14.0))
                        .hint_text(
                            RichText::new("192.168.1.10:7842")
                                .font(egui::FontId::monospace(14.0))
                                .color(colors.faint),
                        )
                        .margin(egui::vec2(12.0, 11.0)),
                )
                .labelled_by(address_label.id)
                .on_hover_text("Endereço do anfitrião: IP ou IP:porta");
            let valid = crate::runtime::parse_address(&inputs.address).is_ok();
            let button = if wide { button } else { button.fill_width(ui) };
            if button.enabled(valid).show(ui, colors).clicked()
                || valid && field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                actions.push(Command::RequestJoin(inputs.address.clone()));
            }
        },
    );
    if !inputs.address.trim().is_empty() && crate::runtime::parse_address(&inputs.address).is_err()
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
}

fn room_row(
    ui: &mut egui::Ui,
    colors: Palette,
    room: &crate::discovery::Room,
    actions: &mut Vec<Command>,
) {
    let wide = ui.available_width() >= 420.0;
    let top = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        top + 0.5,
        Stroke::new(1.0, colors.border),
    );
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let button = || {
                kit::Button::new(kit::Kind::Primary, "Solicitar entrada").size(kit::Size::Medium)
            };
            let mut clicked = false;
            ui.horizontal(|ui| {
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
                            egui::Label::new(strong(ui, format!("Sala de {}", room.name)))
                                .truncate(),
                        )
                        .on_hover_text(&room.name);
                        ui.label(
                            RichText::new(room.address.to_string())
                                .font(egui::FontId::monospace(12.5))
                                .color(colors.faint),
                        );
                    },
                );
                if wide {
                    clicked = button().show(ui, colors).clicked();
                }
            });
            if !wide {
                ui.add_space(6.0);
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
    let compact = ui.ctx().content_rect().height() < 560.0;
    let failed = view.error.is_some();
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        visuals::pair(
            ui,
            colors,
            if failed {
                Presence::Apart
            } else {
                Presence::Waiting
            },
            if compact { 30.0 } else { 44.0 },
        );
        ui.add_space(if compact { 12.0 } else { 26.0 });
        ui.label(title(
            if failed {
                "Não foi possível conectar"
            } else if view.waiting_approval {
                "Aguardando aprovação"
            } else {
                "Conectando à sala…"
            },
            if compact { 24.0 } else { 30.0 },
        ));
        ui.add_space(if compact { 10.0 } else { 22.0 });
        kit::rule(ui, colors);
        ui.add_space(if compact { 8.0 } else { 14.0 });
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let name = view
                .peer_name
                .as_ref()
                .map(|name| format!("Sala de {name}"))
                .unwrap_or_else(|| "Sala do anfitrião".into());
            ui.add(egui::Label::new(strong(ui, name).size(15.0)).truncate());
            ui.label(
                RichText::new(if view.waiting_approval {
                    "Solicitação enviada"
                } else {
                    "Conexão pendente"
                })
                .size(13.0)
                .color(colors.faint),
            );
            ui.add_space(if compact { 4.0 } else { 8.0 });
            if failed {
                feedback(ui, view);
            } else if view.waiting_approval {
                step(ui, colors, true, "Anfitrião encontrado");
                step(ui, colors, false, "Aguardando aprovação");
            } else if view.internet {
                step(ui, colors, false, "Conectando pela internet…");
            } else {
                step(ui, colors, false, "Conectando ao anfitrião…");
            }
        });
        ui.add_space(if compact { 8.0 } else { 14.0 });
        kit::rule(ui, colors);
        ui.add_space(if compact { 10.0 } else { 22.0 });
        if kit::Button::new(kit::Kind::Secondary, "Cancelar solicitação")
            .size(kit::Size::Medium)
            .show(ui, colors)
            .clicked()
        {
            actions.push(Command::Leave);
        }
    });
}

fn step(ui: &mut egui::Ui, colors: Palette, done: bool, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
        kit::draw(
            ui.painter(),
            if done {
                kit::Glyph::Check
            } else {
                kit::Glyph::Spinner
            },
            rect.center(),
            14.0,
            if done { colors.text } else { colors.muted },
            ui.input(|i| i.time),
        );
        if !done {
            ui.ctx().request_repaint();
        }
        ui.label(RichText::new(text).size(14.0).color(if done {
            colors.text
        } else {
            colors.muted
        }));
    });
}
