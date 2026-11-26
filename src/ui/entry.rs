use super::{
    Inputs, Palette, Screen, feedback, kit, messages, quiet, tr,
    visuals::{self, Presence},
};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, RichText, Stroke};

const CODE_FIELD_CHARS: f32 = 20.0;
const CONTENT_WIDTH: f32 = 1144.0;

fn lead_text(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .line_height(Some(size * 1.08))
        .extra_letter_spacing(-0.045 * size)
}

pub(super) fn home(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    let lead = tr(ui).home.lead;
    let window = ui.ctx().content_rect();
    let compact = window.height() < 560.0;
    let available = ui.available_width();
    let width = available.min(CONTENT_WIDTH);
    let target = (window.width() * 0.047).clamp(if compact { 32.0 } else { 40.0 }, 68.0);
    if window.width() < 860.0 {
        let size = kit::fitted_size(ui, lead, target, width, lead_text);
        ui.add(egui::Label::new(lead_text(lead, size)).wrap_mode(egui::TextWrapMode::Extend));
        ui.add_space(if compact { 16.0 } else { 36.0 });
        start_actions(ui, colors, view, inputs, compact, actions);
        return;
    }
    let gap = (window.width() * 0.08).clamp(48.0, 128.0);
    let column = 400.0_f32.min((width - gap) * 0.48).max(320.0);
    let lead_width = width - gap - column;
    let size = kit::fitted_size(ui, lead, target, lead_width, lead_text);
    let origin = ui.cursor().min + egui::vec2((available - width) * 0.5, 0.0);
    let column_rect = egui::Rect::from_min_size(
        origin + egui::vec2(lead_width + gap, 0.0),
        egui::vec2(column, f32::from(i16::MAX)),
    );
    let placed = ui
        .scope_builder(egui::UiBuilder::new().max_rect(column_rect), |ui| {
            ui.set_width(column);
            start_actions(ui, colors, view, inputs, compact, actions);
        })
        .response
        .rect;
    let height = egui::WidgetText::from(lead_text(lead, size))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .y;
    let lead_rect = egui::Rect::from_min_size(
        egui::pos2(origin.x, placed.center().y - height * 0.5),
        egui::vec2(lead_width, height),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(lead_rect), |ui| {
        ui.add(egui::Label::new(lead_text(lead, size)).wrap_mode(egui::TextWrapMode::Extend));
    });
    ui.advance_cursor_after_rect(placed.union(lead_rect));
}

fn start_actions(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    compact: bool,
    actions: &mut Vec<Command>,
) {
    let t = &tr(ui).home;
    let busy = view.hosting;
    ui.spacing_mut().item_spacing.y = 0.0;
    let error = view
        .error
        .as_deref()
        .filter(|error| !inputs.code_entry && inputs.dismissed_error.as_deref() != Some(*error));
    if error.is_some() || view.notice.is_some() {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            messages(ui, error, view.notice.as_deref());
        });
        ui.add_space(12.0);
    }
    let create = kit::Button::new(
        kit::Kind::Primary,
        if view.hosting {
            t.creating_room
        } else {
            t.create_room
        },
    )
    .trailing(kit::Glyph::ArrowRight)
    .spread()
    .height(if compact { 48.0 } else { 56.0 })
    .busy(view.hosting)
    .enabled(!busy)
    .fill_width(ui)
    .show(ui, colors);
    if create.clicked() {
        inputs.code_entry = false;
        actions.push(Command::HostInternet(inputs.relay.clone()));
    }
    ui.add_space(if compact { 8.0 } else { 14.0 });
    if kit::row(
        ui,
        colors,
        t.join_with_code,
        t.join_with_code_hint,
        compact,
        !busy,
    )
    .clicked()
    {
        inputs.screen = Screen::Join;
        inputs.focus_code = true;
    }
    if kit::row(
        ui,
        colors,
        t.watch_alone,
        t.watch_alone_hint,
        compact,
        !busy,
    )
    .clicked()
    {
        actions.push(Command::Open);
    }
    if inputs.lan {
        if kit::row(ui, colors, t.lan_host, t.lan_host_hint, compact, !busy).clicked() {
            actions.push(Command::HostAuto(inputs.port));
        }
        if kit::row(ui, colors, t.lan_browse, t.lan_browse_hint, compact, !busy).clicked() {
            inputs.screen = Screen::Browse;
            actions.push(Command::Discover);
        }
    }
    if view.player.loaded
        && let Some(file) = view.file.as_deref()
        && kit::row(ui, colors, t.resume_movie, file, compact, true).clicked()
    {
        actions.push(crate::runtime::Command::Control(
            crate::protocol::Control::Play,
        ));
    }
    kit::rule(ui, colors);
    if view.hosting {
        ui.add_space(10.0);
        if quiet(ui, colors, tr(ui).common.cancel).clicked() {
            actions.push(Command::Leave);
        }
    }
}

pub(super) fn join(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    let joining = inputs.joining && view.role == Role::Guest && !view.connected;
    let strings = tr(ui);
    let t = &strings.home;
    let window = ui.ctx().content_rect();
    let compact = window.height() < 560.0;
    let refused = inputs.code_entry && !inputs.code_edited && !joining && view.role == Role::Local;
    let error_text = view.error.as_deref().filter(|_| refused);
    let parts = inputs
        .code
        .split(|c: char| c == '-' || c == '.' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .count();
    let hint = parts >= 3 && crate::network::relay::Code::parse(&inputs.code).is_none();
    ui.vertical_centered(|ui| {
        ui.set_max_width(ui.available_width().min(620.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        visuals::pair(
            ui,
            colors,
            if error_text.is_some() {
                Presence::Apart
            } else {
                Presence::Waiting
            },
            if compact { 30.0 } else { 44.0 },
        );
        ui.add_space(if compact { 12.0 } else { 30.0 });
        let label = ui.label(
            RichText::new(t.code_question)
                .size(16.0)
                .color(colors.muted),
        );
        ui.add_space(if compact { 4.0 } else { 12.0 });
        let width = ui.available_width();
        let size = (window.width() * 0.038)
            .clamp(26.0, 50.0)
            .min(width / (CODE_FIELD_CHARS * 0.63));
        let font = egui::FontId::monospace(size);
        let field = ui
            .add_enabled_ui(!joining, |ui| {
                ui.add_sized(
                    [width, size * 1.2 + 20.0],
                    egui::TextEdit::singleline(&mut inputs.code)
                        .id(egui::Id::new("room_code"))
                        .font(font.clone())
                        .horizontal_align(egui::Align::Center)
                        .vertical_align(egui::Align::Center)
                        .frame(egui::Frame::NONE)
                        .margin(egui::vec2(0.0, 6.0)),
                )
                .labelled_by(label.id)
                .on_hover_text(t.code_tooltip)
            })
            .inner;
        if std::mem::take(&mut inputs.focus_code) {
            field.request_focus();
        }
        if inputs.code.is_empty() {
            ui.painter().text(
                field.rect.center(),
                egui::Align2::CENTER_CENTER,
                t.code_placeholder,
                font,
                colors.faint.gamma_multiply(0.62),
            );
        }
        let line = if error_text.is_some() || hint {
            colors.warning
        } else if field.has_focus() {
            colors.text
        } else {
            colors.border_strong
        };
        ui.painter().hline(
            field.rect.x_range(),
            field.rect.bottom(),
            Stroke::new(1.0, line),
        );
        if field.changed() {
            inputs.code_edited = true;
        }
        ui.add_space(14.0);
        let (help, color) = if let Some(error) = error_text {
            (error, colors.error)
        } else if hint {
            (strings.errors.code_hint, colors.warning)
        } else {
            (t.code_help, colors.muted)
        };
        ui.add(
            egui::Label::new(RichText::new(help).size(14.0).color(color))
                .halign(egui::Align::Center)
                .wrap(),
        );
        ui.add_space(if compact { 12.0 } else { 26.0 });
        let valid = crate::network::relay::Code::parse(&inputs.code).is_some();
        let button = kit::Button::new(
            kit::Kind::Primary,
            if joining {
                t.joining
            } else if error_text.is_some() {
                t.try_again
            } else {
                t.join
            },
        )
        .trailing(kit::Glyph::ArrowRight)
        .width(190.0)
        .busy(joining)
        .enabled(valid && !joining);
        if button.show(ui, colors).clicked()
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
        if !inputs.code_entry && view.notice.is_some() {
            ui.add_space(14.0);
            feedback(ui, view);
        }
    });
}
