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
