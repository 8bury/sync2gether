use super::{
    Body, Inputs, Palette, back_label, configure_theme, kit, request_leave, section_label,
    semibold_family, strong,
};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, Color32, RichText, Stroke};

const STEPS: [&str; 3] = ["Sala", "Filme", "Assistir"];

pub(super) fn header(
    ui: &mut egui::Ui,
    colors: Palette,
    inputs: &mut Inputs,
    view: &View,
    step: Option<usize>,
    body: Body,
    actions: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        let wide = ui.available_width() >= 600.0;
        if let Some(label) = back_label(view, body)
            && kit::icon_button(ui, colors, kit::Glyph::ArrowLeft, label, 36.0).clicked()
        {
            request_leave(view, inputs, actions);
        }
        if let Some(step) = step {
            steps(ui, colors, step, wide);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let settings = kit::icon_button(ui, colors, kit::Glyph::Gear, "Configurações", 36.0);
            egui::Popup::menu(&settings)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| settings_menu(ui, colors, inputs, view.role));
            if wide
                && step.is_some()
                && let Some(code) = view.room_code.as_deref().filter(|_| view.internet)
            {
                room_chip(ui, colors, code, view.connected);
            }
        });
    });
}

fn steps(ui: &mut egui::Ui, colors: Palette, current: usize, labels: bool) {
    ui.spacing_mut().item_spacing.x = 8.0;
    for (index, name) in STEPS.iter().enumerate() {
        if index > 0 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                Stroke::new(1.0, colors.border_strong),
            );
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        let center = rect.center();
        if index < current {
            ui.painter()
                .circle_filled(center, 10.0, colors.success.gamma_multiply(0.16));
            kit::draw(
                ui.painter(),
                kit::Glyph::Check,
                center,
                11.0,
                colors.success,
                0.0,
            );
        } else if index == current {
            ui.painter().circle_filled(center, 10.0, colors.action);
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                (index + 1).to_string(),
                egui::FontId::new(11.5, semibold_family(ui.ctx())),
                Color32::WHITE,
            );
        } else {
            ui.painter()
                .circle_stroke(center, 9.5, Stroke::new(1.0, colors.border_strong));
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                (index + 1).to_string(),
                egui::FontId::proportional(11.5),
                colors.faint,
            );
        }
        if labels || index == current {
            ui.label(
                strong(ui, *name)
                    .size(13.5)
                    .color(match index.cmp(&current) {
                        std::cmp::Ordering::Less => colors.muted,
                        std::cmp::Ordering::Equal => colors.text,
                        std::cmp::Ordering::Greater => colors.faint,
                    }),
            );
        }
    }
}

fn room_chip(ui: &mut egui::Ui, colors: Palette, code: &str, connected: bool) {
    let prefix = RichText::new("Sala ").size(12.5).color(colors.muted);
    let font = egui::FontId::new(12.5, semibold_family(ui.ctx()));
    let galley = ui
        .painter()
        .layout_no_wrap(code.to_owned(), font, colors.text);
    let prefix = egui::WidgetText::from(prefix).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let width = 12.0 + 7.0 + 8.0 + prefix.size().x + galley.size().x + 12.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
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
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    ui.painter().rect(
        rect,
        15,
        kit::lerp(colors.surface, colors.hover, hover),
        Stroke::new(1.0, colors.border),
        egui::StrokeKind::Inside,
    );
    let dot = egui::pos2(rect.left() + 12.0 + 3.5, rect.center().y);
    if connected {
        ui.painter()
            .circle_filled(dot, 6.5, colors.success.gamma_multiply(0.18));
        ui.painter().circle_filled(dot, 3.5, colors.success);
    } else {
        ui.painter().circle_filled(dot, 3.5, colors.faint);
    }
    let x = rect.left() + 12.0 + 7.0 + 8.0;
    let prefix_width = prefix.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - prefix.size().y * 0.5),
        prefix,
        colors.muted,
    );
    ui.painter().galley(
        egui::pos2(x + prefix_width, rect.center().y - galley.size().y * 0.5),
        galley,
        colors.text,
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
                    egui::FontId::proportional(12.0),
                    colors.text,
                );
                ui.label(RichText::new(action).color(colors.muted));
                ui.end_row();
            }
        });
}
