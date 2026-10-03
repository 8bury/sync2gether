use eframe::egui;
use sync2gether::{model::SessionState, ui};

#[test]
fn layout_fits_supported_window_sizes_in_both_themes() {
    for light in [false, true] {
        for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
            for (state, demo) in std::iter::once((SessionState::Idle, false))
                .chain(SessionState::ALL.map(|state| (state, true)))
            {
                let ctx = egui::Context::default();
                ui::configure_theme(&ctx, light);
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                size.into(),
                            )),
                            ..Default::default()
                        },
                        |root| {
                            let actions = ui::draw(root, state, demo);
                            assert!(actions.is_empty());
                            assert!(
                                root.min_rect().width() <= size[0] + 1.0,
                                "horizontal overflow: {state:?}, demo={demo} at {size:?}: {:?}",
                                root.min_rect()
                            );
                        },
                    );
                    assert!(!output.shapes.is_empty(), "empty screen: {state:?}");
                    // Sem renderer neste teste: descarte explicitamente os uploads de texturas.
                    output.textures_delta.clear();
                }
            }
        }
    }
}

#[test]
fn real_session_layout_fits_without_starting_io() {
    use sync2gether::{
        player::Status,
        runtime::{Role, View},
    };
    for light in [false, true] {
        for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
            for role in [Role::Local, Role::Host, Role::Guest] {
                let view = View { role, connected: true, status: "Aguardando os dois players ficarem prontos".into(), file: Some("Filme sintético.mkv".into()), room_key: "a".repeat(32), player: Status { loaded: true, paused: true, duration: 5400.0, position: 754.0, ..Default::default() }, error: Some("Erro de teste com texto longo para verificar a quebra de linha em janelas estreitas.".into()), ..Default::default() };
                let ctx = egui::Context::default();
                ui::configure_theme(&ctx, light);
                let mut inputs = ui::Inputs::default();
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                        ..Default::default()
                    },
                    |root| {
                        assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty());
                        assert!(root.min_rect().width() <= size[0] + 1.0);
                    },
                );
                output.textures_delta.clear();
            }
        }
    }
}

fn player_frame(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    time: f64,
    events: Vec<egui::Event>,
) -> Vec<sync2gether::runtime::Command> {
    let mut actions = Vec::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 360.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |root| {
            actions = ui::draw_live(root, view, inputs, None);
        },
    );
    output.textures_delta.clear();
    actions
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn player_controls_hide_reappear_and_keep_room_commands_coordinated() {
    use sync2gether::{
        player::Status,
        protocol::Control,
        runtime::{Command, Role, View},
    };
    let ctx = egui::Context::default();
    let mut inputs = ui::Inputs::default();
    inputs.player.fullscreen = true;
    let mut view = View {
        player: Status {
            loaded: true,
            paused: false,
            position: 20.0,
            duration: 120.0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(
        player_frame(
            &ctx,
            &view,
            &mut inputs,
            0.0,
            vec![egui::Event::PointerMoved(egui::pos2(320.0, 130.0))]
        )
        .is_empty()
    );
    assert!(inputs.player.controls_visible);
    assert!(player_frame(&ctx, &view, &mut inputs, 3.1, vec![]).is_empty());
    assert!(!inputs.player.controls_visible);
    player_frame(
        &ctx,
        &view,
        &mut inputs,
        3.2,
        vec![egui::Event::PointerMoved(egui::pos2(321.0, 130.0))],
    );
    assert!(inputs.player.controls_visible);
    let actions = player_frame(&ctx, &view, &mut inputs, 3.3, vec![key(egui::Key::Space)]);
    assert!(matches!(
        actions.as_slice(),
        [Command::Control(Control::Pause)]
    ));
    view.player.paused = true;
    player_frame(&ctx, &view, &mut inputs, 8.0, vec![]);
    assert!(inputs.player.controls_visible);
    view.role = Role::Guest;
    view.ready = false;
    assert!(player_frame(&ctx, &view, &mut inputs, 8.1, vec![key(egui::Key::Space)]).is_empty());
    view.ready = true;
    view.preparing = true;
    let actions = player_frame(&ctx, &view, &mut inputs, 8.2, vec![key(egui::Key::Space)]);
    assert!(matches!(
        actions.as_slice(),
        [Command::Control(Control::Pause)]
    ));
    view.preparing = false;
    view.matched = true;
    let actions = player_frame(
        &ctx,
        &view,
        &mut inputs,
        8.3,
        vec![key(egui::Key::ArrowRight)],
    );
    assert!(matches!(actions.as_slice(), [Command::Control(Control::Seek(p))] if *p == 30.0));
    player_frame(&ctx, &view, &mut inputs, 8.4, vec![key(egui::Key::Escape)]);
    assert!(!inputs.player.fullscreen);
}
#[test]
fn dragging_progress_commits_once_without_toggling_playback() {
    use sync2gether::{
        player::Status,
        protocol::Control,
        runtime::{Command, View},
    };
    let ctx = egui::Context::default();
    let mut inputs = ui::Inputs::default();
    inputs.player.fullscreen = true;
    let view = View {
        player: Status {
            loaded: true,
            paused: false,
            position: 20.0,
            duration: 120.0,
            ..Default::default()
        },
        ..Default::default()
    };
    player_frame(&ctx, &view, &mut inputs, 0.0, vec![]);
    // Linha de progresso na faixa inferior do player de 640 × 360.
    let start = egui::pos2(96.0, 272.0);
    let end = egui::pos2(280.0, 272.0);
    let pointer_button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    player_frame(
        &ctx,
        &view,
        &mut inputs,
        0.1,
        vec![egui::Event::PointerMoved(start)],
    );
    assert!(
        player_frame(
            &ctx,
            &view,
            &mut inputs,
            0.2,
            vec![pointer_button(start, true)]
        )
        .is_empty()
    );
    assert!(
        player_frame(
            &ctx,
            &view,
            &mut inputs,
            4.0,
            vec![egui::Event::PointerMoved(end)]
        )
        .is_empty()
    );
    assert!(inputs.player.controls_visible);
    let actions = player_frame(
        &ctx,
        &view,
        &mut inputs,
        4.1,
        vec![pointer_button(end, false)],
    );
    assert!(
        matches!(actions.as_slice(), [Command::Control(Control::Seek(p))] if *p > 50.0 && *p < 90.0),
        "wrong actions on release"
    );
    assert!(inputs.seek.is_none());
}

#[test]
fn media_shortcuts_do_not_intercept_focused_text_fields() {
    use sync2gether::{player::Status, runtime::View};
    let ctx = egui::Context::default();
    let view = View {
        player: Status {
            loaded: true,
            paused: false,
            duration: 120.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut inputs = ui::Inputs::default();
    inputs.player.fullscreen = true;
    let mut text = String::new();
    for events in [
        vec![],
        vec![
            key(egui::Key::Space),
            key(egui::Key::F),
            key(egui::Key::M),
            key(egui::Key::ArrowRight),
        ],
    ] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 360.0),
                )),
                events,
                ..Default::default()
            },
            |root| {
                let field = root
                    .add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("room_address")));
                field.request_focus();
                assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty());
            },
        );
        output.textures_delta.clear();
    }
    assert!(inputs.player.fullscreen);
    assert_eq!(inputs.volume, 100.0);
}

fn visible_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(t) => {
                text.push_str(&t.galley.job.text);
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    for shape in shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

#[test]
fn stages_and_room_panel_fit_without_exposing_codes_or_statistics() {
    use sync2gether::{
        discovery::Room,
        player::Status,
        runtime::{JoinRequest, Role, View},
    };
    for light in [false, true] {
        for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
            for screen in [
                ui::Screen::Home,
                ui::Screen::Browse,
                ui::Screen::Preparation,
                ui::Screen::Watching,
            ] {
                for panel in [false, true] {
                    let mut view = View {
                        status: "Assistindo juntos".into(),
                        room_key: "private-token-never-visible".into(),
                        ..Default::default()
                    };
                    if screen == ui::Screen::Browse {
                        view.rooms = vec![Room {
                            name: "nome-comprido-do-dispositivo-anfitriao-para-teste-de-quebra"
                                .into(),
                            address: "100.64.0.2:7842".parse().unwrap(),
                        }];
                    } else if screen != ui::Screen::Home {
                        view.role = Role::Host;
                        view.connected = true;
                        view.file = Some("Um nome de filme comprido para testar a apresentação em janela estreita.mkv".into());
                        view.player = Status {
                            loaded: true,
                            paused: true,
                            duration: 5400.0,
                            ..Default::default()
                        };
                        view.ready = true;
                        view.matched = true;
                        view.peer_ready = true;
                        view.watching = screen == ui::Screen::Watching;
                        if screen == ui::Screen::Preparation {
                            view.join_request = Some(JoinRequest {
                                id: 1,
                                name: "notebook-ana".into(),
                                address: "100.64.0.3:34567".parse().unwrap(),
                            });
                        }
                    }
                    let ctx = egui::Context::default();
                    ui::configure_theme(&ctx, light);
                    let mut inputs = ui::Inputs::default();
                    inputs.screen = screen;
                    inputs.room_panel = panel;
                    for _ in 0..3 {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    size.into(),
                                )),
                                ..Default::default()
                            },
                            |root| {
                                assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty());
                                assert!(
                                    root.min_rect().width() <= size[0] + 1.0,
                                    "overflow at {size:?}, {screen:?}, panel={panel}"
                                );
                            },
                        );
                        let text = visible_text(&output.shapes);
                        output.textures_delta.clear();
                        assert!(!text.contains("private-token"));
                        assert!(!text.contains("Diferença estimada"));
                        assert!(!text.contains("Ida e volta"));
                        if screen == ui::Screen::Preparation {
                            assert!(inputs.player.video_rect.is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn reconnection_keeps_the_player_and_changing_media_returns_to_preparation() {
    use sync2gether::{
        player::Status,
        runtime::{Role, View},
    };
    let ctx = egui::Context::default();
    let mut inputs = ui::Inputs::default();
    let mut view = View {
        role: Role::Guest,
        watching: true,
        connected: true,
        player: Status {
            loaded: true,
            paused: true,
            duration: 120.0,
            ..Default::default()
        },
        ..Default::default()
    };
    player_frame(&ctx, &view, &mut inputs, 0.0, vec![]);
    assert_eq!(inputs.screen, ui::Screen::Watching);
    view.connected = false;
    player_frame(&ctx, &view, &mut inputs, 1.0, vec![]);
    assert_eq!(inputs.screen, ui::Screen::Watching);
    view.connected = true;
    view.watching = false;
    player_frame(&ctx, &view, &mut inputs, 2.0, vec![]);
    assert_eq!(inputs.screen, ui::Screen::Preparation);
    view.role = Role::Local;
    player_frame(&ctx, &view, &mut inputs, 3.0, vec![]);
    assert_eq!(inputs.screen, ui::Screen::Home);
}

fn find_label(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
    match shape {
        egui::Shape::Text(text) if text.galley.job.text == label => {
            Some(text.galley.rect.translate(text.pos.to_vec2()).center())
        }
        egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find_label(shape, label)),
        _ => None,
    }
}
fn click_label(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    label: &str,
) -> Vec<sync2gether::runtime::Command> {
    click_label_at_size(ctx, view, inputs, label, [960.0, 800.0])
}
fn click_label_at_size(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    label: &str,
    size: [f32; 2],
) -> Vec<sync2gether::runtime::Command> {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
            ..Default::default()
        },
        |root| {
            let _ = ui::draw_live(root, view, inputs, None);
        },
    );
    let position = output
        .shapes
        .iter()
        .find_map(|shape| find_label(&shape.shape, label));
    output.textures_delta.clear();
    let position = position.unwrap_or_else(|| panic!("label not found: {label}"));
    let mut actions = Vec::new();
    for pressed in [true, false] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |root| {
                actions.extend(ui::draw_live(root, view, inputs, None));
            },
        );
        output.textures_delta.clear();
    }
    actions
}
#[test]
fn room_buttons_return_commands_and_play_is_disabled_until_both_are_ready() {
    use sync2gether::{
        protocol::{Control, PORT},
        runtime::{Command, JoinRequest, Role, View},
    };
    let ctx = egui::Context::default();
    let mut inputs = ui::Inputs::default();
    let mut view = View::default();
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Criar sala").as_slice(),
        [Command::HostAuto(PORT)]
    ));
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Entrar em uma sala").as_slice(),
        [Command::Discover]
    ));
    assert_eq!(inputs.screen, ui::Screen::Browse);
    view.role = Role::Host;
    view.join_request = Some(JoinRequest {
        id: 123,
        name: "notebook-ana".into(),
        address: "100.64.0.3:34567".parse().unwrap(),
    });
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Aceitar").as_slice(),
        [Command::Approve {
            id: 123,
            accept: true
        }]
    ));
    view.join_request = None;
    assert!(click_label(&ctx, &view, &mut inputs, "Começar a assistir").is_empty());
    view.ready = true;
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Começar a assistir").as_slice(),
        [Command::Control(Control::Play)]
    ));
}

#[test]
fn preparation_primary_action_is_visible_at_supported_sizes() {
    for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
        for light in [false, true] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            ui::configure_theme(&ctx, light);
            let mut inputs = ui::Inputs::default();
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                        ..Default::default()
                    },
                    |root| {
                        assert!(
                            ui::draw_with_inputs(root, SessionState::Ready, true, &mut inputs)
                                .is_empty()
                        );
                    },
                );
                output.textures_delta.clear();
                let action = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        find_label(&shape.shape, "Começar a assistir")
                            .map(|center| (center, shape.clip_rect))
                    })
                    .unwrap_or_else(|| {
                        panic!("primary action missing: {}", visible_text(&output.shapes))
                    });
                let bounds = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find_map(|(_, node)| {
                        (node.label() == Some("Começar a assistir"))
                            .then(|| node.bounds())
                            .flatten()
                    })
                    .expect("primary button missing from accessibility tree");
                let bounds = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                assert!(
                    action.1.contains_rect(bounds),
                    "primary action clipped at {size:?}, light={light}: {bounds:?} in {:?}",
                    action.1
                );
                output.textures_delta.clear();
            }
        }
    }
}

#[test]
fn home_actions_are_visible_in_the_smallest_window() {
    for light in [false, true] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ui::configure_theme(&ctx, light);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 480.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    assert!(ui::draw(root, SessionState::Idle, true).is_empty());
                },
            );
            output.textures_delta.clear();
            for label in ["Criar sala", "Entrar em uma sala", "Abrir filme sozinho"] {
                let clip = output
                    .shapes
                    .iter()
                    .find_map(|shape| find_label(&shape.shape, label).map(|_| shape.clip_rect))
                    .unwrap_or_else(|| panic!("home action missing: {label}"));
                let bounds = output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find_map(|(_, node)| {
                        (node.label() == Some(label))
                            .then(|| node.bounds())
                            .flatten()
                    })
                    .unwrap();
                let bounds = egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                assert!(
                    clip.contains_rect(bounds),
                    "clipped home action: {label}, light={light}"
                );
            }
        }
    }
}

fn click_player_icon(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    label: &str,
) -> Vec<sync2gether::runtime::Command> {
    ctx.enable_accesskit();
    let mut position = None;
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 360.0),
                )),
                ..Default::default()
            },
            |root| {
                assert!(ui::draw_live(root, view, inputs, None).is_empty());
            },
        );
        for (_, node) in output.platform_output.accesskit_update.unwrap().nodes {
            if node.label() == Some(label) {
                let rect = node.bounds().expect("icon must have accessible bounds");
                assert!(
                    rect.x1 - rect.x0 >= 30.0 && rect.y1 - rect.y0 >= 30.0,
                    "clipped icon: {label}"
                );
                position = Some(egui::pos2(
                    ((rect.x0 + rect.x1) * 0.5) as f32,
                    ((rect.y0 + rect.y1) * 0.5) as f32,
                ));
            }
        }
        output.textures_delta.clear();
    }
    let pos = position.unwrap_or_else(|| panic!("icon without accessible label: {label}"));
    let mut commands = Vec::new();
    for pressed in [true, false] {
        commands.extend(player_frame(
            ctx,
            view,
            inputs,
            0.0,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ));
    }
    commands
}

#[test]
fn player_icon_menus_keep_audio_and_volume_local() {
    use sync2gether::{
        player::{Status, Track, TrackKind},
        runtime::{Command, Role, View},
    };
    let ctx = egui::Context::default();
    ui::configure_theme(&ctx, false);
    let mut inputs = ui::Inputs::default();
    let view = View {
        role: Role::Guest,
        connected: true,
        watching: true,
        player: Status {
            loaded: true,
            paused: true,
            duration: 120.0,
            tracks: vec![Track {
                id: 2,
                kind: TrackKind::Audio,
                title: Some("Áudio de teste".into()),
                language: None,
                selected: false,
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(click_player_icon(&ctx, &view, &mut inputs, "Áudio e legendas").is_empty());
    assert!(matches!(
        click_label_at_size(&ctx, &view, &mut inputs, "Áudio de teste", [640.0, 360.0]).as_slice(),
        [Command::AudioTrack(2)]
    ));
    assert!(click_player_icon(&ctx, &view, &mut inputs, "Áudio e legendas").is_empty());
    assert!(matches!(
        click_label_at_size(&ctx, &view, &mut inputs, "Desativadas", [640.0, 360.0]).as_slice(),
        [Command::SubtitleTrack(None)]
    ));
    assert!(click_player_icon(&ctx, &view, &mut inputs, "Volume · M para silenciar").is_empty());
    assert!(
        matches!(click_label_at_size(&ctx, &view, &mut inputs, "Silenciar", [640.0, 360.0]).as_slice(), [Command::Volume(v)] if *v == 0.0)
    );
}

#[test]
fn player_shows_actionable_notices_and_omits_routine_status() {
    use sync2gether::{
        player::Status,
        runtime::{Role, View},
    };
    let mut view = View {
        role: Role::Host,
        watching: true,
        connected: true,
        status: "Assistindo juntos".into(),
        file: Some("Filme sintético.mkv".into()),
        player: Status {
            loaded: true,
            paused: true,
            duration: 120.0,
            ..Default::default()
        },
        ..Default::default()
    };
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut inputs = ui::Inputs::default();
        for disconnected in [false, true] {
            view.connected = !disconnected;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 360.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty());
                },
            );
            output.textures_delta.clear();
            let title_color = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Filme sintético" => {
                        Some(text.galley.job.sections[0].format.color)
                    }
                    _ => None,
                })
                .expect("movie title missing");
            assert_eq!(
                title_color,
                egui::Color32::WHITE,
                "player title must stay readable in theme light={light}"
            );
            let text = visible_text(&output.shapes);
            assert!(!text.contains("Assistindo juntos"));
            assert!(!text.contains("Sessão pausada"));
            assert_eq!(
                text.contains("Aguardando a outra pessoa voltar"),
                disconnected
            );
            assert_eq!(text.contains("Opções da sala"), disconnected);
            output.textures_delta.clear();
        }
    }
}

fn preparation_frame(view: &sync2gether::runtime::View, size: [f32; 2], light: bool) -> String {
    let ctx = egui::Context::default();
    ui::configure_theme(&ctx, light);
    let mut inputs = ui::Inputs::default();
    let mut text = String::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                ..Default::default()
            },
            |root| {
                assert!(ui::draw_live(root, view, &mut inputs, None).is_empty());
                assert!(root.min_rect().width() <= size[0] + 1.0);
            },
        );
        text = visible_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

#[test]
fn preparation_shows_both_files_and_explains_content_comparison_for_both_roles() {
    use sync2gether::runtime::{Role, View};
    for role in [Role::Host, Role::Guest] {
        for light in [false, true] {
            let mut view = View {
                role,
                connected: true,
                file: Some("Meu.filme.mkv".into()),
                peer_file: Some("Outro.nome.mp4".into()),
                verified: true,
                peer_verified: true,
                peer_ready: true,
                matched: true,
                ready: true,
                ..Default::default()
            };
            view.player.loaded = true;
            let text = preparation_frame(&view, [960.0, 1000.0], light);
            for label in [
                "Meu.filme.mkv",
                "Outro.nome.mp4",
                "Você",
                "Outra pessoa",
                "Os arquivos são iguais",
                "Nome completo",
                "Copiar nome",
            ] {
                assert!(text.contains(label), "missing {label}: {text}");
            }
            view.ready = false;
            let text = preparation_frame(&view, [960.0, 1000.0], light);
            assert!(text.contains("conexão estabilizar"));
            view.peer_file = view.file.clone();
            view.matched = false;
            let text = preparation_frame(&view, [960.0, 1000.0], light);
            assert!(text.contains("Os arquivos são diferentes"));
            assert!(!text.contains("Os arquivos são iguais"));
            assert!(text.contains("O nome igual não garante conteúdo igual"));
            assert!(text.contains(if role == Role::Host {
                "Encerrar sala"
            } else {
                "Sair da sala"
            }));
            for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
                view.file = Some(format!("{}.mkv", "Filme muito longo ".repeat(15)));
                view.peer_file = Some(format!("{}.mp4", "Outra edição ".repeat(20)));
                preparation_frame(&view, size, light);
            }
        }
    }
}

#[test]
fn cancelling_verification_keeps_the_room_and_start_is_blocked() {
    use sync2gether::{
        protocol::Verification,
        runtime::{Command, Role, View},
    };
    let ctx = egui::Context::default();
    let mut inputs = ui::Inputs::default();
    let view = View {
        role: Role::Host,
        connected: true,
        loading: true,
        pending_file: Some("Novo.mkv".into()),
        file: Some("Anterior.mkv".into()),
        verification: Some(Verification {
            bytes: 42,
            total: 100,
        }),
        peer_file: Some("Companhia.mp4".into()),
        peer_loading: true,
        peer_verification: Some(Verification {
            bytes: 73,
            total: 100,
        }),
        ..Default::default()
    };
    let text = preparation_frame(&view, [960.0, 1000.0], false);
    assert!(text.contains("Novo.mkv"));
    assert!(!text.contains("Anterior.mkv"));
    assert!(text.contains("42%") && text.contains("73%"));
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Cancelar verificação").as_slice(),
        [Command::CancelOpen]
    ));
    assert!(
        click_label_at_size(
            &ctx,
            &view,
            &mut inputs,
            "Começar a assistir",
            [960.0, 640.0]
        )
        .is_empty()
    );
}

#[test]
fn approval_actions_remain_visible_in_the_smallest_window() {
    for light in [false, true] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ui::configure_theme(&ctx, light);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 480.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    ui::draw(root, SessionState::Approval, true);
                },
            );
            let clip = output
                .shapes
                .iter()
                .find_map(|shape| find_label(&shape.shape, "Aceitar").map(|_| shape.clip_rect))
                .expect("approval missing");
            let bounds = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some("Aceitar"))
                        .then(|| node.bounds())
                        .flatten()
                })
                .expect("approval accessibility bounds missing");
            assert!(
                clip.contains_rect(egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32)
                )),
                "approval action clipped"
            );
            output.textures_delta.clear();
        }
    }
}
