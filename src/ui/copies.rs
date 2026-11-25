use super::{
    Palette, kit,
    preparation::{Compare, Tone, compare, copyable, mismatch},
    strong,
    visuals::{self, Presence},
};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, RichText, Stroke};

const LINK: f32 = 76.0;
const WHO: f32 = 32.0;
const ROW: f32 = 56.0;
const STACK_GAP: f32 = 22.0;
const PAD: f32 = 14.0;

pub(super) fn pair(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    let width = ui.available_width();
    let top = ui.cursor().min;
    let card = egui::vec2(0.0, WHO + ROW);
    if view.role == Role::Local {
        let column = width.min(560.0);
        ui.allocate_exact_size(egui::vec2(width, card.y), egui::Sense::hover());
        let rect = egui::Rect::from_min_size(
            top + egui::vec2((width - column) * 0.5, 0.0),
            egui::vec2(column, card.y),
        );
        copy_card(ui, rect, colors, view, true, actions);
        return;
    }
    let (mine, theirs, link, total) = if width >= 620.0 {
        let column = (width - LINK) * 0.5;
        let size = egui::vec2(column, card.y);
        (
            egui::Rect::from_min_size(top, size),
            egui::Rect::from_min_size(top + egui::vec2(column + LINK, 0.0), size),
            egui::pos2(top.x + width * 0.5, top.y + WHO + ROW * 0.5),
            card.y,
        )
    } else {
        let size = egui::vec2(width, card.y);
        (
            egui::Rect::from_min_size(top, size),
            egui::Rect::from_min_size(top + egui::vec2(0.0, card.y + STACK_GAP), size),
            egui::pos2(top.x + width * 0.5, top.y + card.y + STACK_GAP * 0.5),
            2.0 * card.y + STACK_GAP,
        )
    };
    ui.allocate_exact_size(egui::vec2(width, total), egui::Sense::hover());
    ui.push_id("local_file", |ui| {
        copy_card(ui, mine, colors, view, true, actions)
    });
    ui.push_id("peer_file", |ui| {
        copy_card(ui, theirs, colors, view, false, actions)
    });
    let state = match compare(view) {
        Compare::Same => Presence::Together,
        Compare::Different => Presence::Apart,
        Compare::Waiting | Compare::Checking => Presence::Waiting,
    };
    let diameter = 16.0;
    visuals::paint_pair(
        ui.painter(),
        link - egui::vec2(visuals::pair_width(state, diameter) * 0.5, 0.0),
        diameter,
        state,
        colors,
    );
}

struct CopyState<'a> {
    name: Option<&'a str>,
    loading: bool,
    progress: Option<crate::protocol::Verification>,
    status: String,
    tone: Tone,
    streamed: bool,
}

fn copy_state<'a>(view: &'a View, local: bool) -> CopyState<'a> {
    let stream_card = super::stream::card_stream_status(view, local);
    let streamed = stream_card.is_some();
    let name = if local {
        if view.loading {
            view.pending_file.as_deref()
        } else {
            view.file.as_deref()
        }
    } else {
        view.peer_file.as_deref()
    };
    let loading = if local {
        view.loading
    } else {
        view.peer_loading
    };
    let buffering = if local {
        view.buffering || view.player.buffering
    } else {
        view.peer_buffering
    };
    let blocked = (if local {
        view.player.blocked
    } else {
        view.peer_blocked
    }) && !buffering;
    let progress = if local {
        view.verification
    } else {
        view.peer_verification
    };
    let verified = (if local {
        view.verified
    } else {
        view.peer_verified
    }) && !loading
        && !blocked
        && !buffering
        && (local || view.connected);
    let (status, tone): (String, Tone) = if let Some(card) = stream_card {
        match card {
            super::stream::CardStream::Busy(label) => (label.into(), Tone::Busy),
            super::stream::CardStream::Ready(label) => (label, Tone::Success),
        }
    } else if !local && view.peer_left && !view.connected {
        ("Saiu da sala".into(), Tone::Muted)
    } else if !local && !view.connected {
        ("Ainda não entrou na sala".into(), Tone::Muted)
    } else if loading && name.is_none() {
        ("Escolhendo arquivo…".into(), Tone::Busy)
    } else if loading {
        ("Conferindo…".into(), Tone::Busy)
    } else if buffering {
        ("Carregando vídeo…".into(), Tone::Busy)
    } else if blocked {
        ("Player indisponível".into(), Tone::Warning)
    } else if mismatch(view) {
        ("Conteúdo diferente".into(), Tone::Warning)
    } else if verified {
        ("Conferido".into(), Tone::Success)
    } else if name.is_some() {
        ("Preparando o player…".into(), Tone::Muted)
    } else if !local {
        ("Aguardando a escolha do filme".into(), Tone::Muted)
    } else {
        ("Falta escolher".into(), Tone::Muted)
    };
    CopyState {
        name,
        loading,
        progress: progress.filter(|_| loading),
        status,
        tone,
        streamed,
    }
}

fn copy_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    colors: Palette,
    view: &View,
    local: bool,
    actions: &mut Vec<Command>,
) {
    let state = copy_state(view, local);
    let who = if local {
        "Você"
    } else {
        view.peer_name.as_deref().unwrap_or("Outra pessoa")
    };
    who_row(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), WHO)),
        colors,
        who,
        &state,
    );
    let row = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, WHO), rect.max);
    let warning = state.tone == Tone::Warning && state.name.is_some();
    ui.painter().rect(
        row,
        8,
        if state.name.is_some() {
            colors.inset
        } else {
            colors.raised
        },
        Stroke::new(
            1.0,
            if warning {
                colors.warning.gamma_multiply(0.45)
            } else {
                colors.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let inner = row.shrink2(egui::vec2(PAD, 0.0));
    file_row(ui, inner, colors, view, local, &state, actions);
}

fn who_row(ui: &egui::Ui, rect: egui::Rect, colors: Palette, who: &str, state: &CopyState<'_>) {
    let tone = state.tone.color(colors);
    let tone = if state.tone == Tone::Muted {
        colors.faint
    } else {
        tone
    };
    let glyph = state.tone.glyph();
    let glyph_room = if glyph.is_some() { 18.0 } else { 0.0 };
    let status = kit::single_line(
        ui,
        RichText::new(&state.status).size(13.0).color(tone),
        (rect.width() * 0.62 - glyph_room).max(40.0),
        egui::TextStyle::Small,
    );
    let status_width = status.size().x;
    let name_room = (rect.width() - status_width - glyph_room - 12.0).max(24.0);
    let name = kit::single_line(
        ui,
        strong(ui, who).size(15.0).color(colors.text),
        name_room,
        egui::TextStyle::Body,
    );
    let y = rect.center().y - 2.0;
    ui.painter().galley(
        egui::pos2(rect.left(), y - name.size().y * 0.5),
        name,
        colors.text,
    );
    let text_x = rect.right() - status_width;
    if let Some(glyph) = glyph {
        kit::draw(
            ui.painter(),
            glyph,
            egui::pos2(text_x - 11.0, y),
            13.0,
            tone,
            ui.input(|i| i.time),
        );
        if glyph == kit::Glyph::Spinner {
            ui.ctx().request_repaint();
        }
    }
    ui.painter()
        .galley(egui::pos2(text_x, y - status.size().y * 0.5), status, tone);
}

fn file_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    colors: Palette,
    view: &View,
    local: bool,
    state: &CopyState<'_>,
    actions: &mut Vec<Command>,
) {
    let builder = egui::UiBuilder::new()
        .max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let button = local
            .then(|| {
                if state.loading && view.pending_file.is_some() {
                    Some(
                        kit::Button::new(kit::Kind::Ghost, "Cancelar verificação")
                            .size(kit::Size::Small),
                    )
                } else if !state.loading && state.name.is_some() {
                    Some(
                        kit::Button::new(kit::Kind::Ghost, "Trocar arquivo")
                            .glyph(kit::Glyph::Refresh)
                            .size(kit::Size::Small),
                    )
                } else if !state.loading {
                    Some(
                        kit::Button::new(kit::Kind::Primary, "Escolher arquivo")
                            .glyph(kit::Glyph::Folder)
                            .size(kit::Size::Small),
                    )
                } else {
                    None
                }
            })
            .flatten();
        let buttons = button.as_ref().map_or(0.0, |b| b.natural_width(ui) + 8.0);
        let room = (ui.available_width() - buttons).max(20.0);
        ui.allocate_ui_with_layout(
            egui::vec2(room, rect.height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(room);
                if state.name.is_some() {
                    let (glyph, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 22.0), egui::Sense::hover());
                    visuals::file_glyph(
                        ui.painter(),
                        glyph.center(),
                        20.0,
                        if state.tone == Tone::Warning {
                            colors.warning
                        } else {
                            colors.faint
                        },
                    );
                }
                let bar = state.progress.map(|progress| {
                    let width = (room * 0.4).clamp(80.0, 180.0);
                    (progress, width)
                });
                let name_room = (room - 26.0 - bar.map_or(0.0, |(_, width)| width + 8.0)).max(20.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(name_room, rect.height()),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| name_label(ui, colors, local, state),
                );
                if let Some((progress, width)) = bar {
                    kit::progress(ui, colors, progress.fraction(), width);
                }
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(button) = button
                && button.show(ui, colors).clicked()
            {
                actions.push(if state.loading {
                    Command::CancelOpen
                } else {
                    Command::Open
                });
            }
        });
    });
}

fn name_label(ui: &mut egui::Ui, colors: Palette, local: bool, state: &CopyState<'_>) {
    match state.name {
        Some(name) => copyable(
            ui,
            colors,
            name,
            if local {
                "Copiar nome do seu arquivo"
            } else {
                "Copiar nome do arquivo da outra pessoa"
            },
            |color| {
                RichText::new(name)
                    .font(egui::FontId::monospace(12.5))
                    .color(color)
            },
        ),
        None => {
            ui.label(
                RichText::new(if state.streamed && !local {
                    "Recebendo seu filme"
                } else {
                    "Nenhum arquivo ainda"
                })
                .font(egui::FontId::monospace(12.5))
                .color(colors.faint),
            );
        }
    }
}
