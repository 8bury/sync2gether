//! Preparação e comparação dos dois arquivos, sem I/O.
use super::{Palette, card, connection_details, feedback, kit, secondary, strong, title, visuals};
use crate::{
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, RichText, Stroke};

/// Explica a condição que impede o play, sem usar o nome como identidade.
pub(super) fn readiness(view: &View) -> (&'static str, &'static str) {
    if view.error.is_some() {
        return ("Não foi possível preparar o filme", "");
    }
    if view.preparing || view.scheduled {
        return ("Preparando a reprodução…", "");
    }
    if view.loading {
        return if view.pending_file.is_some() {
            ("Verificando seu arquivo…", "")
        } else {
            (
                "Escolha seu arquivo",
                "A janela de seleção está aberta neste PC.",
            )
        };
    }
    if view.role != Role::Local && !view.connected {
        return if view.role == Role::Host {
            ("Aguardando participante", "")
        } else if view.waiting_approval {
            ("Aguardando aprovação", "")
        } else {
            ("Conectando à sala…", "")
        };
    }
    if view.file.is_none() {
        return ("Escolha sua cópia do filme", "");
    }
    if view.role != Role::Local {
        if view.peer_loading {
            return ("A outra pessoa está preparando o arquivo", "");
        }
        if view.peer_file.is_none() && !view.peer_ready {
            return ("Falta o arquivo da outra pessoa", "");
        }
        if view.verified && view.peer_verified && !view.matched {
            return (
                "Os arquivos são diferentes",
                "Escolham arquivos com o mesmo conteúdo.",
            );
        }
    }
    if view.player.blocked || view.peer_blocked {
        return (
            "Aguardando o player",
            "Se a espera continuar, selecione o arquivo novamente.",
        );
    }
    if view.ready || view.role == Role::Local && view.player.loaded {
        return if view.role == Role::Local {
            ("Seu filme está pronto", "")
        } else {
            ("Os arquivos são iguais", "")
        };
    }
    if view.matched && view.player.loaded && view.peer_ready {
        return (
            "Os arquivos são iguais",
            "Aguardando a conexão estabilizar para liberar o play.",
        );
    }
    ("Preparando os arquivos…", "")
}

fn start_enabled(view: &View) -> bool {
    view.error.is_none()
        && (view.ready
            || view.role == Role::Local
                && view.player.loaded
                && !view.loading
                && !view.player.blocked)
}

pub(super) fn action(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    explain: bool,
    actions: &mut Vec<Command>,
) {
    let enabled = start_enabled(view);
    let (title, detail) = readiness(view);
    if explain {
        let (tone, glyph) = if view.error.is_some() {
            (colors.error, Some(kit::Glyph::Alert))
        } else if enabled {
            (colors.success, Some(kit::Glyph::Check))
        } else if view.verified && view.peer_verified && !view.matched && view.connected {
            (colors.warning, Some(kit::Glyph::Alert))
        } else if view.preparing || view.scheduled || view.loading || view.peer_loading {
            (colors.muted, Some(kit::Glyph::Spinner))
        } else {
            (colors.muted, None)
        };
        // A família é obtida antes: consultar as fontes dentro de `fonts_mut` travaria o contexto.
        let font = egui::FontId::new(14.5, super::semibold_family(ui.ctx()));
        let galley_width = ui
            .fonts_mut(|f| f.layout_no_wrap(title.to_owned(), font, tone))
            .size()
            .x;
        kit::centered_row(ui, &[16.0, galley_width], |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
            match glyph {
                Some(glyph) => {
                    kit::draw(
                        ui.painter(),
                        glyph,
                        rect.center(),
                        15.0,
                        tone,
                        ui.input(|i| i.time),
                    );
                    if glyph == kit::Glyph::Spinner {
                        ui.ctx().request_repaint();
                    }
                }
                None => {
                    ui.painter().circle_filled(rect.center(), 3.5, tone);
                }
            }
            ui.add(egui::Label::new(strong(ui, title).size(14.5).color(tone)).wrap());
        });
        ui.add_space(2.0);
    }
    ui.vertical_centered(|ui| {
        ui.set_max_width(ui.available_width().min(320.0));
        if view.preparing || view.scheduled {
            if secondary(ui, colors, "Cancelar início", true).clicked() {
                actions.push(Command::Control(Control::Pause));
            }
        } else {
            let response = kit::Button::new(
                if enabled {
                    kit::Kind::Primary
                } else {
                    kit::Kind::Secondary
                },
                "Começar a assistir",
            )
            .glyph(kit::Glyph::Play)
            .enabled(enabled)
            .fill_width(ui)
            .show(ui, colors);
            if response.clicked() {
                actions.push(Command::Control(Control::Play));
            }
        }
    });
    if explain && !detail.is_empty() {
        ui.add(egui::Label::new(RichText::new(detail).small().color(colors.muted)).wrap());
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
    let colors = Palette::new(ui.visuals().dark_mode);
    let id = ui.id().with(("copied", value));
    let now = ui.input(|i| i.time);
    let copied = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|until| until > now);
    if kit::Button::new(
        kit::Kind::Secondary,
        if copied { "Copiado!" } else { label },
    )
    .glyph(if copied {
        kit::Glyph::Check
    } else {
        kit::Glyph::Copy
    })
    .size(kit::Size::Small)
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

fn file_row(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    local: bool,
    actions: &mut Vec<Command>,
) {
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
    let verified = if local {
        view.verified
    } else {
        view.peer_verified
    };
    let blocked = if local {
        view.player.blocked
    } else {
        view.peer_blocked
    };
    let progress = if local {
        view.verification
    } else {
        view.peer_verification
    };
    let verified = verified && !loading && !blocked && (local || view.connected);
    let status = if !local && !view.connected {
        "Ainda não entrou na sala"
    } else if loading && name.is_none() {
        "Escolhendo arquivo…"
    } else if loading {
        "Verificando conteúdo…"
    } else if blocked {
        "Player indisponível"
    } else if verified {
        "Arquivo verificado"
    } else if name.is_some() {
        "Preparando o player…"
    } else if !local {
        "Aguardando a escolha do filme"
    } else {
        "Nenhum arquivo escolhido"
    };
    let has_action = local && (!loading || view.pending_file.is_some());
    let button = has_action.then(|| {
        if loading {
            kit::Button::new(kit::Kind::Ghost, "Cancelar verificação").size(kit::Size::Small)
        } else if name.is_none() {
            kit::Button::new(kit::Kind::Primary, "Escolher arquivo").size(kit::Size::Medium)
        } else {
            kit::Button::new(kit::Kind::Secondary, "Trocar arquivo").size(kit::Size::Small)
        }
    });
    let gap = ui.spacing().item_spacing.x;
    // O menu ⋯ ocupa sempre a mesma coluna nas duas linhas.
    let menu_slot = if name.is_some() { MENU } else { 0.0 };
    let action_width = button
        .as_ref()
        .map_or(0.0, |button| button.natural_width(ui) + gap)
        + menu_slot;
    let tone = if verified {
        colors.success
    } else if blocked {
        colors.warning
    } else if loading {
        colors.accent
    } else {
        colors.muted
    };
    ui.horizontal(|ui| {
        let icon_color = if verified {
            colors.success
        } else if blocked {
            colors.warning
        } else if name.is_some() || loading {
            colors.accent
        } else {
            colors.faint
        };
        visuals::file(ui, icon_color, 44.0, true);
        let text_width =
            (ui.available_width() - action_width - ui.spacing().item_spacing.x).max(1.0);
        ui.allocate_ui_with_layout(
            egui::vec2(text_width, 64.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_width(text_width);
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(
                    egui::Label::new(
                        strong(
                            ui,
                            if local {
                                "Você"
                            } else {
                                view.peer_name.as_deref().unwrap_or("Outra pessoa")
                            },
                        )
                        .size(15.0),
                    )
                    .truncate(),
                );
                if let Some(name) = name {
                    ui.add(egui::Label::new(RichText::new(name).color(colors.muted)).truncate())
                        .on_hover_text(name);
                }
                if let Some(progress) = progress.filter(|_| loading) {
                    kit::progress(ui, colors, progress.fraction(), text_width.min(360.0));
                    ui.add(
                        egui::Label::new(RichText::new(status).size(12.5).color(colors.muted))
                            .truncate(),
                    );
                } else {
                    kit::status(
                        ui,
                        if verified {
                            Some(kit::Glyph::Check)
                        } else if blocked {
                            Some(kit::Glyph::Alert)
                        } else if loading {
                            Some(kit::Glyph::Spinner)
                        } else {
                            None
                        },
                        status,
                        tone,
                    );
                }
            },
        );
        if action_width > 0.0 {
            ui.allocate_ui_with_layout(
                egui::vec2(action_width, 48.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_min_width(action_width);
                    ui.set_max_width(action_width);
                    ui.spacing_mut().item_spacing.x = gap;
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
                },
            );
        }
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
    ui.vertical_centered(|ui| {
        if !compact && view.join_request.is_none() {
            visuals::file(ui, colors.faint, 56.0, false);
            ui.add_space(6.0);
        }
        ui.label(title(
            ui,
            if start_enabled(view) {
                "Pronto para assistir"
            } else {
                "Selecionar filme"
            },
            if compact { 26.0 } else { 30.0 },
        ));
    });
    ui.add_space(if compact { 10.0 } else { 22.0 });
    if view.error.is_some() {
        feedback(ui, view);
        ui.add_space(4.0);
    }
    if let Some(request) = &view.join_request {
        approval(ui, colors, request, actions);
        ui.add_space(8.0);
    }
    // Enquanto ninguém entrou, o convite é o que o anfitrião precisa ver primeiro.
    if view.role == Role::Host && !view.connected {
        invite(ui, colors, view);
        ui.add_space(if compact { 6.0 } else { 12.0 });
    }
    card(colors)
        .inner_margin(egui::Margin::symmetric(
            if compact { 14 } else { 18 },
            if compact { 10 } else { 14 },
        ))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.push_id("local_file", |ui| file_row(ui, colors, view, true, actions));
            if !local {
                ui.add_space(if compact { 4.0 } else { 8.0 });
                let y = ui.cursor().top();
                ui.painter()
                    .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, colors.border));
                ui.add_space(if compact { 5.0 } else { 9.0 });
                ui.push_id("peer_file", |ui| file_row(ui, colors, view, false, actions));
            }
        });
    if !sticky && view.join_request.is_none() {
        ui.add_space(if compact { 14.0 } else { 22.0 });
        ui.vertical_centered(|ui| action(ui, colors, view, true, actions));
    }
    if view.error.is_none() {
        feedback(ui, view);
    }
    ui.add_space(if compact { 6.0 } else { 18.0 });
    let leave = leave_label(view.role);
    let ghost = |label| kit::Button::new(kit::Kind::Ghost, label).size(kit::Size::Medium);
    let mut widths = vec![ghost(leave).natural_width(ui)];
    if !local {
        widths.insert(0, ghost("Detalhes da sala").natural_width(ui));
    }
    kit::centered_row(ui, &widths, |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if !local {
            let details = ghost("Detalhes da sala").show(ui, colors);
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
            let y = ui.max_rect().center().y;
            let x = ui.cursor().left();
            ui.painter()
                .circle_filled(egui::pos2(x - 6.0, y), 1.5, colors.faint);
        }
        if ghost(leave).show(ui, colors).clicked() {
            actions.push(Command::Leave);
        }
    });
}

fn approval(
    ui: &mut egui::Ui,
    colors: Palette,
    request: &crate::runtime::JoinRequest,
    actions: &mut Vec<Command>,
) {
    egui::Frame::new()
        .fill(super::kit::lerp(colors.surface, colors.accent, 0.08))
        .stroke(Stroke::new(1.0, colors.accent.gamma_multiply(0.55)))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let wide = ui.available_width() >= 460.0;
            let accept = || kit::Button::new(kit::Kind::Primary, "Aceitar").size(kit::Size::Small);
            let reject =
                || kit::Button::new(kit::Kind::Secondary, "Recusar").size(kit::Size::Small);
            let buttons = accept().natural_width(ui) + reject().natural_width(ui) + 8.0;
            let mut answer = None;
            ui.horizontal(|ui| {
                kit::avatar(ui, colors, &request.name, 36.0);
                let text_width = (ui.available_width()
                    - if wide {
                        buttons + ui.spacing().item_spacing.x
                    } else {
                        0.0
                    })
                .max(1.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(text_width, 36.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(text_width);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add(
                            egui::Label::new(strong(
                                ui,
                                format!("{} quer entrar na sala", request.name),
                            ))
                            .wrap(),
                        );
                        ui.label(
                            RichText::new(request.address.ip().to_string())
                                .small()
                                .color(colors.muted),
                        );
                    },
                );
                if wide {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if accept().show(ui, colors).clicked() {
                        answer = Some(true);
                    }
                    if reject().show(ui, colors).clicked() {
                        answer = Some(false);
                    }
                }
            });
            if !wide {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_space(44.0);
                    if accept().show(ui, colors).clicked() {
                        answer = Some(true);
                    }
                    if reject().show(ui, colors).clicked() {
                        answer = Some(false);
                    }
                });
            }
            if let Some(accept) = answer {
                actions.push(Command::Approve {
                    id: request.id,
                    accept,
                });
            }
        });
}

fn invite(ui: &mut egui::Ui, colors: Palette, view: &View) {
    if view.internet {
        return invite_code(ui, colors, view);
    }
    card(colors)
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                kit::draw(
                    ui.painter(),
                    kit::Glyph::People,
                    rect.center(),
                    16.0,
                    colors.accent,
                    0.0,
                );
                ui.label(strong(ui, "Convidar por endereço"));
            });
            for address in super::room_addresses(view) {
                ui.horizontal_wrapped(|ui| {
                    egui::Frame::new()
                        .fill(colors.inset)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(address.to_string())
                                    .monospace()
                                    .size(13.0)
                                    .color(colors.text),
                            );
                        });
                    copy_button(ui, "Copiar endereço", &address.to_string());
                });
            }
        });
}

/// Convite pela internet: o código é tudo que a outra pessoa precisa.
fn invite_code(ui: &mut egui::Ui, colors: Palette, view: &View) {
    card(colors)
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                kit::draw(
                    ui.painter(),
                    kit::Glyph::People,
                    rect.center(),
                    16.0,
                    colors.accent,
                    0.0,
                );
                ui.label(strong(ui, "Convidar com código"));
            });
            match &view.room_code {
                Some(code) => {
                    ui.horizontal_wrapped(|ui| {
                        egui::Frame::new()
                            .fill(colors.inset)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(code.as_str())
                                        .monospace()
                                        .size(18.0)
                                        .color(colors.text),
                                );
                            });
                        copy_button(ui, "Copiar código", code);
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Envie o código para a outra pessoa. Ela entra com ele em \"Entrar em uma sala\".",
                            )
                            .small()
                            .color(colors.muted),
                        )
                        .wrap(),
                    );
                }
                None => {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        kit::draw(
                            ui.painter(),
                            kit::Glyph::Spinner,
                            rect.center(),
                            16.0,
                            colors.accent,
                            ui.input(|i| i.time),
                        );
                        ui.ctx().request_repaint();
                        ui.label(RichText::new("Gerando o código…").color(colors.muted));
                    });
                }
            }
            if view.relay_offline {
                ui.add(
                    egui::Label::new(
                        RichText::new("Sem contato com o servidor. Tentando de novo…")
                            .small()
                            .color(colors.warning),
                    )
                    .wrap(),
                );
            }
        });
}
