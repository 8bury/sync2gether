use super::{Palette, strong};
use eframe::egui::{
    self, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, Vec2, epaint, vec2,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Primary,
    Secondary,
    Ghost,
    Link,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Large,
    Medium,
    Small,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Self::Large => 48.0,
            Self::Medium => 40.0,
            Self::Small => 32.0,
        }
    }
    fn font(self) -> f32 {
        match self {
            Self::Large => 15.0,
            Self::Medium => 14.5,
            Self::Small => 13.5,
        }
    }
    fn padding(self) -> f32 {
        match self {
            Self::Large => 22.0,
            Self::Medium => 16.0,
            Self::Small => 12.0,
        }
    }
    fn radius(self) -> u8 {
        match self {
            Self::Large => 12,
            Self::Medium => 10,
            Self::Small => 8,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glyph {
    Plus,
    ArrowLeft,
    ArrowRight,
    Play,
    Refresh,
    Gear,
    Check,
    Copy,
    Close,
    Alert,
    Info,
    People,
    Spinner,
    Folder,
}

const GLYPH: f32 = 16.0;
const GLYPH_GAP: f32 = 8.0;

pub(super) struct Button<'a> {
    label: &'a str,
    kind: Kind,
    size: Size,
    glyph: Option<Glyph>,
    width: Option<f32>,
    enabled: bool,
    busy: bool,
}

impl<'a> Button<'a> {
    pub fn new(kind: Kind, label: &'a str) -> Self {
        Self {
            label,
            kind,
            size: Size::Large,
            glyph: None,
            width: None,
            enabled: true,
            busy: false,
        }
    }
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = Some(glyph);
        self
    }
    pub fn maybe_glyph(mut self, glyph: Option<Glyph>) -> Self {
        self.glyph = glyph;
        self
    }
    pub fn fill_width(mut self, ui: &egui::Ui) -> Self {
        self.width = Some(ui.available_width());
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    fn text(&self, ui: &egui::Ui, color: Color32) -> egui::RichText {
        let text = if matches!(self.kind, Kind::Primary | Kind::Secondary | Kind::Link) {
            strong(ui, self.label)
        } else {
            egui::RichText::new(self.label)
        };
        text.size(self.size.font()).color(color)
    }

    pub fn natural_width(&self, ui: &egui::Ui) -> f32 {
        let galley = egui::WidgetText::from(self.text(ui, Color32::WHITE)).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        );
        galley.size().x + self.glyph_width() + 2.0 * self.padding()
    }

    fn glyph_width(&self) -> f32 {
        if self.glyph.is_some() || self.busy {
            GLYPH + GLYPH_GAP
        } else {
            0.0
        }
    }

    fn padding(&self) -> f32 {
        if matches!(self.kind, Kind::Link) {
            6.0
        } else {
            self.size.padding()
        }
    }

    pub fn show(self, ui: &mut egui::Ui, colors: Palette) -> egui::Response {
        let enabled = self.enabled && !self.busy && ui.is_enabled();
        let natural = self.natural_width(ui);
        let width = self
            .width
            .unwrap_or(natural)
            .min(ui.available_width().max(1.0));
        let height = self.size.height();
        let (rect, response) = ui.allocate_exact_size(
            vec2(width, height),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let label = self.label.to_owned();
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
        let response = if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        };
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            enabled && response.hovered(),
            0.12,
        );
        let pressed = enabled && response.is_pointer_button_down_on();
        let radius = self.size.radius();
        let painter = ui.painter();
        let (text_color, glyph_color) = match self.kind {
            Kind::Primary if enabled || self.busy => (Color32::WHITE, Color32::WHITE),
            Kind::Primary | Kind::Secondary if !enabled => (colors.muted, colors.faint),
            Kind::Primary | Kind::Secondary => (colors.text, colors.accent),
            Kind::Ghost if !enabled => (colors.faint, colors.faint),
            Kind::Ghost => {
                let color = lerp(colors.muted, colors.text, hover);
                (color, color)
            }
            Kind::Link if !enabled => (colors.faint, colors.faint),
            Kind::Link => {
                let color = lerp(colors.accent, colors.accent_strong, hover);
                (color, color)
            }
        };
        match self.kind {
            Kind::Primary if enabled || self.busy => {
                let glow = colors
                    .action
                    .gamma_multiply(if colors.dark { 0.38 } else { 0.30 });
                painter.add(
                    egui::Shadow {
                        offset: [0, if pressed { 2 } else { 6 }],
                        blur: if pressed { 8 } else { 18 },
                        spread: 0,
                        color: glow.gamma_multiply(0.7 + 0.3 * hover),
                    }
                    .as_shape(rect.shrink2(vec2(6.0, 4.0)), radius),
                );
                let lift = 0.08 * hover - if pressed { 0.08 } else { 0.0 };
                let top = lerp(colors.action_top, Color32::WHITE, lift.max(0.0));
                let bottom = lerp(colors.action, Color32::WHITE, lift.max(0.0));
                let (top, bottom) = if pressed {
                    (colors.action, colors.action)
                } else {
                    (top, bottom)
                };
                gradient_rect(painter, rect, radius, top, bottom);
                painter.rect_stroke(
                    rect.shrink(0.5),
                    radius,
                    Stroke::new(1.0, Color32::from_white_alpha(28)),
                    egui::StrokeKind::Inside,
                );
            }
            Kind::Primary | Kind::Secondary => {
                let fill = if enabled {
                    lerp(colors.raised, colors.hover, hover)
                } else {
                    colors.disabled
                };
                painter.rect(
                    rect,
                    radius,
                    fill,
                    Stroke::new(
                        1.0,
                        if enabled {
                            lerp(colors.border, colors.border_strong, hover)
                        } else {
                            colors.border
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
            }
            Kind::Ghost | Kind::Link => {
                if hover > 0.0 {
                    painter.rect_filled(rect, radius, colors.hover.gamma_multiply(hover));
                }
            }
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                radius + 2,
                Stroke::new(2.0, colors.accent.gamma_multiply(0.8)),
                egui::StrokeKind::Outside,
            );
        }
        let text_room = (width - 2.0 * self.padding() - self.glyph_width()).max(8.0);
        let galley = egui::WidgetText::from(self.text(ui, text_color)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            text_room,
            egui::TextStyle::Button,
        );
        let content = galley.size().x + self.glyph_width();
        let start = rect.center().x - content * 0.5;
        if self.busy {
            draw(
                painter,
                Glyph::Spinner,
                Pos2::new(start + GLYPH * 0.5, rect.center().y),
                GLYPH,
                glyph_color,
                ui.input(|i| i.time),
            );
            ui.ctx().request_repaint();
        } else if let Some(glyph) = self.glyph {
            draw(
                painter,
                glyph,
                Pos2::new(start + GLYPH * 0.5, rect.center().y),
                GLYPH,
                glyph_color,
                0.0,
            );
        }
        let text_pos = Pos2::new(
            start + self.glyph_width(),
            rect.center().y - galley.size().y * 0.5,
        );
        painter.galley(text_pos, galley, text_color);
        response
    }
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    colors: Palette,
    glyph: Glyph,
    label: &str,
    size: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 8, colors.hover.gamma_multiply(hover));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            9,
            Stroke::new(2.0, colors.accent.gamma_multiply(0.8)),
            egui::StrokeKind::Outside,
        );
    }
    draw(
        ui.painter(),
        glyph,
        rect.center(),
        GLYPH + 2.0,
        lerp(colors.muted, colors.text, hover),
        0.0,
    );
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn centered_row<R>(
    ui: &mut egui::Ui,
    widths: &[f32],
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let gap = ui.spacing().item_spacing.x;
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - total) * 0.5).max(0.0));
        add(ui)
    })
    .inner
}

pub(super) fn avatar(ui: &mut egui::Ui, colors: Palette, name: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_avatar(ui.painter(), colors, rect.center(), size, name);
}

pub(super) fn paint_avatar(
    painter: &egui::Painter,
    colors: Palette,
    center: Pos2,
    size: f32,
    name: &str,
) {
    let tints = [
        Color32::from_rgb(99, 132, 255),
        Color32::from_rgb(168, 112, 245),
        Color32::from_rgb(46, 182, 166),
        Color32::from_rgb(236, 128, 92),
        Color32::from_rgb(222, 98, 152),
    ];
    let hash = name
        .bytes()
        .fold(7u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    let tint = tints[hash as usize % tints.len()];
    painter.circle_filled(
        center,
        size * 0.5,
        tint.gamma_multiply(if colors.dark { 0.22 } else { 0.16 }),
    );
    painter.circle_stroke(
        center,
        size * 0.5 - 0.5,
        Stroke::new(1.0, tint.gamma_multiply(0.45)),
    );
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into());
    let color = if colors.dark {
        lerp(tint, Color32::WHITE, 0.45)
    } else {
        lerp(tint, Color32::BLACK, 0.25)
    };
    painter.text(
        center + vec2(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::new(size * 0.42, super::semibold_family(painter.ctx())),
        color,
    );
}

pub(super) fn status(ui: &mut egui::Ui, glyph: Option<Glyph>, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        match glyph {
            Some(glyph) => draw(
                ui.painter(),
                glyph,
                rect.center(),
                13.0,
                color,
                ui.input(|i| i.time),
            ),
            None => {
                ui.painter().circle_filled(rect.center(), 3.5, color);
            }
        }
        if glyph == Some(Glyph::Spinner) {
            ui.ctx().request_repaint();
        }
        ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).truncate())
            .on_hover_text(text);
    });
}

pub(super) fn progress(ui: &mut egui::Ui, colors: Palette, fraction: f32, width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let label = format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0));
        let bar = (width - 44.0).max(20.0);
        let (rect, _) = ui.allocate_exact_size(vec2(bar, 14.0), Sense::hover());
        let track = Rect::from_center_size(rect.center(), vec2(bar, 6.0));
        ui.painter().rect_filled(track, 3, colors.hover);
        let mut fill = track;
        fill.set_width((bar * fraction.clamp(0.0, 1.0)).max(6.0));
        gradient_rect_h(ui.painter(), fill, 3, colors.action, colors.accent);
        ui.label(egui::RichText::new(label).size(12.5).color(colors.muted));
    });
}

pub(crate) fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

fn rounded_points(rect: Rect, radius: u8) -> Vec<Pos2> {
    let mut points = Vec::new();
    epaint::tessellator::path::rounded_rectangle(
        &mut points,
        rect,
        CornerRadius::same(radius).into(),
    );
    points
}

pub(super) fn gradient_rect(
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    top: Color32,
    bottom: Color32,
) {
    gradient_mesh(painter, rect, radius, |p| {
        lerp(top, bottom, (p.y - rect.top()) / rect.height().max(1.0))
    });
}

pub(super) fn gradient_rect_h(
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    left: Color32,
    right: Color32,
) {
    gradient_mesh(painter, rect, radius, |p| {
        lerp(left, right, (p.x - rect.left()) / rect.width().max(1.0))
    });
}

fn gradient_mesh(painter: &egui::Painter, rect: Rect, radius: u8, color: impl Fn(Pos2) -> Color32) {
    let points = rounded_points(rect, radius);
    if points.len() < 3 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), color(rect.center()));
    for &p in &points {
        mesh.colored_vertex(p, color(p));
    }
    let n = points.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::closed_line(
        points,
        Stroke::new(1.0, color(rect.center())),
    ));
}

pub(crate) fn draw(
    painter: &egui::Painter,
    glyph: Glyph,
    center: Pos2,
    size: f32,
    color: Color32,
    time: f64,
) {
    let s = size / 16.0;
    let p = |x: f32, y: f32| center + vec2(x, y) * s;
    let stroke = Stroke::new((1.6 * s).max(1.2), color);
    match glyph {
        Glyph::Plus => {
            painter.line_segment([p(-6.0, 0.0), p(6.0, 0.0)], stroke);
            painter.line_segment([p(0.0, -6.0), p(0.0, 6.0)], stroke);
        }
        Glyph::ArrowLeft | Glyph::ArrowRight => {
            let d = if glyph == Glyph::ArrowLeft { -1.0 } else { 1.0 };
            painter.line_segment([p(-6.0 * d, 0.0), p(6.0 * d, 0.0)], stroke);
            painter.add(Shape::line(
                vec![p(1.0 * d, -5.0), p(6.0 * d, 0.0), p(1.0 * d, 5.0)],
                stroke,
            ));
        }
        Glyph::Play => {
            painter.add(Shape::convex_polygon(
                vec![p(-4.0, -6.5), p(6.5, 0.0), p(-4.0, 6.5)],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Refresh => {
            let arc: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -0.6 + i as f32 / 20.0 * 4.9;
                    p(a.cos() * 6.0, a.sin() * 6.0)
                })
                .collect();
            let end = *arc.last().unwrap_or(&center);
            painter.add(Shape::line(arc, stroke));
            let a: f32 = -0.6 + 4.9;
            let tangent = vec2(-a.sin(), a.cos());
            let normal = vec2(a.cos(), a.sin());
            painter.add(Shape::convex_polygon(
                vec![
                    end + tangent * 3.0 * s,
                    end - normal * 3.2 * s - tangent * 1.0 * s,
                    end + normal * 3.2 * s - tangent * 1.0 * s,
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0 + std::f32::consts::FRAC_PI_8;
                let dir = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [center + dir * 5.0 * s, center + dir * 7.2 * s],
                    Stroke::new(2.2 * s, color),
                );
            }
            painter.circle_stroke(center, 5.0 * s, stroke);
            painter.circle_stroke(center, 1.9 * s, stroke);
        }
        Glyph::Check => {
            painter.add(Shape::line(
                vec![p(-5.5, 0.5), p(-1.5, 4.5), p(6.0, -4.5)],
                Stroke::new(stroke.width * 1.15, color),
            ));
        }
        Glyph::Copy => {
            painter.rect_stroke(
                Rect::from_min_size(p(-3.0, -3.0), vec2(9.0, 10.0) * s),
                2.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.add(Shape::line(
                vec![p(-6.0, 3.5), p(-6.0, -6.5), p(2.5, -6.5)],
                stroke,
            ));
        }
        Glyph::Close => {
            painter.line_segment([p(-5.0, -5.0), p(5.0, 5.0)], stroke);
            painter.line_segment([p(5.0, -5.0), p(-5.0, 5.0)], stroke);
        }
        Glyph::Alert | Glyph::Info => {
            painter.circle_stroke(center, 7.0 * s, stroke);
            let (a, b, dot) = if glyph == Glyph::Alert {
                (p(0.0, -3.5), p(0.0, 1.0), p(0.0, 3.8))
            } else {
                (p(0.0, -0.5), p(0.0, 3.8), p(0.0, -3.4))
            };
            painter.line_segment([a, b], stroke);
            painter.circle_filled(dot, 1.1 * s, color);
        }
        Glyph::People => {
            for (x, alpha) in [(-3.5, 1.0), (3.5, 0.55)] {
                let c = p(x, 0.0);
                let col = color.gamma_multiply(alpha);
                painter.circle_stroke(
                    c + vec2(0.0, -3.2) * s,
                    2.6 * s,
                    Stroke::new(stroke.width, col),
                );
                painter.add(Shape::line(
                    (0..=10)
                        .map(|i| {
                            let a = std::f32::consts::PI + i as f32 / 10.0 * std::f32::consts::PI;
                            c + vec2(a.cos() * 4.6, 6.2 + a.sin() * 4.0) * s
                        })
                        .collect(),
                    Stroke::new(stroke.width, col),
                ));
            }
        }
        Glyph::Folder => {
            painter.add(Shape::closed_line(
                vec![
                    p(-7.0, -5.5),
                    p(-2.5, -5.5),
                    p(-0.5, -3.5),
                    p(6.0, -3.5),
                    p(6.0, 5.5),
                    p(-7.0, 5.5),
                ],
                stroke,
            ));
            painter.line_segment([p(-7.0, -0.5), p(6.0, -0.5)], stroke);
        }
        Glyph::Spinner => {
            let start = (time * 5.0) as f32;
            let arc: Vec<Pos2> = (0..=18)
                .map(|i| {
                    let a = start + i as f32 / 18.0 * 4.4;
                    p(a.cos() * 6.0, a.sin() * 6.0)
                })
                .collect();
            painter.circle_stroke(
                center,
                6.0 * s,
                Stroke::new(stroke.width, color.gamma_multiply(0.22)),
            );
            painter.add(Shape::line(arc, stroke));
        }
    }
}
