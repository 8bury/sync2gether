use super::{Palette, strong};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Together,
    Solo,
    Waiting,
    Apart,
    Lost,
}

impl Presence {
    fn span(self) -> f32 {
        match self {
            Self::Together => 1.6,
            Self::Solo => 1.0,
            Self::Waiting | Self::Lost => 2.25,
            Self::Apart => 2.6,
        }
    }

    fn second_offset(self) -> Option<f32> {
        match self {
            Self::Together => Some(0.6),
            Self::Solo => None,
            Self::Waiting | Self::Lost => Some(1.25),
            Self::Apart => Some(1.6),
        }
    }

    fn breath_period(self) -> Option<f64> {
        match self {
            Self::Waiting => Some(1.9),
            Self::Lost => Some(1.2),
            _ => None,
        }
    }
}

pub(crate) fn pair_width(state: Presence, diameter: f32) -> f32 {
    diameter * state.span()
}

pub(crate) fn paint_pair(
    painter: &egui::Painter,
    left_center: Pos2,
    diameter: f32,
    state: Presence,
    colors: Palette,
) {
    let radius = diameter * 0.5;
    let first = left_center + egui::vec2(radius, 0.0);
    let first_color = if state == Presence::Together {
        colors.text.gamma_multiply(0.62)
    } else {
        colors.text
    };
    painter.circle_filled(first, radius, first_color);
    let Some(offset) = state.second_offset() else {
        return;
    };
    let second = first + egui::vec2(diameter * offset, 0.0);
    match state {
        Presence::Together => {
            painter.circle_filled(second, radius, colors.text.gamma_multiply(0.62));
        }
        _ => {
            let ring = if state == Presence::Waiting {
                colors.faint
            } else {
                colors.warning
            };
            let alpha = state.breath_period().map_or(1.0, |period| {
                let time = painter.ctx().input(|input| input.time);
                painter
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
                let wave = (time / period * std::f64::consts::TAU).cos() as f32;
                0.65 + 0.35 * wave
            });
            painter.circle_stroke(
                second,
                radius - 0.75,
                Stroke::new(1.5, ring.gamma_multiply(alpha)),
            );
        }
    }
}

pub(crate) fn pair(ui: &mut egui::Ui, colors: Palette, state: Presence, diameter: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(pair_width(state, diameter), diameter),
        egui::Sense::hover(),
    );
    paint_pair(ui.painter(), rect.left_center(), diameter, state, colors);
}

pub(super) fn brand(ui: &mut egui::Ui, colors: Palette) {
    ui.spacing_mut().item_spacing.x = 9.0;
    pair(ui, colors, Presence::Together, 15.0);
    ui.label(
        strong(ui, "sync2gether")
            .size(15.0)
            .extra_letter_spacing(-0.15)
            .color(colors.text),
    );
}

pub(super) fn demo_video(painter: &egui::Painter, rect: egui::Rect, label: &str) {
    painter.rect_filled(rect, 0, Color32::from_rgb(26, 26, 26));
    painter.text(
        rect.center() - egui::vec2(0.0, rect.height() * 0.3),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(14.0),
        Color32::from_white_alpha(70),
    );
}

pub(super) fn file_glyph(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let scale = size / 48.0;
    let offset = |x: f32, y: f32| center + Vec2::new(x, y) * scale;
    let stroke = Stroke::new(1.5, color);
    painter.add(egui::Shape::closed_line(
        vec![
            offset(-13.0, -18.0),
            offset(5.0, -18.0),
            offset(14.0, -9.0),
            offset(14.0, 18.0),
            offset(-13.0, 18.0),
        ],
        stroke,
    ));
    painter.add(egui::Shape::line(
        vec![offset(5.0, -18.0), offset(5.0, -9.0), offset(14.0, -9.0)],
        stroke,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![offset(-4.0, -1.0), offset(6.0, 5.0), offset(-4.0, 11.0)],
        color,
        Stroke::NONE,
    ));
}
