use crate::ui::kit::{self, Glyph};
use eframe::egui::{self, Color32, Pos2, Stroke, vec2};

const ICON_SIZE: f32 = sync2gether_core::theme::PLAYER_ICON_PX as f32;
const INK: Color32 = Color32::from_rgb(
    sync2gether_core::theme::PLAYER_INK.0,
    sync2gether_core::theme::PLAYER_INK.1,
    sync2gether_core::theme::PLAYER_INK.2,
);

pub(super) enum Icon {
    Play,
    Pause,
    Cancel,
    Back,
    Forward,
    Volume(bool),
    Captions,
    Fullscreen,
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    enabled: bool,
    active: bool,
    tooltip: &str,
) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2(ICON_SIZE, ICON_SIZE)),
        )
        .on_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tooltip));
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let center = response.rect.center();
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered() && enabled || response.has_focus(),
        0.15,
    );
    let wash = hover.max(if active { 1.0 } else { 0.0 });
    if wash > 0.0 {
        ui.painter().rect_filled(
            response.rect,
            8,
            Color32::from_white_alpha((26.0 * wash) as u8),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            8,
            Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    let color = if !enabled {
        INK.gamma_multiply(0.36)
    } else if hover > 0.0 || active {
        Color32::WHITE
    } else {
        INK
    };
    paint(ui.painter(), icon, center, color);
    response
}

fn paint(painter: &egui::Painter, icon: Icon, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    match icon {
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + vec2(-6.0, -9.0),
                    c + vec2(9.0, 0.0),
                    c + vec2(-6.0, 9.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [-5.0, 5.0] {
                painter.rect_filled(
                    egui::Rect::from_center_size(c + vec2(x, 0.0), vec2(5.0, 17.0)),
                    1.0,
                    color,
                );
            }
        }
        Icon::Cancel => kit::draw(painter, Glyph::Close, c, 16.0, color, 0.0),
        Icon::Back => rotate(painter, c, -1.0, stroke),
        Icon::Forward => rotate(painter, c, 1.0, stroke),
        Icon::Volume(muted) => volume(painter, c, muted, stroke),
        Icon::Captions => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, vec2(19.0, 15.0)),
                3,
                stroke,
                egui::StrokeKind::Middle,
            );
            for (y, left, right) in [(-1.5, -5.5, -1.5), (-1.5, 1.5, 5.5), (2.5, -5.5, 5.5)] {
                painter.line_segment([c + vec2(left, y), c + vec2(right, y)], stroke);
            }
        }
        Icon::Fullscreen => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = c + vec2(x * 8.0, y * 8.0);
                painter.add(egui::Shape::line(
                    vec![
                        corner - vec2(x * 4.5, 0.0),
                        corner,
                        corner - vec2(0.0, y * 4.5),
                    ],
                    stroke,
                ));
            }
        }
    }
}

fn rotate(painter: &egui::Painter, c: Pos2, direction: f32, stroke: Stroke) {
    let point = |x: f32, y: f32| c + vec2(-direction * x, y) * 0.78;
    let mut arc: Vec<Pos2> = (0..=36)
        .map(|i| {
            let angle = std::f32::consts::PI - i as f32 / 36.0 * 1.75 * std::f32::consts::PI;
            point(9.0 * angle.cos(), 9.0 * angle.sin())
        })
        .collect();
    arc.push(point(-9.0, -4.0));
    painter.add(egui::Shape::line(arc, stroke));
    painter.add(egui::Shape::line(
        vec![point(-9.0, -9.5), point(-9.0, -4.0), point(-3.5, -4.0)],
        stroke,
    ));
}

fn volume(painter: &egui::Painter, c: Pos2, muted: bool, stroke: Stroke) {
    let base = c + vec2(-3.0, 0.0);
    painter.add(egui::Shape::closed_line(
        vec![
            base + vec2(-5.5, -2.8),
            base + vec2(-2.0, -2.8),
            base + vec2(2.5, -7.0),
            base + vec2(2.5, 7.0),
            base + vec2(-2.0, 2.8),
            base + vec2(-5.5, 2.8),
        ],
        stroke,
    ));
    if muted {
        for y in [-1.0, 1.0] {
            painter.line_segment([c + vec2(3.5, y * 3.5), c + vec2(9.5, -y * 3.5)], stroke);
        }
        return;
    }
    for radius in [6.0, 10.0] {
        let arc = (0..=12)
            .map(|i| {
                let angle = -0.75 + i as f32 / 12.0 * 1.5;
                base + vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(arc, stroke));
    }
}
