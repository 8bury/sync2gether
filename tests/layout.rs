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
                    let text = visible_text(&output.shapes);
                    for removed in [
                        "O mesmo momento",
                        "Um play para vocês dois",
                        "Pela rede local ou VPN",
                        "A outra pessoa pode encontrar sua sala",
                        "Encerrar a sala desconecta",
                        "Cada pessoa abre a cópia",
                        "Uma vaga para você",
                        "Aceite para escolherem o filme juntos",
                    ] {
                        assert!(
                            !text.contains(removed),
                            "redundant copy {removed:?} in {state:?}, light={light}"
                        );
                    }
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
    let text = preparation_text_of(&ctx, &view, &mut inputs);
    assert!(
        !text.contains("rede local"),
        "a rede local fica nas opções avançadas"
    );
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Criar sala").as_slice(),
        [Command::HostInternet(relay)] if relay == sync2gether::network::relay::DEFAULT_RELAY
    ));
    // O código é digitado no próprio início, sem uma tela só para ele.
    assert!(text.contains("ou entre com um código"));
    assert_eq!(inputs.screen, ui::Screen::Home);
    inputs.lan = true;
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Criar sala na rede local").as_slice(),
        [Command::HostAuto(PORT)]
    ));
    assert!(matches!(
        click_label(&ctx, &view, &mut inputs, "Procurar na rede local").as_slice(),
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
    view.connected = true;
    // Desativado, o botão principal diz o que falta.
    assert!(click_label(&ctx, &view, &mut inputs, "Escolha seu arquivo para começar").is_empty());
    view.file = Some("Filme.mkv".into());
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
            for label in ["Criar sala", "Entrar", "Abrir filme sozinho"] {
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
            // A queda da conexão aparece só no selo da sala, sem um segundo aviso.
            assert_eq!(
                text.contains("Esperando a outra pessoa voltar"),
                disconnected
            );
            assert_eq!(text.contains("em sincronia"), !disconnected);
            assert!(!text.contains("Opções da sala"));
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

fn preparation_output(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Vec<sync2gether::runtime::Command>) {
    let mut actions = Vec::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |root| {
            actions = ui::draw_live(root, view, inputs, None);
        },
    );
    output.textures_delta.clear();
    (output, actions)
}

fn click_preparation_widget(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    label: &str,
) -> egui::FullOutput {
    ctx.enable_accesskit();
    let mut position = None;
    for _ in 0..3 {
        let (output, actions) = preparation_output(ctx, view, inputs, vec![]);
        assert!(actions.is_empty());
        for (_, node) in output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
        {
            if node.label() == Some(label) {
                let bounds = node.bounds().expect("accessible control must have bounds");
                position = Some(egui::pos2(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    ((bounds.y0 + bounds.y1) * 0.5) as f32,
                ));
            }
        }
    }
    let position = position.unwrap_or_else(|| panic!("accessible control not found: {label}"));
    let event = |pressed| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let (_, actions) = preparation_output(
        ctx,
        view,
        inputs,
        vec![egui::Event::PointerMoved(position), event(true)],
    );
    assert!(actions.is_empty());
    let (output, actions) = preparation_output(ctx, view, inputs, vec![event(false)]);
    assert!(actions.is_empty());
    output
}

#[test]
fn each_file_menu_copies_the_full_name_without_changing_playback() {
    use sync2gether::runtime::{Role, View};
    for light in [false, true] {
        for local in [false, true] {
            let ctx = egui::Context::default();
            ui::configure_theme(&ctx, light);
            let mut inputs = ui::Inputs::default();
            let view = View {
                role: Role::Guest,
                connected: true,
                file: Some(format!("{}.mkv", "Minha cópia com nome longo ".repeat(8))),
                peer_file: Some(format!("{}.mp4", "Cópia da outra pessoa ".repeat(8))),
                peer_name: Some("Ana".into()),
                ..Default::default()
            };
            let _ = click_preparation_widget(
                &ctx,
                &view,
                &mut inputs,
                if local {
                    "Opções do seu arquivo"
                } else {
                    "Opções do arquivo da outra pessoa"
                },
            );
            assert!(egui::Popup::is_any_open(&ctx));
            let (output, actions) = preparation_output(&ctx, &view, &mut inputs, vec![]);
            assert!(actions.is_empty());
            assert!(
                visible_text(&output.shapes).contains("Nome completo"),
                "menu not shown: local={local}, light={light}, popup={}, text={}",
                egui::Popup::is_any_open(&ctx),
                visible_text(&output.shapes)
            );
            let output = click_preparation_widget(&ctx, &view, &mut inputs, "Copiar nome");
            let expected = if local {
                view.file.as_ref().unwrap()
            } else {
                view.peer_file.as_ref().unwrap()
            };
            assert!(output.platform_output.commands.iter().any(
                |command| matches!(command, egui::OutputCommand::CopyText(name) if name == expected)
            ));
            let (output, actions) = preparation_output(&ctx, &view, &mut inputs, vec![]);
            assert!(actions.is_empty());
            assert!(visible_text(&output.shapes).contains("Copiado"));
            assert!(!inputs.player.fullscreen);
        }
    }
}

#[test]
fn preparation_changes_only_the_local_file_and_cancels_scheduled_start() {
    use sync2gether::{
        protocol::Control,
        runtime::{Command, Role, View},
    };
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut inputs = ui::Inputs::default();
        let mut view = View {
            role: Role::Guest,
            connected: true,
            peer_file: Some("Companhia.mkv".into()),
            ..Default::default()
        };
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Escolher arquivo").as_slice(),
            [Command::Open]
        ));
        assert!(
            click_label(&ctx, &view, &mut inputs, "Escolha seu arquivo para começar").is_empty()
        );
        view.file = Some("Meu.mkv".into());
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Trocar arquivo").as_slice(),
            [Command::Open]
        ));
        let (output, _) = preparation_output(&ctx, &view, &mut inputs, vec![]);
        let text = visible_text(&output.shapes);
        assert_eq!(text.matches("Trocar arquivo").count(), 1);
        assert!(!text.contains("Nome completo") && !text.contains("Copiar nome"));
        view.preparing = true;
        view.scheduled = true;
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Cancelar início").as_slice(),
            [Command::Control(Control::Pause)]
        ));
        view.preparing = false;
        view.scheduled = false;
        view.player.blocked = true;
        let text = preparation_frame(&view, [960.0, 800.0], light);
        assert!(text.contains("Player indisponível"));
        assert!(click_label(&ctx, &view, &mut inputs, "Aguardando o player").is_empty());
    }
}

#[test]
fn connection_screen_reports_only_confirmed_stages_and_keeps_cancellation_visible() {
    use sync2gether::runtime::{Command, Role, View};
    for light in [false, true] {
        for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            ui::configure_theme(&ctx, light);
            let mut inputs = ui::Inputs::default();
            let mut view = View {
                role: Role::Guest,
                peer_name: Some("Anfitrião de teste com um nome muito comprido ".repeat(8)),
                room_key: "private-token-never-visible".into(),
                ..Default::default()
            };
            for waiting in [false, true] {
                view.waiting_approval = waiting;
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
                            assert!(root.min_rect().width() <= size[0] + 1.0);
                        },
                    );
                    output.textures_delta.clear();
                    let text = visible_text(&output.shapes);
                    assert_eq!(text.contains("Anfitrião encontrado"), waiting);
                    assert_eq!(text.contains("Solicitação enviada"), waiting);
                    assert_eq!(text.contains("Aguardando aprovação"), waiting);
                    assert!(!text.contains("Começar a assistir"));
                    assert!(!text.contains("Escolham o mesmo filme"));
                    assert!(!text.contains("private-token"));
                    let clip = output
                        .shapes
                        .iter()
                        .find_map(|shape| {
                            find_label(&shape.shape, "Cancelar solicitação")
                                .map(|_| shape.clip_rect)
                        })
                        .expect("cancellation missing");
                    let bounds = output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .unwrap()
                        .nodes
                        .iter()
                        .find_map(|(_, node)| {
                            (node.label() == Some("Cancelar solicitação"))
                                .then(|| node.bounds())
                                .flatten()
                        })
                        .expect("cancellation accessibility bounds missing");
                    assert!(
                        clip.contains_rect(egui::Rect::from_min_max(
                            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                        )),
                        "cancellation clipped at {size:?}, light={light}, waiting={waiting}"
                    );
                    output.textures_delta.clear();
                }
                assert!(matches!(
                    click_label_at_size(&ctx, &view, &mut inputs, "Cancelar solicitação", size)
                        .as_slice(),
                    [Command::Leave]
                ));
            }
            view.error = Some("Não foi possível alcançar o anfitrião.".into());
            let text = preparation_frame(&view, size, light);
            assert!(text.contains("Não foi possível conectar"));
            assert!(text.contains("Não foi possível alcançar o anfitrião."));
            assert!(!text.contains("Anfitrião encontrado"));
            assert!(!text.contains("Aguardando aprovação"));
            view.error = None;
            view.connected = true;
            let text = preparation_frame(&view, size, light);
            assert!(text.contains("Escolham o mesmo filme"));
            assert!(!text.contains("Cancelar solicitação"));
        }
    }
}

#[test]
fn room_browser_selects_the_room_and_preserves_manual_entry_after_discovery_failure() {
    use sync2gether::{
        discovery::Room,
        runtime::{Command, View},
    };
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut inputs = ui::Inputs::default();
        inputs.lan = true;
        let mut view = View::default();
        click_label(&ctx, &view, &mut inputs, "Procurar na rede local");
        for address in ["192.168.1.10:7842", "192.168.1.20:7843"] {
            view.rooms = vec![Room {
                name: "Ana".into(),
                address: address.parse().unwrap(),
            }];
            assert!(
                matches!(click_label(&ctx, &view, &mut inputs, "Solicitar entrada").as_slice(),
                [Command::RequestJoin(selected)] if selected == address)
            );
        }
        view.rooms.clear();
        view.discovery_error = Some("Descoberta indisponível na rede de teste.".into());
        inputs.address = "endereço inválido".into();
        assert!(click_label(&ctx, &view, &mut inputs, "Solicitar entrada").is_empty());
        inputs.address = "192.168.1.30:7844".into();
        assert!(
            matches!(click_label(&ctx, &view, &mut inputs, "Solicitar entrada").as_slice(),
            [Command::RequestJoin(selected)] if selected == "192.168.1.30:7844")
        );
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Voltar").as_slice(),
            [Command::CancelDiscovery]
        ));
        assert_eq!(inputs.screen, ui::Screen::Home);
    }
}

#[test]
fn manual_address_accepts_enter_after_editing_in_the_same_frame() {
    use sync2gether::runtime::{Command, View};
    let ctx = egui::Context::default();
    ui::configure_theme(&ctx, true);
    let view = View::default();
    let mut inputs = ui::Inputs::default();
    inputs.screen = ui::Screen::Browse;
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 800.0),
                )),
                ..Default::default()
            },
            |root| {
                assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty());
            },
        );
        output.textures_delta.clear();
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("host_address")));
    let mut actions = Vec::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 800.0),
            )),
            events: vec![
                egui::Event::Text("192.168.1.10:7842".into()),
                key(egui::Key::Enter),
            ],
            ..Default::default()
        },
        |root| {
            actions = ui::draw_live(root, &view, &mut inputs, None);
        },
    );
    output.textures_delta.clear();
    assert!(
        matches!(actions.as_slice(), [Command::RequestJoin(address)] if address == "192.168.1.10:7842")
    );
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
            ] {
                assert!(text.contains(label), "missing {label}: {text}");
            }
            assert!(!text.contains("Nome completo") && !text.contains("Copiar nome"));
            view.ready = false;
            let text = preparation_frame(&view, [960.0, 1000.0], light);
            assert!(text.contains("conexão estabilizar"));
            view.peer_file = view.file.clone();
            view.matched = false;
            let text = preparation_frame(&view, [960.0, 1000.0], light);
            assert!(text.contains("Os arquivos são diferentes"));
            assert!(!text.contains("Os arquivos são iguais"));
            assert!(text.contains("Escolham arquivos com o mesmo conteúdo"));
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
            "Conferindo seu arquivo…",
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

#[test]
fn settings_icon_keeps_its_accessible_name_and_switches_the_theme() {
    let view = sync2gether::runtime::View::default();
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut inputs = ui::Inputs::default();
        let _ = click_preparation_widget(&ctx, &view, &mut inputs, "Configurações");
        assert!(egui::Popup::is_any_open(&ctx));
        let (output, actions) = preparation_output(&ctx, &view, &mut inputs, vec![]);
        assert!(actions.is_empty());
        let text = visible_text(&output.shapes);
        assert!(
            text.contains("Escuro") && text.contains("Claro"),
            "settings menu did not open: {text}"
        );
        click_preparation_widget(
            &ctx,
            &view,
            &mut inputs,
            if light { "Escuro" } else { "Claro" },
        );
        assert_eq!(ctx.global_style().visuals.dark_mode, light);
    }
}

fn preparation_text_of(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
) -> String {
    let mut text = String::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 800.0),
                )),
                ..Default::default()
            },
            |root| {
                let _ = ui::draw_live(root, view, inputs, None);
            },
        );
        text = visible_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

#[test]
fn code_entry_validates_joins_and_returns_to_the_code_after_a_refusal() {
    use sync2gether::runtime::{Command, Role, View};
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut view = View::default();
        let mut inputs = ui::Inputs::default();
        inputs.code = "4821-gato-limao".into();
        assert!(click_label(&ctx, &view, &mut inputs, "Entrar").is_empty());
        assert!(preparation_text_of(&ctx, &view, &mut inputs).contains("Digite o código completo"));
        inputs.code = " 4821 Gato LUA ".into();
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Entrar").as_slice(),
            [Command::JoinCode { relay, code }]
                if relay == sync2gether::network::relay::DEFAULT_RELAY && code == " 4821 Gato LUA "
        ));
        // Enquanto a sala responde, o início continua na tela com o progresso no botão.
        view.role = Role::Guest;
        view.internet = true;
        let text = preparation_text_of(&ctx, &view, &mut inputs);
        assert_eq!(inputs.screen, ui::Screen::Home);
        assert!(text.contains("Entrando"));
        assert!(!text.contains("Conectando pela internet"));
        // A recusa mostra o erro sob o código digitado.
        view = View {
            error: Some("Código incorreto. Confira as palavras ou peça um código novo.".into()),
            ..Default::default()
        };
        let text = preparation_text_of(&ctx, &view, &mut inputs);
        assert_eq!(inputs.screen, ui::Screen::Home);
        assert_eq!(text.matches("Código incorreto").count(), 1);
        assert!(!text.contains("Entrando"));
        assert_eq!(inputs.code, " 4821 Gato LUA ");
    }
}

#[test]
fn code_field_accepts_enter_after_typing_in_the_same_frame() {
    use sync2gether::runtime::{Command, View};
    let ctx = egui::Context::default();
    let view = View::default();
    let mut inputs = ui::Inputs::default();
    preparation_text_of(&ctx, &view, &mut inputs);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("room_code")));
    let mut actions = Vec::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 800.0),
            )),
            events: vec![
                egui::Event::Text("4821-gato-lua".into()),
                key(egui::Key::Enter),
            ],
            ..Default::default()
        },
        |root| {
            actions = ui::draw_live(root, &view, &mut inputs, None);
        },
    );
    output.textures_delta.clear();
    assert!(
        matches!(actions.as_slice(), [Command::JoinCode { code, .. }] if code == "4821-gato-lua")
    );
}

#[test]
fn internet_room_invites_with_the_code_and_never_with_addresses() {
    use sync2gether::runtime::{Role, View};
    for size in [[480.0, 480.0], [720.0, 540.0], [960.0, 640.0]] {
        for light in [false, true] {
            let mut view = View {
                role: Role::Host,
                internet: true,
                room_address: Some("192.168.1.10:7842".parse().unwrap()),
                ..Default::default()
            };
            let text = preparation_frame(&view, size, light);
            assert!(text.contains("Gerando o código"));
            view.room_code = Some("4821-gato-lua".into());
            view.relay_offline = true;
            let text = preparation_frame(&view, size, light);
            assert!(text.contains("Envie este código"));
            assert!(text.contains("4821-gato-lua"));
            assert!(text.contains("Copiar"));
            assert!(text.contains("Sem contato com o servidor"));
            assert!(!text.contains("192.168.1.10"));
        }
    }
}

#[test]
fn advanced_network_settings_reveal_the_relay_and_validate_it() {
    let ctx = egui::Context::default();
    let view = sync2gether::runtime::View::default();
    let mut inputs = ui::Inputs::default();
    let _ = click_preparation_widget(&ctx, &view, &mut inputs, "Configurações");
    let text = preparation_text_of(&ctx, &view, &mut inputs);
    assert!(text.contains("Opções avançadas de rede"));
    assert!(!text.contains("Servidor das salas"));
    inputs.lan = true;
    inputs.relay = "relay inválido".into();
    let text = preparation_text_of(&ctx, &view, &mut inputs);
    assert!(text.contains("Servidor das salas pela internet"));
    assert!(text.contains("Informe o servidor"));
}

#[test]
fn mismatch_says_it_once_and_the_main_button_changes_the_local_file() {
    use sync2gether::runtime::{Command, Role, View};
    for light in [false, true] {
        let ctx = egui::Context::default();
        ui::configure_theme(&ctx, light);
        let mut inputs = ui::Inputs::default();
        let view = View {
            role: Role::Guest,
            connected: true,
            file: Some("Meu.mkv".into()),
            peer_file: Some("Outro.mp4".into()),
            verified: true,
            peer_verified: true,
            peer_ready: true,
            ..Default::default()
        };
        let text = preparation_frame(&view, [960.0, 800.0], light);
        assert!(text.contains("Os arquivos não batem"));
        assert_eq!(text.matches("Os arquivos são diferentes").count(), 1);
        // Nenhum cartão diz "Conferido" ao lado do aviso de diferença.
        assert!(!text.contains("Conferido"));
        assert!(!text.contains("Começar a assistir"));
        assert!(matches!(
            click_label(&ctx, &view, &mut inputs, "Trocar meu arquivo").as_slice(),
            [Command::Open]
        ));
    }
}

#[test]
fn host_room_shows_the_invite_until_someone_joins_then_the_film_step() {
    use sync2gether::runtime::{Role, View};
    let mut view = View {
        role: Role::Host,
        internet: true,
        room_code: Some("4821-gato-lua".into()),
        ..Default::default()
    };
    for size in [[480.0, 480.0], [960.0, 640.0]] {
        let text = preparation_frame(&view, size, false);
        assert!(text.contains("Esperando alguém entrar"));
        assert!(text.contains("Escolher meu filme"));
        assert!(!text.contains("Escolham o mesmo filme"));
        assert!(!text.contains("Começar a assistir"));
    }
    view.connected = true;
    view.peer_name = Some("Ana".into());
    let text = preparation_frame(&view, [960.0, 640.0], false);
    assert!(text.contains("Escolham o mesmo filme"));
    assert!(!text.contains("Esperando alguém entrar"));
    // A etapa atual e o código ficam no cabeçalho.
    assert!(text.contains("Filme") && text.contains("4821-gato-lua"));
}

/// Clica num controle pelo nome acessível e devolve os comandos do clique.
fn click_accessible(
    ctx: &egui::Context,
    view: &sync2gether::runtime::View,
    inputs: &mut ui::Inputs,
    label: &str,
    size: [f32; 2],
) -> Vec<sync2gether::runtime::Command> {
    ctx.enable_accesskit();
    let frame = |inputs: &mut ui::Inputs, events| {
        let mut actions = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                events,
                ..Default::default()
            },
            |root| actions = ui::draw_live(root, view, inputs, None),
        );
        (output, actions)
    };
    let mut position = None;
    for _ in 0..3 {
        let (mut output, actions) = frame(inputs, vec![]);
        assert!(actions.is_empty());
        for (_, node) in &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
        {
            if node.label() == Some(label) {
                let b = node.bounds().expect("accessible control must have bounds");
                position = Some(egui::pos2(
                    ((b.x0 + b.x1) * 0.5) as f32,
                    ((b.y0 + b.y1) * 0.5) as f32,
                ));
            }
        }
        output.textures_delta.clear();
    }
    let position = position.unwrap_or_else(|| panic!("accessible control not found: {label}"));
    let mut commands = Vec::new();
    for pressed in [true, false] {
        let (mut output, actions) = frame(
            inputs,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        output.textures_delta.clear();
        commands.extend(actions);
    }
    commands
}

#[test]
fn back_arrow_leaves_the_room_and_asks_first_when_someone_is_there() {
    use sync2gether::{
        player::Status,
        runtime::{Command, Role, View},
    };
    for light in [false, true] {
        for size in [[480.0, 480.0], [960.0, 640.0]] {
            let ctx = egui::Context::default();
            ui::configure_theme(&ctx, light);
            let mut inputs = ui::Inputs::default();
            // Sozinho na sala: a seta sai na hora.
            let mut view = View {
                role: Role::Host,
                internet: true,
                room_code: Some("4821-gato-lua".into()),
                ..Default::default()
            };
            assert!(matches!(
                click_accessible(&ctx, &view, &mut inputs, "Encerrar sala", size).as_slice(),
                [Command::Leave]
            ));
            // Com a outra pessoa conectada, pergunta antes de desconectá-la.
            view.connected = true;
            view.peer_name = Some("Ana".into());
            assert!(click_accessible(&ctx, &view, &mut inputs, "Encerrar sala", size).is_empty());
            assert!(inputs.confirm_leave);
            let text = preparation_text_of(&ctx, &view, &mut inputs);
            assert!(
                text.contains("Encerrar a sala?") && text.contains("a conexão com Ana termina")
            );
            assert!(
                click_label_at_size(&ctx, &view, &mut inputs, "Continuar na sala", size).is_empty()
            );
            assert!(!inputs.confirm_leave);
            // Esc faz o mesmo que a seta.
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                    events: vec![key(egui::Key::Escape)],
                    ..Default::default()
                },
                |root| assert!(ui::draw_live(root, &view, &mut inputs, None).is_empty()),
            );
            output.textures_delta.clear();
            assert!(inputs.confirm_leave);
            assert!(matches!(
                click_label_at_size(&ctx, &view, &mut inputs, "Encerrar", size).as_slice(),
                [Command::Leave]
            ));
            assert!(!inputs.confirm_leave);
            // O convidado vê "Sair da sala", também no player.
            view.role = Role::Guest;
            view.watching = true;
            view.player = Status {
                loaded: true,
                paused: true,
                duration: 120.0,
                ..Default::default()
            };
            assert!(click_accessible(&ctx, &view, &mut inputs, "Sair da sala", size).is_empty());
            let text = preparation_text_of(&ctx, &view, &mut inputs);
            assert!(text.contains("Sair da sala?"));
            assert!(matches!(
                click_label_at_size(&ctx, &view, &mut inputs, "Sair", size).as_slice(),
                [Command::Leave]
            ));
        }
    }
}
