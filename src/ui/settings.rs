use super::{
    Body, Inputs, Palette, Screen, back_label, configure_theme, kit, medium_family, request_leave,
    section_label, visuals,
};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, RichText, Stroke};

pub(super) fn header(
    ui: &mut egui::Ui,
    colors: Palette,
    inputs: &mut Inputs,
    view: &View,
    body: Body,
    actions: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        let wide = ui.available_width() >= 600.0;
        if body == Body::Home {
            visuals::brand(ui, colors);
        } else {
            leave_action(ui, colors, inputs, view, body, actions);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let settings = kit::icon_button(ui, colors, kit::Glyph::Gear, "Configurações", 36.0);
            egui::Popup::menu(&settings)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| settings_menu(ui, colors, inputs, view.role));
            if wide
                && body == Body::Preparation
                && let Some(code) = view.room_code.as_deref().filter(|_| view.internet)
            {
                room_chip(ui, colors, code);
            }
        });
    });
}

fn leave_action(
    ui: &mut egui::Ui,
    colors: Palette,
    inputs: &mut Inputs,
    view: &View,
    body: Body,
    actions: &mut Vec<Command>,
) {
    let joining = inputs.joining && view.role == Role::Guest && !view.connected;
    let label = match body {
        Body::Home => return,
        Body::Browse => "Voltar",
        Body::Join if joining => "Cancelar",
        Body::Join => "Voltar",
        _ => back_label(view, body).unwrap_or("Voltar"),
    };
    let clicked = kit::Button::new(kit::Kind::Ghost, label)
        .glyph(kit::Glyph::ChevronLeft)
        .size(kit::Size::Small)
        .show(ui, colors)
        .clicked();
    if !clicked {
        return;
    }
    match body {
        Body::Browse => {
            inputs.screen = Screen::Home;
            actions.push(Command::CancelDiscovery);
        }
        Body::Join if joining => actions.push(Command::Leave),
        Body::Join => {
            inputs.screen = Screen::Home;
            inputs.joining = false;
            inputs.code_entry = false;
            inputs.code_edited = false;
            inputs.dismissed_error = view.error.clone();
        }
        _ => request_leave(view, inputs, actions),
    }
}

fn room_chip(ui: &mut egui::Ui, colors: Palette, code: &str) {
    let font = egui::FontId::monospace(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(code.to_owned(), font, colors.muted);
    let size = galley.size() + egui::vec2(20.0, 14.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(size.x, size.y.max(32.0)), egui::Sense::click());
    let label = format!("Copiar código da sala {code}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let id = ui.id().with(("copied", code));
    let now = ui.input(|i| i.time);
    if response.clicked() {
        ui.ctx().copy_text(code.to_owned());
        ui.ctx().data_mut(|d| d.insert_temp(id, now + 2.0));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(2));
    }
    let copied = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|until| until > now);
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.15);
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 6, colors.hover.gamma_multiply(hover));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            8,
            Stroke::new(2.0, colors.text),
            egui::StrokeKind::Outside,
        );
    }
    let ink = kit::lerp(colors.muted, colors.text, hover);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        ink,
    );
    response
        .on_hover_text(if copied { "Copiado" } else { "Copiar código" })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn settings_menu(ui: &mut egui::Ui, colors: Palette, inputs: &mut Inputs, role: Role) {
    ui.set_width(250.0);
    ui.spacing_mut().item_spacing.y = 8.0;
    section_label(ui, colors, "Aparência");
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(colors.inset)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(8)
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
                            6,
                            colors.raised,
                            Stroke::new(1.0, colors.border_strong),
                            egui::StrokeKind::Inside,
                        );
                    } else if response.hovered() {
                        column.painter().rect_filled(rect, 6, colors.hover);
                    }
                    column.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::new(14.0, medium_family(column.ctx())),
                        if selected { colors.text } else { colors.muted },
                    );
                    if response.clicked() && !selected {
                        configure_theme(column.ctx(), light);
                    }
                }
            });
        });
    ui.add_space(4.0);
    section_label(ui, colors, "Rede");
    let local = role == Role::Local;
    ui.add_enabled(
        local,
        egui::Checkbox::new(&mut inputs.lan, "Opções avançadas de rede"),
    )
    .on_hover_text("Salas na rede local ou VPN, entrada por endereço e servidor próprio");
    if inputs.lan {
        ui.label(
            RichText::new("Porta das salas na rede local")
                .small()
                .color(colors.muted),
        );
        ui.add_enabled(
            local,
            egui::DragValue::new(&mut inputs.port).range(1..=65535),
        );
        let label = ui.label(
            RichText::new("Servidor das salas pela internet")
                .small()
                .color(colors.muted),
        );
        ui.add_enabled(
            local,
            egui::TextEdit::singleline(&mut inputs.relay)
                .id(egui::Id::new("relay_address"))
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
        if let Err(error) = crate::network::relay::relay_address(&inputs.relay) {
            ui.add(egui::Label::new(RichText::new(error).small().color(colors.warning)).wrap());
        }
    }
    if !local {
        ui.label(
            RichText::new("Altere antes de criar ou entrar em outra sala.")
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
                    egui::FontId::monospace(12.0),
                    colors.text,
                );
                ui.label(RichText::new(action).color(colors.muted));
                ui.end_row();
            }
        });
}
