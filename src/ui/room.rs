use super::{
    Inputs, Palette, feedback, kit, preparation, quiet, section_label, strong, title, tr,
    visuals::{self, Presence},
};
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
    let strings = super::language(ctx).strings();
    let t = &strings.room;
    let host = view.role == Role::Host;
    let peer = view
        .peer_name
        .as_deref()
        .unwrap_or(strings.common.other_person_object);
    let modal = egui::Modal::new(egui::Id::new("confirm_leave"))
        .frame(
            egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(10)
                .inner_margin(24),
        )
        .show(ctx, |ui| {
            ui.set_width(ui.available_width().min(340.0));
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(title(
                if host {
                    t.end_question
                } else {
                    t.leave_question
                },
                20.0,
            ));
            ui.add(
                egui::Label::new(
                    RichText::new(if host {
                        (t.end_detail)(peer)
                    } else {
                        (t.leave_detail)(peer)
                    })
                    .color(colors.muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            let leave = if host { t.end } else { t.leave };
            let mut stay = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::Button::new(kit::Kind::Primary, t.stay)
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
    let strings = tr(ui);
    let t = &strings.room;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(title(t.title, 22.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, colors, kit::Glyph::Close, t.close, 32.0).clicked() {
                    inputs.room_panel = false;
                }
            });
        });
        ui.add_space(14.0);
        let name = view
            .peer_name
            .as_deref()
            .unwrap_or(if view.role == Role::Local {
                t.watching_alone
            } else {
                strings.common.other_person
            });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            visuals::pair(
                ui,
                colors,
                if view.connected {
                    Presence::Together
                } else if view.role == Role::Local || view.peer_left {
                    Presence::Solo
                } else {
                    Presence::Lost
                },
                18.0,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(strong(ui, name)).truncate());
                let (label, color) = if view.connected {
                    (t.connected, colors.success)
                } else if view.role == Role::Local {
                    (strings.common.local_playback, colors.muted)
                } else if view.peer_left {
                    (t.left_room, colors.muted)
                } else {
                    (t.disconnected, colors.warning)
                };
                kit::status(ui, None, label, color);
            });
        });
        ui.add_space(18.0);
        section_label(ui, colors, t.your_file);
        ui.add(egui::Label::new(view.file.as_deref().unwrap_or(t.no_file)).truncate())
            .on_hover_text(view.file.as_deref().unwrap_or_default());
        if view.role != Role::Local {
            ui.add_space(4.0);
            section_label(ui, colors, t.peer_file);
            ui.add(
                egui::Label::new(view.peer_file.as_deref().unwrap_or(t.waiting_file)).truncate(),
            )
            .on_hover_text(view.peer_file.as_deref().unwrap_or_default());
        }
        ui.add_space(12.0);
        if kit::Button::new(kit::Kind::Secondary, t.change_movie)
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
        super::stream::room_section(ui, colors, view, actions);
        feedback(ui, view);
        connection_details(ui, view, actions);
        ui.add_space(12.0);
        if quiet(ui, colors, preparation::leave_label(strings, view.role)).clicked() {
            request_leave(view, inputs, actions);
        }
    });
}

pub(crate) fn preparation_leave_label(
    t: &'static crate::i18n::Strings,
    role: Role,
) -> &'static str {
    preparation::leave_label(t, role)
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
    let strings = tr(ui);
    let t = &strings.room;
    egui::CollapsingHeader::new(t.connection_details).show(ui, |ui| {
        if view.internet {
            ui.label(t.internet_room);
            if let Some(code) = &view.room_code {
                ui.label((t.code)(code));
                if view.role == Role::Host {
                    preparation::copy_button(ui, strings.common.copy_code, code);
                }
                if let Some(link) = &view.web_link {
                    preparation::copy_button(ui, strings.preparation.copy_browser_link, link);
                }
            }
            if view.relay_offline {
                ui.label(strings.common.relay_offline);
            }
        }
        let addresses = if view.internet {
            Vec::new()
        } else {
            room_addresses(view)
        };
        if let Some(address) = view.room_address.filter(|addr| addr.ip().is_unspecified()) {
            ui.label((t.port)(address.port()));
        }
        for address in addresses {
            ui.label((t.address)(&address.to_string()));
            preparation::copy_button(ui, strings.common.copy_address, &address.to_string());
        }
        if let Some(error) = &view.discovery_error {
            ui.label(error);
        }
        ui.label(&view.status);
        if let Some(drift) = view.peer_drift {
            ui.label((t.drift)(drift.abs() * 1000.0));
        }
        if view.connected && view.role == Role::Guest {
            ui.label((t.rtt)(view.rtt_ms));
        }
        if view.role == Role::Local && ui.button(strings.common.refresh_rooms).clicked() {
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

pub(super) fn approval(
    ui: &mut egui::Ui,
    colors: Palette,
    request: &crate::runtime::JoinRequest,
    actions: &mut Vec<Command>,
) {
    egui::Frame::new()
        .fill(colors.raised)
        .stroke(Stroke::new(1.0, colors.border_strong))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let t = &tr(ui).room;
            let wide = ui.available_width() >= 460.0;
            let accept = || kit::Button::new(kit::Kind::Primary, t.accept).size(kit::Size::Small);
            let reject = || kit::Button::new(kit::Kind::Secondary, t.reject).size(kit::Size::Small);
            let buttons = accept().natural_width(ui) + reject().natural_width(ui) + 8.0;
            let mut answer = None;
            ui.horizontal(|ui| {
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
                            egui::Label::new(strong(ui, (t.wants_to_join)(&request.name))).wrap(),
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
