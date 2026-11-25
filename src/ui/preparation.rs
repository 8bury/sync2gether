use super::{
    Palette, copies, feedback, kit, title,
    visuals::{self, Presence},
};
use crate::{
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, Color32, RichText, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Muted,
    Busy,
    Success,
    Warning,
    Error,
}

impl Tone {
    pub(super) fn color(self, colors: Palette) -> Color32 {
        match self {
            Self::Muted | Self::Busy => colors.muted,
            Self::Success => colors.success,
            Self::Warning => colors.warning,
            Self::Error => colors.error,
        }
    }
    pub(super) fn glyph(self) -> Option<kit::Glyph> {
        match self {
            Self::Muted => None,
            Self::Busy => Some(kit::Glyph::Spinner),
            Self::Success => Some(kit::Glyph::Check),
            Self::Warning | Self::Error => Some(kit::Glyph::Alert),
        }
    }
}

enum Main {
    Start,
    Cancel,
    Change,
    Waiting(String),
}

struct Readiness {
    note: String,
    tone: Tone,
    main: Main,
}

fn peer_name(view: &View) -> &str {
    view.peer_name.as_deref().unwrap_or("a outra pessoa")
}

pub(super) fn mismatch(view: &View) -> bool {
    view.role != Role::Local
        && view.connected
        && view.verified
        && view.peer_verified
        && !view.matched
        && !view.loading
        && !view.peer_loading
}

fn start_enabled(view: &View) -> bool {
    view.error.is_none()
        && (view.ready
            || view.role == Role::Local
                && view.player.loaded
                && !view.loading
                && !view.player.blocked)
}

fn readiness(view: &View) -> Readiness {
    let waiting = |note: &str, tone, button: &str| Readiness {
        note: note.into(),
        tone,
        main: Main::Waiting(button.into()),
    };
    if let Some(error) = &view.error {
        return waiting(error, Tone::Error, "Começar a assistir");
    }
    if view.preparing || view.scheduled {
        return Readiness {
            note: "Preparando a reprodução…".into(),
            tone: Tone::Busy,
            main: Main::Cancel,
        };
    }
    if view.loading {
        return if view.pending_file.is_some() {
            waiting(
                "Conferindo o conteúdo do seu arquivo…",
                Tone::Busy,
                "Conferindo seu arquivo…",
            )
        } else {
            waiting(
                "A janela de seleção está aberta neste PC.",
                Tone::Muted,
                "Escolha seu arquivo para começar",
            )
        };
    }
    if view.file.is_none() {
        return waiting(
            if view.role == Role::Local {
                "Escolha um arquivo de vídeo neste PC."
            } else {
                ""
            },
            Tone::Muted,
            "Escolha seu arquivo para começar",
        );
    }
    if view.ready && !view.player.blocked && !view.peer_blocked {
        return Readiness {
            note: "Os arquivos são iguais.".into(),
            tone: Tone::Success,
            main: Main::Start,
        };
    }
    if view.role != Role::Local {
        let peer = peer_name(view);
        if view.peer_loading {
            return Readiness {
                note: format!("O arquivo de {peer} está sendo conferido…"),
                tone: Tone::Busy,
                main: Main::Waiting(format!("Esperando o arquivo de {peer}")),
            };
        }
        if view.peer_file.is_none() && !view.peer_ready {
            return Readiness {
                note: format!("Falta o arquivo de {peer}."),
                tone: Tone::Muted,
                main: Main::Waiting(format!("Esperando o arquivo de {peer}")),
            };
        }
        if mismatch(view) {
            return Readiness {
                note: "Escolham arquivos com o mesmo conteúdo.".into(),
                tone: Tone::Warning,
                main: Main::Change,
            };
        }
    }
    if view.buffering || view.peer_buffering {
        return waiting("Carregando vídeo para iniciar…", Tone::Busy, "Carregando…");
    }
    if view.player.blocked || view.peer_blocked {
        return waiting(
            "Se a espera continuar, selecione o arquivo novamente.",
            Tone::Warning,
            "Aguardando o player",
        );
    }
    if start_enabled(view) {
        return Readiness {
            note: if view.role == Role::Local {
                "Seu filme está pronto."
            } else {
                "Os arquivos são iguais."
            }
            .into(),
            tone: Tone::Success,
            main: Main::Start,
        };
    }
    if view.matched && view.player.loaded && view.peer_ready {
        return waiting(
            "Os arquivos são iguais. Aguardando a conexão estabilizar para liberar o play.",
            Tone::Success,
            "Preparando a conexão…",
        );
    }
    waiting("Preparando os arquivos…", Tone::Busy, "Preparando…")
}

fn heading(view: &View) -> &'static str {
    if start_enabled(view) {
        if view.role == Role::Local {
            "Tudo pronto"
        } else {
            "Vocês têm o mesmo filme"
        }
    } else if view.error.is_some() || (view.player.blocked && !view.buffering) {
        "O filme não abriu"
    } else if mismatch(view) {
        "As cópias são diferentes"
    } else if view.loading && view.pending_file.is_some() || view.peer_loading {
        "Conferindo as cópias"
    } else if compare(view) == Compare::Same {
        "Quase pronto"
    } else if view.role == Role::Local {
        "Escolha o filme"
    } else {
        "Escolham o mesmo filme"
    }
}

fn note(ui: &mut egui::Ui, colors: Palette, text: &str, tone: Tone) {
    let color = tone.color(colors);
    let font = egui::FontId::proportional(14.0);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, color)
        .size()
        .x;
    match tone.glyph() {
        Some(glyph) if width + 24.0 <= ui.available_width() => {
            kit::centered_row(ui, &[16.0, width], |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
                kit::draw(
                    ui.painter(),
                    glyph,
                    rect.center(),
                    15.0,
                    color,
                    ui.input(|i| i.time),
                );
                if glyph == kit::Glyph::Spinner {
                    ui.ctx().request_repaint();
                }
                ui.label(RichText::new(text).size(14.0).color(color));
            });
        }
        _ => {
            ui.add(
                egui::Label::new(RichText::new(text).size(14.0).color(color))
                    .halign(egui::Align::Center)
                    .wrap(),
            );
        }
    }
}

pub(super) fn action(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    let state = readiness(view);
    ui.vertical_centered(|ui| {
        if !state.note.is_empty() {
            note(ui, colors, &state.note, state.tone);
            ui.add_space(2.0);
        }
    });
    ui.vertical_centered(|ui| {
        let button = match &state.main {
            Main::Start => kit::Button::new(kit::Kind::Primary, "Começar a assistir")
                .trailing(kit::Glyph::ArrowRight),
            Main::Cancel => kit::Button::new(kit::Kind::Secondary, "Cancelar início"),
            Main::Change => {
                kit::Button::new(kit::Kind::Primary, "Trocar meu arquivo").glyph(kit::Glyph::Folder)
            }
            Main::Waiting(label) => kit::Button::new(kit::Kind::Primary, label).enabled(false),
        };
        if button.show(ui, colors).clicked() {
            actions.push(match state.main {
                Main::Start => Command::Control(Control::Play),
                Main::Cancel => Command::Control(Control::Pause),
                Main::Change => Command::Open,
                Main::Waiting(_) => return,
            });
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Compare {
    Waiting,
    Checking,
    Same,
    Different,
}

pub(super) fn compare(view: &View) -> Compare {
    let stream_busy = matches!(
        view.stream.phase,
        crate::stream::StreamPhase::Offering
            | crate::stream::StreamPhase::Connecting
            | crate::stream::StreamPhase::Probing
            | crate::stream::StreamPhase::Reconnecting
    ) || view.peer_stream.as_ref().is_some_and(|s| s.reconnecting);
    if mismatch(view) {
        Compare::Different
    } else if view.loading || view.peer_loading || (stream_busy && !view.matched) {
        Compare::Checking
    } else if view.matched && view.verified && view.peer_verified {
        Compare::Same
    } else {
        Compare::Waiting
    }
}

#[derive(Clone, Copy)]
pub(super) struct Fit {
    pub(super) sticky: bool,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    fit: Fit,
    actions: &mut Vec<Command>,
) {
    let Fit { sticky } = fit;
    let window = ui.ctx().content_rect();
    let compact = window.height() < 700.0;
    ui.spacing_mut().item_spacing.y = if compact { 6.0 } else { 10.0 };
    if let Some(request) = &view.join_request {
        super::room::approval(ui, colors, request, actions);
        ui.add_space(8.0);
    }
    let size = (window.width() * 0.03).clamp(if compact { 24.0 } else { 28.0 }, 38.0);
    ui.vertical_centered(|ui| {
        ui.label(title(heading(view), size));
    });
    ui.add_space(if compact { 8.0 } else { 20.0 });
    copies::pair(ui, colors, view, actions);
    if view.role != Role::Local && view.connected {
        ui.add_space(if compact { 4.0 } else { 8.0 });
        super::stream::preparation_strip(ui, colors, view, actions);
    }
    if !sticky && view.join_request.is_none() {
        ui.add_space(if compact { 6.0 } else { 12.0 });
        action(ui, colors, view, actions);
    }
    if view.error.is_none() {
        feedback(ui, view);
    }
}

pub(super) fn room(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    let window = ui.ctx().content_rect();
    let compact = window.height() < 560.0;
    if let Some(request) = &view.join_request {
        super::room::approval(ui, colors, request, actions);
        ui.add_space(12.0);
    }
    feedback(ui, view);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        visuals::pair(
            ui,
            colors,
            Presence::Waiting,
            if compact { 30.0 } else { 44.0 },
        );
        ui.add_space(if compact { 10.0 } else { 30.0 });
        ui.label(
            RichText::new(if view.internet {
                "Envie este código"
            } else {
                "Envie este endereço"
            })
            .size(16.0)
            .color(colors.muted),
        );
        ui.add_space(if compact { 2.0 } else { 10.0 });
        if view.internet {
            invite_code(ui, colors, view, compact);
        } else {
            for address in super::room_addresses(view) {
                ui.push_id(address, |ui| {
                    invite(ui, colors, &address.to_string(), "Copiar endereço", compact)
                });
            }
        }
        if view.relay_offline {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Sem contato com o servidor. Tentando de novo…")
                    .size(13.0)
                    .color(colors.warning),
            );
        }
    });
}

fn code_text(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .font(egui::FontId::monospace(size))
        .line_height(Some(size * 1.15))
        .extra_letter_spacing(-0.05 * size)
}

fn invite(ui: &mut egui::Ui, colors: Palette, value: &str, copy: &str, compact: bool) {
    let window = ui.ctx().content_rect();
    let target = (window.width() * 0.052).clamp(if compact { 30.0 } else { 38.0 }, 72.0);
    let size = kit::fitted_size(ui, value, target, ui.available_width(), code_text);
    ui.add(
        egui::Label::new(code_text(value, size).color(colors.text))
            .wrap_mode(egui::TextWrapMode::Extend)
            .selectable(true),
    );
    ui.add_space(if compact { 8.0 } else { 26.0 });
    copy_button_with(ui, kit::Kind::Secondary, kit::Size::Medium, copy, value);
}

pub(super) fn copyable(
    ui: &mut egui::Ui,
    colors: Palette,
    value: &str,
    accessible: &str,
    text: impl Fn(Color32) -> RichText,
) {
    let id = ui.id().with(("copyable", value));
    let now = ui.input(|i| i.time);
    let elapsed = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(id))
        .map(|at| now - at)
        .filter(|elapsed| (0.0..COPIED_FOR).contains(elapsed));
    let copied = elapsed.is_some();
    let flash = ui
        .ctx()
        .animate_bool_with_time(id.with("flash"), copied, 0.18);
    let hover_id = id.with("hover");
    let hovered = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(hover_id))
        .unwrap_or(false);
    let hover = ui.ctx().animate_bool_with_time(hover_id, hovered, 0.15);
    let response = ui
        .add(
            egui::Label::new(text(
                kit::lerp(colors.muted, colors.text, hover).gamma_multiply(1.0 - flash),
            ))
            .sense(egui::Sense::click())
            .show_tooltip_when_elided(false)
            .truncate(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add(egui::Label::new(value).wrap());
            kit::status(
                ui,
                Some(if copied {
                    kit::Glyph::Check
                } else {
                    kit::Glyph::Copy
                }),
                if copied {
                    "Copiado"
                } else {
                    "Clique para copiar"
                },
                colors.muted,
            );
        });
    ui.ctx()
        .data_mut(|d| d.insert_temp(hover_id, response.hovered()));
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(3.0),
            4,
            Stroke::new(2.0, colors.text),
            egui::StrokeKind::Outside,
        );
    }
    if let Some(elapsed) = elapsed {
        copied_flash(ui, colors, response.rect, elapsed as f32, flash);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible));
    if response.clicked() {
        ui.ctx().copy_text(value.to_owned());
        ui.ctx().data_mut(|d| d.insert_temp(id, now));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(COPIED_FOR));
    }
}

const COPIED_FOR: f64 = 1.6;

fn copied_flash(ui: &egui::Ui, colors: Palette, rect: egui::Rect, elapsed: f32, flash: f32) {
    let pulse = (1.0 - elapsed / 0.6).clamp(0.0, 1.0);
    if pulse > 0.0 {
        ui.ctx().request_repaint();
    }
    let bounds = ui.max_rect();
    let painter = ui.painter_at(bounds.intersect(ui.clip_rect()));
    let ink = colors.success.gamma_multiply(flash);
    let pop = 1.0 + 0.35 * pulse * pulse;
    let center = egui::pos2(rect.left() + 7.0, rect.center().y);
    kit::draw(
        &painter,
        kit::Glyph::Check,
        center,
        13.0 * pop,
        ink,
        ui.input(|i| i.time),
    );
    let x = rect.left() + 20.0 - 6.0 * (1.0 - flash);
    let room = bounds.right() - (rect.left() + 20.0);
    let font = egui::FontId::proportional(13.0);
    let label = ["Nome copiado", "Copiado"].into_iter().find(|label| {
        painter
            .layout_no_wrap((*label).to_owned(), font.clone(), ink)
            .size()
            .x
            <= room
    });
    if let Some(label) = label {
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            ink,
        );
    }
}

fn invite_code(ui: &mut egui::Ui, colors: Palette, view: &View, compact: bool) {
    match &view.room_code {
        Some(code) => invite(ui, colors, code, "Copiar código", compact),
        None => {
            ui.add_space(12.0);
            kit::centered_row(ui, &[18.0, 140.0], |ui| {
                kit::status(
                    ui,
                    Some(kit::Glyph::Spinner),
                    "Gerando o código…",
                    colors.muted,
                );
            });
        }
    }
}

pub(super) fn leave_label(role: Role) -> &'static str {
    match role {
        Role::Host => "Encerrar sala",
        Role::Guest => "Sair da sala",
        Role::Local => "Voltar ao início",
    }
}

pub(super) fn copy_button(ui: &mut egui::Ui, label: &str, value: &str) {
    copy_button_with(ui, kit::Kind::Secondary, kit::Size::Small, label, value);
}

pub(super) fn copy_button_with(
    ui: &mut egui::Ui,
    kind: kit::Kind,
    size: kit::Size,
    label: &str,
    value: &str,
) {
    let colors = Palette::new(ui.visuals().dark_mode);
    let id = ui.id().with(("copied", value));
    let now = ui.input(|i| i.time);
    let copied = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|until| until > now);
    if kit::Button::new(
        if copied { kit::Kind::Secondary } else { kind },
        if copied { "Copiado" } else { label },
    )
    .glyph(if copied {
        kit::Glyph::Check
    } else {
        kit::Glyph::Copy
    })
    .size(size)
    .show(ui, colors)
    .clicked()
    {
        ui.ctx().copy_text(value.to_owned());
        ui.ctx().data_mut(|d| d.insert_temp(id, now + 2.0));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(2));
    }
}
