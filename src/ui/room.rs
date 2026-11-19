use super::{Inputs, Palette, feedback, kit, preparation, quiet, section_label, strong, title};
use crate::runtime::{Command, Role, View};
use eframe::egui::{self, RichText, Stroke};

pub(crate) fn request_leave(view: &View, inputs: &mut Inputs, actions: &mut Vec<Command>) {
    if view.role != Role::Local && view.connected {
        inputs.confirm_leave = true;
    } else {
        inputs.player.fullscreen = false;
        inputs.room_panel = false;
        actions.push(Command::Leave);
    }
}

pub(super) fn confirm_leave(
    ctx: &egui::Context,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    if view.role == Role::Local || !view.connected {
        inputs.confirm_leave = false;
        return;
    }
    let host = view.role == Role::Host;
    let peer = view.peer_name.as_deref().unwrap_or("A outra pessoa");
    let modal = egui::Modal::new(egui::Id::new("confirm_leave"))
        .frame(
            egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(16)
                .inner_margin(24),
        )
        .show(ctx, |ui| {
            ui.set_width(ui.available_width().min(340.0));
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(title(
                ui,
                if host {
                    "Encerrar a sala?"
                } else {
                    "Sair da sala?"
                },
                20.0,
            ));
            ui.add(
                egui::Label::new(
                    RichText::new(if host {
                        format!("A sala fecha e a conexão com {peer} termina.")
                    } else {
                        format!("Você deixa a sala de {peer} e volta ao início.")
                    })
                    .color(colors.muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            let leave = if host { "Encerrar" } else { "Sair" };
            let mut stay = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::Button::new(kit::Kind::Primary, "Continuar na sala")
                        .size(kit::Size::Medium)
                        .show(ui, colors)
                        .clicked()
                    {
                        stay = true;
                    }
                    if kit::Button::new(kit::Kind::Secondary, leave)
                        .size(kit::Size::Medium)
                        .show(ui, colors)
                        .clicked()
                    {
                        inputs.confirm_leave = false;
                        inputs.player.fullscreen = false;
                        inputs.room_panel = false;
                        actions.push(Command::Leave);
                    }
                });
            });
            stay
        });
    if modal.inner || modal.should_close() {
        inputs.confirm_leave = false;
    }
}

pub(super) fn room_contents(
    ui: &mut egui::Ui,
    colors: Palette,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Command>,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(title(ui, "Sala", 20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, colors, kit::Glyph::Close, "Fechar", 32.0).clicked() {
                    inputs.room_panel = false;
                }
            });
        });
        ui.add_space(14.0);
        let name = view
            .peer_name
            .as_deref()
            .unwrap_or(if view.role == Role::Local {
                "Assistindo sozinho"
            } else {
                "Outra pessoa"
            });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            kit::avatar(ui, colors, name, 36.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(strong(ui, name)).truncate());
                let (label, color) = if view.connected {
                    ("Conectado", colors.success)
                } else if view.role == Role::Local {
                    ("Reprodução local", colors.muted)
                } else {
                    ("Desconectado", colors.warning)
                };
                kit::status(ui, None, label, color);
            });
        });
        ui.add_space(18.0);
        section_label(ui, colors, "Seu arquivo");
        ui.add(egui::Label::new(view.file.as_deref().unwrap_or("Nenhum arquivo")).truncate())
            .on_hover_text(view.file.as_deref().unwrap_or_default());
        if view.role != Role::Local {
            ui.add_space(4.0);
            section_label(ui, colors, "Arquivo da outra pessoa");
            ui.add(
                egui::Label::new(view.peer_file.as_deref().unwrap_or("Aguardando arquivo"))
                    .truncate(),
            )
            .on_hover_text(view.peer_file.as_deref().unwrap_or_default());
        }
        ui.add_space(12.0);
        if kit::Button::new(kit::Kind::Secondary, "Trocar filme")
            .size(kit::Size::Medium)
            .fill_width(ui)
            .show(ui, colors)
            .clicked()
        {
            inputs.room_panel = false;
            inputs.player.fullscreen = false;
            actions.push(Command::Open);
        }
        ui.add_space(12.0);
        feedback(ui, view);
        connection_details(ui, view, actions);
        ui.add_space(12.0);
        if quiet(ui, colors, preparation::leave_label(view.role)).clicked() {
            request_leave(view, inputs, actions);
        }
    });
}

pub(crate) fn preparation_leave_label(role: Role) -> &'static str {
    preparation::leave_label(role)
}

pub(crate) fn media_title(file: &str) -> &str {
    if let Some((name, extension)) = file.rsplit_once('.')
        && [
            "mkv", "mp4", "avi", "mov", "webm", "m4v", "mpeg", "mpg", "ts",
        ]
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
        && !name.is_empty()
    {
        name
    } else {
        file
    }
}

pub(super) fn connection_details(ui: &mut egui::Ui, view: &View, actions: &mut Vec<Command>) {
    egui::CollapsingHeader::new("Detalhes da conexão").show(ui, |ui| {
        if view.internet {
            ui.label("Sala pela internet, cifrada de ponta a ponta");
            if let Some(code) = &view.room_code {
                ui.label(format!("Código: {code}"));
                if view.role == Role::Host {
                    preparation::copy_button(ui, "Copiar código", code);
                }
            }
            if view.relay_offline {
                ui.label("Sem contato com o servidor. Tentando de novo…");
            }
        }
        let addresses = if view.internet {
            Vec::new()
        } else {
            room_addresses(view)
        };
        if let Some(address) = view.room_address.filter(|addr| addr.ip().is_unspecified()) {
            ui.label(format!("Porta: {}", address.port()));
        }
        for address in addresses {
            ui.label(format!("Endereço: {address}"));
            preparation::copy_button(ui, "Copiar endereço", &address.to_string());
        }
        if let Some(error) = &view.discovery_error {
            ui.label(error);
        }
        ui.label(&view.status);
        if let Some(drift) = view.peer_drift {
            ui.label(format!(
                "Diferença estimada entre players: {:.0} ms",
                drift.abs() * 1000.0
            ));
        }
        if view.connected && view.role == Role::Guest {
            ui.label(format!("Ida e volta: {:.0} ms", view.rtt_ms));
        }
        if view.role == Role::Local && ui.button("Atualizar salas").clicked() {
            actions.push(Command::Discover);
        }
    });
}

pub(super) fn room_addresses(view: &View) -> Vec<std::net::SocketAddr> {
    if view.room_addresses.is_empty() {
        view.room_address
            .into_iter()
            .filter(|addr| !addr.ip().is_unspecified())
            .collect()
    } else {
        view.room_addresses.clone()
    }
}
