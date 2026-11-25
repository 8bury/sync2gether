use super::{Inputs, Palette, Screen, draw_flow, strong};
use crate::model::{Action, SessionState};
use eframe::egui::{self, RichText, Stroke};

pub fn draw(ui: &mut egui::Ui, state: SessionState, demo: bool) -> Vec<Action> {
    draw_with_inputs(ui, state, demo, &mut Inputs::default())
}

pub fn draw_with_inputs(
    ui: &mut egui::Ui,
    state: SessionState,
    demo: bool,
    inputs: &mut Inputs,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if demo {
        let colors = Palette::new(ui.visuals().dark_mode);
        egui::Panel::top("demo_controls")
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(16, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    egui::Frame::new()
                        .fill(colors.warning.gamma_multiply(0.14))
                        .stroke(Stroke::new(1.0, colors.warning.gamma_multiply(0.4)))
                        .corner_radius(4)
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.label(
                                strong(ui, "DEMO OFFLINE")
                                    .size(10.5)
                                    .extra_letter_spacing(0.6)
                                    .color(colors.warning),
                            );
                        });
                    ui.menu_button(
                        RichText::new(state.label()).size(13.5).color(colors.muted),
                        |ui| {
                            ui.label(
                                RichText::new("Cenários fictícios, sem player ou rede")
                                    .small()
                                    .color(colors.muted),
                            );
                            ui.separator();
                            for candidate in SessionState::ALL {
                                if ui
                                    .selectable_label(state == candidate, candidate.label())
                                    .clicked()
                                {
                                    inputs.screen = Screen::Home;
                                    inputs.joining = false;
                                    inputs.room_panel = false;
                                    actions.push(Action::SetDemoState(candidate));
                                    ui.close();
                                }
                            }
                        },
                    );
                });
            });
    }
    let state = actions
        .last()
        .map_or(state, |Action::SetDemoState(next)| *next);
    let view = demo_view(state);
    if state == SessionState::Browsing {
        inputs.screen = Screen::Browse;
        inputs.lan = true;
    }
    if matches!(state, SessionState::JoinCode | SessionState::Connecting) {
        inputs.screen = Screen::Join;
        inputs.code = "4821-gato-lua".into();
        inputs.joining = state == SessionState::Connecting;
    }
    let _ = draw_flow(ui, &view, inputs, None, demo);
    actions
}

fn demo_view(state: SessionState) -> crate::runtime::View {
    use crate::runtime::{JoinRequest, Role, View};
    let mut view = View::default();
    if matches!(state, SessionState::Idle | SessionState::JoinCode) {
        return view;
    }
    if state == SessionState::Connecting {
        view.role = Role::Guest;
        view.internet = true;
        view.room_code = Some("4821-gato-lua".into());
        return view;
    }
    if state == SessionState::PendingApproval {
        view.role = Role::Guest;
        view.peer_name = Some("Ana".into());
        view.room_address = Some("192.168.1.10:7842".parse().unwrap());
        view.waiting_approval = true;
        return view;
    }
    if state == SessionState::Browsing {
        view.rooms.push(crate::discovery::Room {
            name: "Ana".into(),
            address: "100.64.0.2:7842".parse().unwrap(),
        });
        return view;
    }
    view.role = Role::Host;
    view.file = Some("Filme de demonstração.mkv".into());
    view.verified = true;
    view.player.loaded = true;
    view.player.paused = state != SessionState::Synchronized;
    view.player.duration = 5400.0;
    view.player.position = 754.0;
    view.connected = !matches!(
        state,
        SessionState::Waiting | SessionState::Approval | SessionState::Reconnecting
    );
    view.peer_name = Some("notebook-ana".into());
    view.peer_ready = view.connected;
    view.peer_verified = view.connected;
    view.peer_file = view
        .connected
        .then(|| "Filme.de.demonstração.1080p.mkv".into());
    if state == SessionState::Approval {
        view.room_address = Some("192.168.1.10:7842".parse().unwrap());
    } else {
        view.internet = true;
        view.room_code = Some("4821-gato-lua".into());
    }
    view.matched = view.connected;
    view.ready = view.connected;
    view.watching = matches!(
        state,
        SessionState::Synchronized | SessionState::Paused | SessionState::Reconnecting
    );
    view.status = state.label().into();
    if view.watching && view.connected {
        view.peer_drift = Some(0.0);
    }
    if state == SessionState::Approval {
        view.join_request = Some(JoinRequest {
            id: 1,
            name: "notebook-ana".into(),
            address: "100.64.0.2:45678".parse().unwrap(),
        });
    }
    match state {
        SessionState::Choosing => {
            view.file = None;
            view.verified = false;
            view.player.loaded = false;
            view.peer_file = None;
            view.peer_verified = false;
            view.peer_ready = false;
            view.matched = false;
            view.ready = false;
        }
        SessionState::Verifying => {
            view.loading = true;
            view.pending_file = view.file.take();
            view.verification = Some(crate::protocol::Verification {
                bytes: 42,
                total: 100,
            });
            view.peer_loading = true;
            view.peer_verification = Some(crate::protocol::Verification {
                bytes: 73,
                total: 100,
            });
            view.verified = false;
            view.peer_verified = false;
            view.ready = false;
            view.matched = false;
        }
        SessionState::Mismatch => {
            view.peer_file = Some("Filme.de.demonstração.720p.mp4".into());
            view.ready = false;
            view.matched = false;
        }
        SessionState::Stabilizing => {
            view.ready = false;
        }
        SessionState::PlayerError => {
            view.error = Some(
                "Não foi possível abrir o vídeo. Confira o arquivo e tente escolher outra cópia."
                    .into(),
            );
            view.player.blocked = true;
            view.ready = false;
        }
        _ => {}
    }
    view
}
