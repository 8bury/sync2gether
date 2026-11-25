use super::{
    Palette, kit,
    preparation::{Compare, Tone, compare, copy_button, mismatch},
    strong,
    visuals::{self, Presence},
};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, RichText, Stroke};

const LINK: f32 = 76.0;
const MENU: f32 = 32.0;
const TIGHT_FRAME: f32 = 52.0;
const TIGHT_BELOW: f32 = 330.0;

#[derive(Clone, Copy)]
struct Metrics {
    who: f32,
    meta: f32,
    gap: f32,
    link: f32,
    min_frame: f32,
}

impl Metrics {
    fn new(tight: bool) -> Self {
        if tight {
            Self {
                who: 26.0,
                meta: 32.0,
                gap: 18.0,
                link: 12.0,
                min_frame: TIGHT_FRAME,
            }
        } else {
            Self {
                who: 32.0,
                meta: 40.0,
                gap: 30.0,
                link: 22.0,
                min_frame: 84.0,
            }
        }
    }

    fn frame(self, column: f32, free: f32, apart: bool) -> f32 {
        let ratio = if apart { 2.1 } else { 16.0 / 9.0 };
        (column / ratio).min(free.max(self.min_frame)).round()
    }
}

pub(super) fn pair(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    free: f32,
    actions: &mut Vec<Command>,
) {
    let width = ui.available_width();
    let apart = mismatch(view);
    if view.role == Role::Local {
        let metrics = Metrics::new(false);
        let column = width.min(560.0);
        let frame = metrics.frame(column, free - metrics.who - metrics.meta, false);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width, metrics.who + frame + metrics.meta),
            egui::Sense::hover(),
        );
        let copy = egui::Rect::from_center_size(rect.center(), egui::vec2(column, rect.height()));
        copy_card(
            ui,
            CardSlot {
                rect: copy,
                frame_height: frame,
                metrics,
            },
            colors,
            view,
            true,
            actions,
        );
        return;
    }
    let wide = width >= 620.0;
    let metrics = Metrics::new(!wide && free < TIGHT_BELOW);
    let chrome = metrics.who + metrics.meta;
    let top = ui.cursor().min;
    let (mine, theirs, link, frame, total) = if wide {
        let column = (width - LINK) * 0.5;
        let frame = metrics.frame(column, free - chrome, apart);
        let size = egui::vec2(column, chrome + frame);
        (
            egui::Rect::from_min_size(top, size),
            egui::Rect::from_min_size(top + egui::vec2(column + LINK, 0.0), size),
            egui::pos2(top.x + width * 0.5, top.y + metrics.who + frame * 0.5),
            frame,
            size.y,
        )
    } else {
        let frame = metrics.frame(width, (free - metrics.gap) * 0.5 - chrome, apart);
        let size = egui::vec2(width, chrome + frame);
        (
            egui::Rect::from_min_size(top, size),
            egui::Rect::from_min_size(top + egui::vec2(0.0, size.y + metrics.gap), size),
            egui::pos2(top.x + width * 0.5, top.y + size.y + metrics.gap * 0.5),
            frame,
            2.0 * size.y + metrics.gap,
        )
    };
    ui.allocate_exact_size(egui::vec2(width, total), egui::Sense::hover());
    ui.push_id("local_file", |ui| {
        copy_card(
            ui,
            CardSlot {
                rect: mine,
                frame_height: frame,
                metrics,
            },
            colors,
            view,
            true,
            actions,
        )
    });
    ui.push_id("peer_file", |ui| {
        copy_card(
            ui,
            CardSlot {
                rect: theirs,
                frame_height: frame,
                metrics,
            },
            colors,
            view,
            false,
            actions,
        )
    });
    let state = match compare(view) {
        Compare::Same => Presence::Together,
        Compare::Different => Presence::Apart,
        Compare::Waiting | Compare::Checking => Presence::Waiting,
    };
    visuals::paint_pair(
        ui.painter(),
        link - egui::vec2(visuals::pair_width(state, metrics.link) * 0.5, 0.0),
        metrics.link,
        state,
        colors,
    );
}

#[derive(Clone, Copy)]
struct CardSlot {
    rect: egui::Rect,
    frame_height: f32,
    metrics: Metrics,
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
    slot: CardSlot,
    colors: Palette,
    view: &View,
    local: bool,
    actions: &mut Vec<Command>,
) {
    let CardSlot {
        rect,
        frame_height,
        metrics,
    } = slot;
    let state = copy_state(view, local);
    let who = if local {
        "Você"
    } else {
        view.peer_name.as_deref().unwrap_or("Outra pessoa")
    };
    who_row(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), metrics.who)),
        colors,
        who,
        &state,
    );
    let frame = egui::Rect::from_min_size(
        rect.min + egui::vec2(0.0, metrics.who),
        egui::vec2(rect.width(), frame_height),
    );
    if state.name.is_some() {
        ui.painter().rect(
            frame,
            8,
            colors.inset,
            Stroke::new(
                1.0,
                if state.tone == Tone::Warning {
                    colors.warning.gamma_multiply(0.45)
                } else {
                    colors.border
                },
            ),
            egui::StrokeKind::Inside,
        );
    } else {
        ui.painter().rect(
            frame,
            8,
            colors.raised,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
    }
    if local && state.name.is_none() && !state.loading {
        let choose = kit::Button::new(kit::Kind::Primary, "Escolher arquivo")
            .glyph(kit::Glyph::Folder)
            .size(kit::Size::Medium);
        let size = egui::vec2(choose.natural_width(ui), 40.0);
        let slot = egui::Rect::from_center_size(frame.center(), size);
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
            if choose.show(ui, colors).clicked() {
                actions.push(Command::Open);
            }
        });
    } else if let Some(progress) = state.progress {
        let width = (frame.width() - 32.0).clamp(80.0, 260.0);
        let slot = egui::Rect::from_center_size(frame.center(), egui::vec2(width, 20.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
            kit::progress(ui, colors, progress.fraction(), width);
        });
    } else if state.name.is_some() {
        let size = (frame.height() * 0.42).clamp(30.0, 56.0);
        let ink = if state.tone == Tone::Warning {
            colors.warning
        } else {
            colors.faint
        };
        visuals::file_glyph(ui.painter(), frame.center(), size, ink);
    }
    let meta = egui::Rect::from_min_max(
        egui::pos2(rect.left(), frame.bottom()),
        egui::pos2(rect.right(), rect.bottom()),
    );
    meta_row(ui, meta, colors, view, local, &state, actions);
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

fn meta_row(
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
        ui.spacing_mut().item_spacing.x = 4.0;
        let menu = if state.name.is_some() { MENU } else { 0.0 };
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
                } else {
                    None
                }
            })
            .flatten();
        let buttons = button.as_ref().map_or(0.0, |b| b.natural_width(ui) + 4.0) + menu;
        let room = (ui.available_width() - buttons - 8.0).max(20.0);
        ui.allocate_ui_with_layout(
            egui::vec2(room, rect.height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(room);
                match state.name {
                    Some(name) => {
                        ui.add(
                            egui::Label::new(
                                RichText::new(name)
                                    .font(egui::FontId::monospace(12.5))
                                    .color(colors.muted),
                            )
                            .truncate(),
                        )
                        .on_hover_text(name);
                    }
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
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(name) = state.name {
                file_menu(ui, colors, name, local);
            }
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

fn file_menu(ui: &mut egui::Ui, colors: Palette, name: &str, local: bool) {
    let label = if local {
        "Opções do seu arquivo"
    } else {
        "Opções do arquivo da outra pessoa"
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(MENU, MENU), egui::Sense::click());
    let response = response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered() || open,
        0.15,
    );
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 6, colors.hover.gamma_multiply(hover));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            egui::Stroke::new(2.0, colors.text),
            egui::StrokeKind::Inside,
        );
    }
    for offset in [-5.0, 0.0, 5.0] {
        ui.painter().circle_filled(
            rect.center() + egui::vec2(offset, 0.0),
            1.5,
            kit::lerp(colors.muted, colors.text, hover),
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_max_width(320.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(strong(ui, "Nome completo").size(13.5).color(colors.muted));
            ui.add(egui::Label::new(name).wrap().selectable(true));
            copy_button(ui, "Copiar nome", name);
        });
}
