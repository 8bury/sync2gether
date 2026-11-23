use super::{Palette, connection_details, feedback, kit, strong, title, visuals};
use crate::{
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, Color32, RichText, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Busy,
    Success,
    Warning,
    Error,
}

impl Tone {
    fn color(self, colors: Palette) -> Color32 {
        match self {
            Self::Muted | Self::Busy => colors.muted,
            Self::Success => colors.success,
            Self::Warning => colors.warning,
            Self::Error => colors.error,
        }
    }
    fn glyph(self) -> Option<kit::Glyph> {
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

fn mismatch(view: &View) -> bool {
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
                "Cada pessoa escolhe a própria cópia. O app compara o conteúdo, não o nome."
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
                note: "Os arquivos são diferentes. Escolham arquivos com o mesmo conteúdo.".into(),
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
        "Tudo pronto"
    } else if view.error.is_some() || (view.player.blocked && !view.buffering) {
        "O filme não abriu"
    } else if mismatch(view) {
        "Os arquivos não batem"
    } else if view.loading && view.pending_file.is_some() || view.peer_loading {
        "Conferindo os arquivos"
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
        note(ui, colors, &state.note, state.tone);
        ui.add_space(6.0);
    });
    ui.vertical_centered(|ui| {
        ui.set_max_width(ui.available_width().min(340.0));
        let button = match &state.main {
            Main::Start => {
                kit::Button::new(kit::Kind::Primary, "Começar a assistir").glyph(kit::Glyph::Play)
            }
            Main::Cancel => kit::Button::new(kit::Kind::Secondary, "Cancelar início"),
            Main::Change => kit::Button::new(kit::Kind::Secondary, "Trocar meu arquivo")
                .glyph(kit::Glyph::Refresh),
            Main::Waiting(label) => kit::Button::new(kit::Kind::Secondary, label)
                .glyph(kit::Glyph::Play)
                .enabled(false),
        };
        if button.fill_width(ui).show(ui, colors).clicked() {
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
enum Compare {
    Waiting,
    Checking,
    Same,
    Different,
}

fn compare(view: &View) -> Compare {
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

fn seam(ui: &egui::Ui, colors: Palette, center: egui::Pos2, state: Compare) {
    let painter = ui.painter();
    painter.circle_filled(center, 23.0, colors.background);
    let (fill, ink) = match state {
        Compare::Same => (colors.success, colors.background),
        Compare::Different => (colors.warning, colors.background),
        Compare::Waiting | Compare::Checking => (colors.raised, colors.muted),
    };
    painter.circle_filled(center, 18.0, fill);
    if matches!(state, Compare::Waiting | Compare::Checking) {
        painter.circle_stroke(center, 17.5, Stroke::new(1.0, colors.border));
    }
    match state {
        Compare::Waiting => {
            painter.circle_filled(center, 3.0, colors.faint);
        }
        Compare::Checking => {
            kit::draw(
                painter,
                kit::Glyph::Spinner,
                center,
                16.0,
                colors.accent,
                ui.input(|i| i.time),
            );
            ui.ctx().request_repaint();
        }
        Compare::Same => kit::draw(painter, kit::Glyph::Check, center, 16.0, ink, 0.0),
        Compare::Different => kit::draw(painter, kit::Glyph::Close, center, 14.0, ink, 0.0),
    }
}

const CARD: f32 = 118.0;
const GAP: f32 = 16.0;

fn pair(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    let width = ui.available_width();
    if view.role == Role::Local {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CARD), egui::Sense::hover());
        copy_card(ui, rect, colors, view, true, actions);
        return;
    }
    let wide = width >= 560.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, if wide { CARD } else { 2.0 * CARD + GAP }),
        egui::Sense::hover(),
    );
    let (mine, theirs) = if wide {
        let half = (width - GAP) * 0.5;
        (
            egui::Rect::from_min_size(rect.min, egui::vec2(half, CARD)),
            egui::Rect::from_min_size(
                rect.min + egui::vec2(half + GAP, 0.0),
                egui::vec2(half, CARD),
            ),
        )
    } else {
        (
            egui::Rect::from_min_size(rect.min, egui::vec2(width, CARD)),
            egui::Rect::from_min_size(
                rect.min + egui::vec2(0.0, CARD + GAP),
                egui::vec2(width, CARD),
            ),
        )
    };
    ui.push_id("local_file", |ui| {
        copy_card(ui, mine, colors, view, true, actions)
    });
    ui.push_id("peer_file", |ui| {
        copy_card(ui, theirs, colors, view, false, actions)
    });
    let center = if wide {
        egui::pos2(rect.center().x, rect.center().y)
    } else {
        egui::pos2(rect.center().x, mine.bottom() + GAP * 0.5)
    };
    seam(ui, colors, center, compare(view));
}

fn copy_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    colors: Palette,
    view: &View,
    local: bool,
    actions: &mut Vec<Command>,
) {
    let stream_card = super::stream::card_stream_status(view, local);
    let is_stream_card = stream_card.is_some();
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
    let different = mismatch(view);
    let (status, tone): (String, Tone) = if let Some(card) = stream_card {
        match card {
            super::stream::CardStream::Busy(label) => (label.into(), Tone::Busy),
            super::stream::CardStream::Ready(label) => (label, Tone::Success),
        }
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
    } else if different {
        ("Conteúdo diferente".into(), Tone::Warning)
    } else if verified {
        ("Conferido".into(), Tone::Success)
    } else if name.is_some() {
        ("Preparando o player…".into(), Tone::Muted)
    } else if !local {
        ("Aguardando a escolha do filme".into(), Tone::Muted)
    } else {
        ("Nenhum arquivo escolhido".into(), Tone::Muted)
    };
    let warn = matches!(tone, Tone::Warning);
    ui.painter().rect(
        rect,
        14,
        colors.surface,
        Stroke::new(
            1.0,
            if warn {
                colors.warning.gamma_multiply(0.45)
            } else {
                colors.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(16.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 12.0);
        ui.horizontal(|ui| {
            ui.set_height(24.0);
            let who = if local {
                "Você"
            } else {
                view.peer_name.as_deref().unwrap_or("Outra pessoa")
            };
            let menu = if name.is_some() { MENU } else { 0.0 };
            let button = local
                .then(|| {
                    if loading && view.pending_file.is_some() {
                        Some(
                            kit::Button::new(kit::Kind::Ghost, "Cancelar verificação")
                                .size(kit::Size::Small),
                        )
                    } else if !loading && name.is_some() {
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
            kit::avatar(ui, colors, who, 22.0);
            let room = (ui.available_width() - buttons - 8.0).max(20.0);
            ui.allocate_ui_with_layout(
                egui::vec2(room, 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(strong(ui, who).size(13.5).color(colors.muted)).truncate(),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(name) = name {
                    file_menu(ui, colors, name, local);
                }
                if let Some(button) = button
                    && button.show(ui, colors).clicked()
                {
                    actions.push(if loading {
                        Command::CancelOpen
                    } else {
                        Command::Open
                    });
                }
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let icon = match tone {
                Tone::Success => colors.success,
                Tone::Warning => colors.warning,
                _ if name.is_some() => colors.accent,
                _ => colors.faint,
            };
            visuals::file(ui, icon, 42.0, true);
            if local && name.is_none() && !loading {
                let choose = kit::Button::new(kit::Kind::Primary, "Escolher arquivo")
                    .glyph(kit::Glyph::Folder)
                    .size(kit::Size::Medium);
                if choose.show(ui, colors).clicked() {
                    actions.push(Command::Open);
                }
                return;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                match name {
                    Some(name) => {
                        ui.add(egui::Label::new(strong(ui, name).size(14.0)).truncate())
                            .on_hover_text(name);
                    }
                    None => {
                        ui.label(
                            RichText::new(if is_stream_card && !local {
                                "Recebendo seu filme"
                            } else {
                                "Nenhum arquivo ainda"
                            })
                            .size(14.0)
                            .color(colors.faint),
                        );
                    }
                }
                if let Some(progress) = progress.filter(|_| loading) {
                    kit::progress(
                        ui,
                        colors,
                        progress.fraction(),
                        ui.available_width().min(220.0),
                    );
                } else {
                    kit::status(ui, tone.glyph(), &status, tone.color(colors));
                }
            });
        });
    });
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    sticky: bool,
    actions: &mut Vec<Command>,
) {
    let local = view.role == Role::Local;
    let compact = ui.ctx().content_rect().height() < 700.0;
    ui.spacing_mut().item_spacing.y = if compact { 6.0 } else { 10.0 };
    if let Some(request) = &view.join_request {
        super::room::approval(ui, colors, request, actions);
        ui.add_space(8.0);
    }
    ui.vertical_centered(|ui| {
        ui.label(title(ui, heading(view), if compact { 24.0 } else { 26.0 }));
    });
    ui.add_space(if compact { 8.0 } else { 16.0 });
    pair(ui, colors, view, actions);
    if view.role != Role::Local && view.connected {
        ui.add_space(if compact { 6.0 } else { 10.0 });
        super::stream::preparation_strip(ui, colors, view, actions);
    }
    if !sticky && view.join_request.is_none() {
        ui.add_space(if compact { 10.0 } else { 16.0 });
        action(ui, colors, view, actions);
    }
    if view.error.is_none() {
        feedback(ui, view);
    }
    if !local {
        ui.add_space(if compact { 6.0 } else { 14.0 });
        footer(ui, colors, view, actions);
    }
}

fn footer(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    ui.vertical_centered(|ui| {
        let details = kit::Button::new(kit::Kind::Ghost, "Detalhes da sala")
            .size(kit::Size::Medium)
            .show(ui, colors);
        egui::Popup::menu(&details)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_max_width(320.0);
                ui.add(
                    egui::Label::new(
                        strong(
                            ui,
                            view.peer_name
                                .as_deref()
                                .unwrap_or("Aguardando participante"),
                        )
                        .size(14.0),
                    )
                    .wrap(),
                );
                connection_details(ui, view, actions);
            });
    });
}

pub(super) fn room(ui: &mut egui::Ui, colors: Palette, view: &View, actions: &mut Vec<Command>) {
    if let Some(request) = &view.join_request {
        super::room::approval(ui, colors, request, actions);
        ui.add_space(12.0);
    }
    feedback(ui, view);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.label(
            RichText::new(if view.internet {
                "Envie este código para a outra pessoa"
            } else {
                "Envie este endereço para a outra pessoa"
            })
            .size(14.5)
            .color(colors.muted),
        );
        if view.internet {
            invite_code(ui, colors, view);
        } else {
            for address in super::room_addresses(view) {
                ui.push_id(address, |ui| {
                    invite_box(ui, colors, &address.to_string(), 17.0, "Copiar endereço")
                });
            }
        }
        if view.relay_offline {
            ui.label(
                RichText::new("Sem contato com o servidor. Tentando de novo…")
                    .size(13.0)
                    .color(colors.warning),
            );
        }
        ui.add_space(6.0);
        presence(ui, colors);
        ui.add_space(14.0);
        ui.set_max_width(ui.available_width().min(340.0));
        if view.loading {
            if let Some(progress) = view.verification.filter(|_| view.pending_file.is_some()) {
                ui.label(
                    RichText::new("Conferindo seu arquivo")
                        .size(13.0)
                        .color(colors.muted),
                );
                kit::progress(ui, colors, progress.fraction(), ui.available_width());
            } else {
                kit::status(
                    ui,
                    Some(kit::Glyph::Spinner),
                    "Escolhendo arquivo…",
                    colors.muted,
                );
            }
        } else {
            let label = if view.file.is_some() {
                "Trocar meu filme"
            } else {
                "Escolher meu filme"
            };
            if kit::Button::new(kit::Kind::Secondary, label)
                .glyph(kit::Glyph::Folder)
                .fill_width(ui)
                .show(ui, colors)
                .clicked()
            {
                actions.push(Command::Open);
            }
            if let Some(file) = &view.file {
                ui.add(
                    egui::Label::new(RichText::new(file).size(12.5).color(colors.muted))
                        .halign(egui::Align::Center)
                        .truncate(),
                )
                .on_hover_text(file);
            }
        }
    });
}

fn presence(ui: &mut egui::Ui, colors: Palette) {
    let text = "Esperando alguém entrar…";
    let width = ui
        .painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(14.0),
            colors.muted,
        )
        .size()
        .x;
    kit::centered_row(ui, &[28.0, 24.0, 28.0, width + 4.0], |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        kit::avatar(ui, colors, "Você", 28.0);
        let (line, _) = ui.allocate_exact_size(egui::vec2(24.0, 28.0), egui::Sense::hover());
        ui.painter().hline(
            line.x_range(),
            line.center().y,
            Stroke::new(2.0, colors.border_strong),
        );
        let (seat, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
        let time = ui.input(|i| i.time) as f32;
        ui.painter()
            .circle_stroke(seat.center(), 13.0, Stroke::new(1.5, colors.border_strong));
        let arc: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let a = time * 2.4 + i as f32 / 12.0 * 1.4;
                seat.center() + egui::vec2(a.cos(), a.sin()) * 13.0
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(arc, Stroke::new(1.5, colors.accent)));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
        ui.add_space(4.0);
        ui.label(RichText::new(text).size(14.0).color(colors.muted));
    });
}

fn invite_box(ui: &mut egui::Ui, colors: Palette, value: &str, size: f32, copy: &str) {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(14)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let button = kit::Button::new(kit::Kind::Primary, copy)
                    .glyph(kit::Glyph::Copy)
                    .size(kit::Size::Medium)
                    .natural_width(ui);
                ui.add_space(8.0);
                let room = (ui.available_width() - button - 16.0).max(40.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(room, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                strong(ui, value)
                                    .size(size)
                                    .extra_letter_spacing(-0.01 * size),
                            )
                            .truncate(),
                        )
                        .on_hover_text(value);
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    copy_button_with(ui, kit::Kind::Primary, kit::Size::Medium, copy, value);
                });
            });
        });
}

fn invite_code(ui: &mut egui::Ui, colors: Palette, view: &View) {
    match &view.room_code {
        Some(code) => {
            let size = if ui.available_width() < 400.0 {
                22.0
            } else {
                28.0
            };
            invite_box(ui, colors, code, size, "Copiar");
        }
        None => {
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

const MENU: f32 = 32.0;

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
        0.12,
    );
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 8, colors.hover.gamma_multiply(hover));
    }
    for offset in [-5.0, 0.0, 5.0] {
        ui.painter().circle_filled(
            rect.center() + egui::vec2(offset, 0.0),
            1.6,
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
