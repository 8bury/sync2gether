//! Preparação e comparação dos dois arquivos, sem I/O.
use super::{Palette, card, connection_details, feedback, primary, quiet, secondary, visuals};
use crate::{
    protocol::Control,
    runtime::{Command, Role, View},
};
use eframe::egui::{self, RichText, Stroke};

/// Explica a condição que impede o play, sem usar o nome como identidade.
pub(super) fn readiness(view: &View) -> (&'static str, &'static str) {
    if view.error.is_some() {
        return (
            "Precisamos resolver um problema",
            "Confira o aviso e tente escolher o arquivo novamente.",
        );
    }
    if view.preparing || view.scheduled {
        return (
            "Preparando a reprodução…",
            "Alinhando os players para começar no mesmo momento.",
        );
    }
    if view.loading {
        return if view.pending_file.is_some() {
            (
                "Verificando seu arquivo…",
                "Você pode cancelar a verificação e escolher outro arquivo.",
            )
        } else {
            (
                "Escolha seu arquivo",
                "A janela de seleção está aberta neste PC.",
            )
        };
    }
    if view.role != Role::Local && !view.connected {
        return if view.role == Role::Host {
            (
                "Aguardando companhia",
                "A outra pessoa pode encontrar sua sala na rede ou entrar pelo endereço.",
            )
        } else if view.waiting_approval {
            (
                "Aguardando aprovação",
                "O anfitrião precisa aceitar sua solicitação de entrada.",
            )
        } else {
            (
                "Conectando à sala…",
                "Confira se o anfitrião continua com a sala aberta e na mesma rede ou VPN.",
            )
        };
    }
    if view.file.is_none() {
        return (
            "Escolha sua cópia do filme",
            "Cada pessoa abre o arquivo que já tem no seu PC.",
        );
    }
    if view.role != Role::Local {
        if view.peer_loading {
            return (
                "A outra pessoa está preparando o arquivo",
                "Acompanhe o progresso no cartão da outra pessoa.",
            );
        }
        if view.peer_file.is_none() && !view.peer_ready {
            return (
                "Falta o arquivo da outra pessoa",
                "Ela precisa escolher a cópia local do mesmo filme.",
            );
        }
        if view.verified && view.peer_verified && !view.matched {
            return (
                "Os arquivos são diferentes",
                "Usem uma cópia do mesmo arquivo. O nome igual não garante conteúdo igual.",
            );
        }
    }
    if view.player.blocked || view.peer_blocked {
        return (
            "Aguardando o player",
            "Um dos players não está pronto. Se a espera continuar, escolham o arquivo novamente.",
        );
    }
    if view.ready || view.role == Role::Local && view.player.loaded {
        return if view.role == Role::Local {
            (
                "Seu filme está pronto",
                "Tudo certo para assistir neste PC.",
            )
        } else {
            (
                "Os arquivos são iguais",
                "Conteúdo confirmado. Qualquer um de vocês pode começar.",
            )
        };
    }
    if view.matched && view.player.loaded && view.peer_ready {
        return (
            "Os arquivos são iguais",
            "Aguardando a conexão estabilizar para liberar o play.",
        );
    }
    (
        "Preparando os arquivos…",
        "Aguarde a verificação e o carregamento dos players.",
    )
}

pub(super) fn action(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    explain: bool,
    actions: &mut Vec<Command>,
) {
    let enabled = view.error.is_none()
        && (view.ready
            || view.role == Role::Local
                && view.player.loaded
                && !view.loading
                && !view.player.blocked);
    let (title, detail) = readiness(view);
    if explain {
        let tone = if view.error.is_some() {
            colors.error
        } else if view.ready {
            colors.success
        } else if view.verified && view.peer_verified && !view.matched && view.connected {
            colors.warning
        } else {
            colors.text
        };
        ui.label(RichText::new(title).strong().color(tone));
        ui.add(egui::Label::new(RichText::new(detail).small().color(colors.muted)).wrap());
    }
    if view.preparing || view.scheduled {
        if secondary(ui, colors, "Cancelar início", true).clicked() {
            actions.push(Command::Control(Control::Pause));
        }
    } else if primary(ui, colors, "Começar a assistir", enabled).clicked() {
        actions.push(Command::Control(Control::Play));
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
    let id = ui.id().with(("copied", value));
    let now = ui.input(|i| i.time);
    let copied = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(id))
        .is_some_and(|until| until > now);
    if ui
        .button(RichText::new(if copied { "Copiado!" } else { label }).small())
        .clicked()
    {
        ui.ctx().copy_text(value.to_owned());
        ui.ctx().data_mut(|d| d.insert_temp(id, now + 2.0));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(2));
    }
}

fn file_card(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    local: bool,
    compact: bool,
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
    let progress = if local {
        view.verification
    } else {
        view.peer_verification
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
    card(colors)
        .inner_margin(if compact { 14 } else { 18 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = if compact { 6.0 } else { 10.0 };
            ui.spacing_mut().button_padding.y = if compact { 6.0 } else { 11.0 };
            ui.set_min_height(if compact {
                if view.loading || view.peer_loading {
                    228.0
                } else {
                    184.0
                }
            } else if view.loading || view.peer_loading {
                266.0
            } else {
                232.0
            });
            ui.horizontal(|ui| {
                if !compact {
                    visuals::film(ui, colors.accent);
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(if local { "Você" } else { "Outra pessoa" }).strong());
                    ui.add(
                        egui::Label::new(
                            RichText::new(if local {
                                "Arquivo neste PC"
                            } else {
                                view.peer_name.as_deref().unwrap_or("Aguardando companhia")
                            })
                            .small()
                            .color(colors.muted),
                        )
                        .truncate(),
                    );
                });
            });
            ui.add_space(if compact { 2.0 } else { 8.0 });
            if let Some(name) = name {
                ui.add(egui::Label::new(RichText::new(name).size(17.0).strong()).truncate())
                    .on_hover_text(name);
                ui.horizontal(|ui| {
                    ui.menu_button(RichText::new("Nome completo").small(), |ui| {
                        ui.set_max_width(320.0);
                        ui.add(egui::Label::new(name).wrap().selectable(true));
                        copy_button(ui, "Copiar nome", name);
                    });
                    copy_button(ui, "Copiar nome", name);
                });
            } else {
                ui.label(RichText::new("Nenhum arquivo escolhido").color(colors.muted));
                ui.add_space(10.0);
            }
            ui.add_space(6.0);
            let status = if !local && !view.connected {
                "Ainda não entrou na sala"
            } else if loading && name.is_none() {
                "Escolhendo arquivo…"
            } else if loading {
                "Verificando conteúdo…"
            } else if blocked {
                "Player indisponível"
            } else if verified {
                "Conteúdo verificado"
            } else if name.is_some() {
                "Preparando o player…"
            } else if local {
                "Selecione sua cópia local."
            } else {
                "Aguardando a escolha do filme."
            };
            ui.label(
                RichText::new(status)
                    .small()
                    .color(if verified && !loading && !blocked {
                        colors.success
                    } else {
                        colors.muted
                    }),
            );
            if let Some(progress) = progress.filter(|_| loading) {
                ui.add(
                    egui::ProgressBar::new(progress.fraction())
                        .show_percentage()
                        .desired_width(ui.available_width()),
                );
            }
            if local {
                ui.add_space(8.0);
                if loading {
                    if view.pending_file.is_some()
                        && quiet(ui, colors, "Cancelar verificação").clicked()
                    {
                        actions.push(Command::CancelOpen);
                    }
                } else if ui
                    .add_sized(
                        [ui.available_width(), if compact { 32.0 } else { 48.0 }],
                        egui::Button::new(if name.is_some() {
                            "Trocar arquivo"
                        } else {
                            "Escolher arquivo"
                        }),
                    )
                    .clicked()
                {
                    actions.push(Command::Open);
                }
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
    ui.spacing_mut().item_spacing.y = if sticky { 6.0 } else { 10.0 };
    if !sticky {
        ui.label(
            RichText::new(if local { "SEU CINEMA" } else { "SALA A DOIS" })
                .size(11.0)
                .extra_letter_spacing(1.5)
                .color(colors.accent),
        );
    }
    ui.label(
        RichText::new(if local {
            "Escolha seu filme."
        } else {
            "Escolham o filme."
        })
        .size(30.0)
        .strong(),
    );
    ui.add(
        egui::Label::new(
            RichText::new(if local {
                "Abra o arquivo que você quer assistir neste PC."
            } else {
                "Uma cópia em cada PC. A gente confere se o conteúdo é igual."
            })
            .color(colors.muted),
        )
        .wrap(),
    );
    ui.add_space(if sticky { 6.0 } else { 14.0 });

    if view.error.is_some() {
        feedback(ui, view);
    }

    if let Some(request) = &view.join_request {
        card(colors)
            .inner_margin(18)
            .stroke(Stroke::new(1.0, colors.accent))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(format!("{} quer entrar na sala", request.name)).strong());
                ui.label(
                    RichText::new("Aceite para escolherem o filme juntos.")
                        .small()
                        .color(colors.muted),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Aceitar").color(colors.background).strong(),
                            )
                            .fill(colors.text),
                        )
                        .clicked()
                    {
                        actions.push(Command::Approve {
                            id: request.id,
                            accept: true,
                        });
                    }
                    if quiet(ui, colors, "Recusar").clicked() {
                        actions.push(Command::Approve {
                            id: request.id,
                            accept: false,
                        });
                    }
                });
            });
        ui.add_space(12.0);
    }
    if view.role == Role::Guest && !view.connected {
        let (title, detail) = readiness(view);
        card(colors).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(RichText::new(title).strong());
            });
            ui.add(egui::Label::new(detail).wrap());
        });
    } else {
        if !local && ui.available_width() >= 580.0 {
            ui.columns(2, |columns| {
                columns[0].push_id("local_file", |ui| {
                    file_card(ui, colors, view, true, sticky, actions)
                });
                columns[1].push_id("peer_file", |ui| {
                    file_card(ui, colors, view, false, sticky, actions)
                });
            });
        } else {
            ui.push_id("local_file", |ui| {
                file_card(ui, colors, view, true, sticky, actions)
            });
            if !local {
                ui.add_space(8.0);
                ui.push_id("peer_file", |ui| {
                    file_card(ui, colors, view, false, sticky, actions)
                });
            }
        }
        if !sticky {
            ui.add_space(14.0);
            let mismatch = view.verified
                && view.peer_verified
                && !view.matched
                && view.connected
                && !view.loading
                && !view.peer_loading;
            let tone = if mismatch {
                colors.warning
            } else if view.ready {
                colors.success
            } else {
                colors.accent
            };
            let (title, detail) = readiness(view);
            card(colors)
                .inner_margin(16)
                .stroke(Stroke::new(1.0, tone.gamma_multiply(0.5)))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(title).strong().color(tone));
                    ui.add(
                        egui::Label::new(RichText::new(detail).small().color(colors.muted)).wrap(),
                    );
                });
            if !local {
                ui.add(egui::Label::new(RichText::new("Os nomes ajudam a comparar. A confirmação usa o conteúdo completo dos arquivos.").small().color(colors.muted)).wrap());
            }
        }
        if !sticky && view.join_request.is_none() {
            ui.add_space(16.0);
            action(ui, colors, view, false, actions);
        }
    }
    ui.add_space(12.0);
    if view.error.is_none() {
        feedback(ui, view);
    }
    if view.role == Role::Host && !view.connected {
        invite(ui, colors, view);
    }
    if !local {
        connection_details(ui, view, actions);
    }
    ui.add_space(8.0);
    let pending = view.role == Role::Guest && !view.connected;
    if quiet(
        ui,
        colors,
        if pending {
            "Cancelar solicitação"
        } else {
            leave_label(view.role)
        },
    )
    .clicked()
    {
        actions.push(Command::Leave);
    }
    if view.role == Role::Host {
        ui.label(
            RichText::new("Encerrar a sala desconecta a outra pessoa.")
                .small()
                .color(colors.muted),
        );
    }
}

fn invite(ui: &mut egui::Ui, colors: Palette, view: &View) {
    egui::CollapsingHeader::new("Convidar por endereço").default_open(true).show(ui, |ui| {
        ui.add(egui::Label::new("A sala não apareceu? Envie um endereço da rede ou VPN que vocês compartilham. A outra pessoa usa “Conectar por endereço”.").wrap());
        for address in super::room_addresses(view) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(address.to_string()).monospace().color(colors.muted));
                copy_button(ui, "Copiar endereço", &address.to_string());
            });
        }
    });
}
