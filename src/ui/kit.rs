use super::{Palette, strong};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2, vec2};
use sync2gether_core::theme as tokens;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Primary,
    Secondary,
    Ghost,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    Large,
    Medium,
    Small,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Self::Large => f32::from(tokens::BUTTON_LARGE_H_PX),
            Self::Medium => f32::from(tokens::BUTTON_MEDIUM_H_PX),
            Self::Small => f32::from(tokens::BUTTON_SMALL_H_PX),
        }
    }
    fn font(self) -> f32 {
        match self {
            Self::Large => 16.0,
            Self::Medium => f32::from(tokens::TEXT_BODY_PX),
            Self::Small => 14.0,
        }
    }
    fn padding(self) -> f32 {
        match self {
            Self::Large => 24.0,
            Self::Medium => 18.0,
            Self::Small => 10.0,
        }
    }
    fn radius(self) -> u8 {
        match self {
            Self::Large | Self::Medium => tokens::RADIUS_CONTROL_PX,
            Self::Small => tokens::RADIUS_SMALL_PX,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glyph {
    ArrowLeft,
    ArrowRight,
    ChevronLeft,
    Refresh,
    Gear,
    Check,
    Copy,
    Close,
    Alert,
    Info,
    Spinner,
    Folder,
}

const GLYPH: f32 = 16.0;
const GLYPH_GAP: f32 = 8.0;

pub(crate) struct Button<'a> {
    label: &'a str,
    kind: Kind,
    size: Size,
    glyph: Option<Glyph>,
    trailing: Option<Glyph>,
    spread: bool,
    height: Option<f32>,
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
            trailing: None,
            spread: false,
            height: None,
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
    pub fn trailing(mut self, glyph: Glyph) -> Self {
        self.trailing = Some(glyph);
        self
    }
    pub fn spread(mut self) -> Self {
        self.spread = true;
        self
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
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
        let text = if matches!(self.kind, Kind::Primary | Kind::Secondary) {
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
        galley.size().x + self.glyph_width() + self.trailing_width() + 2.0 * self.size.padding()
    }

    fn glyph_width(&self) -> f32 {
        if self.glyph.is_some() || self.busy {
            GLYPH + GLYPH_GAP
        } else {
            0.0
        }
    }

    fn trailing_width(&self) -> f32 {
        if self.trailing.is_some() && !self.busy {
            GLYPH + GLYPH_GAP
        } else {
            0.0
        }
    }

    pub fn show(self, ui: &mut egui::Ui, colors: Palette) -> egui::Response {
        let enabled = self.enabled && !self.busy && ui.is_enabled();
        let natural = self.natural_width(ui);
        let width = self
            .width
            .unwrap_or(natural)
            .min(ui.available_width().max(1.0));
        let height = self.height.unwrap_or(self.size.height());
        let (full_rect, response) = ui.allocate_exact_size(
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
        if !ui.is_rect_visible(full_rect) {
            return response;
        }
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            enabled && response.hovered(),
            0.15,
        );
        let pressed = enabled && response.is_pointer_button_down_on();
        let rect = if pressed && self.kind != Kind::Ghost {
            full_rect.shrink2(full_rect.size() * 0.0075)
        } else {
            full_rect
        };
        let radius = self.size.radius();
        let painter = ui.painter();
        let solid = self.kind == Kind::Primary && (enabled || self.busy);
        let (text_color, glyph_color) = match self.kind {
            Kind::Primary if solid => (colors.action_ink, colors.action_ink),
            Kind::Primary | Kind::Secondary if !enabled => (colors.faint, colors.faint),
            Kind::Primary | Kind::Secondary => (colors.text, colors.text),
            Kind::Ghost if !enabled => (colors.faint, colors.faint),
            Kind::Ghost => {
                let color = lerp(colors.muted, colors.text, hover);
                (color, color)
            }
        };
        match self.kind {
            Kind::Primary if solid => {
                painter.rect_filled(
                    rect,
                    radius,
                    lerp(colors.action, colors.action_hover, hover),
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
                    Stroke::new(1.0, colors.border),
                    egui::StrokeKind::Inside,
                );
            }
            Kind::Ghost => {
                if hover > 0.0 {
                    painter.rect_filled(rect, radius, colors.hover.gamma_multiply(hover));
                }
            }
        }
        if response.has_focus() {
            painter.rect_stroke(
                full_rect.expand(3.0),
                radius + 3,
                Stroke::new(2.0, colors.text),
                egui::StrokeKind::Outside,
            );
        }
        let padding = self.size.padding();
        let glyph_width = self.glyph_width();
        let trailing_width = self.trailing_width();
        let text_room = (width - 2.0 * padding - glyph_width - trailing_width).max(8.0);
        let galley = egui::WidgetText::from(self.text(ui, text_color)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            text_room,
            egui::TextStyle::Button,
        );
        let content = galley.size().x + glyph_width + trailing_width;
        let start = if self.spread {
            rect.left() + padding
        } else {
            rect.center().x - content * 0.5
        };
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
        let text_x = start + glyph_width;
        let text_end = text_x + galley.size().x;
        let text_pos = Pos2::new(text_x, rect.center().y - galley.size().y * 0.5);
        painter.galley(text_pos, galley, text_color);
        if let Some(trailing) = self.trailing.filter(|_| !self.busy) {
            let x = if self.spread {
                rect.right() - padding - GLYPH * 0.5
            } else {
                text_end + GLYPH_GAP + GLYPH * 0.5
            };
            draw(
                painter,
                trailing,
                Pos2::new(x, rect.center().y),
                GLYPH,
                glyph_color,
                0.0,
            );
        }
        response
    }
}

pub(super) fn row(
    ui: &mut egui::Ui,
    colors: Palette,
    title: &str,
    detail: &str,
    compact: bool,
    enabled: bool,
) -> egui::Response {
    let height = if compact { 58.0 } else { 78.0 };
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title));
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        enabled && response.hovered(),
        0.2,
    );
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1.0, colors.border),
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.shrink(1.0),
            4,
            Stroke::new(2.0, colors.text),
            egui::StrokeKind::Inside,
        );
    }
    let left = rect.left() + 4.0 + 6.0 * hover;
    let arrow_x = rect.right() - 12.0 + 4.0 * hover;
    let room = (arrow_x - left - 20.0).max(20.0);
    let ink = if enabled { colors.text } else { colors.faint };
    let title_galley = single_line(
        ui,
        strong(ui, title).size(15.0).color(ink),
        room,
        egui::TextStyle::Body,
    );
    let detail_galley = single_line(
        ui,
        egui::RichText::new(detail).size(13.0).color(colors.faint),
        room,
        egui::TextStyle::Small,
    );
    let total = title_galley.size().y + 3.0 + detail_galley.size().y;
    let top = rect.center().y - total * 0.5;
    let detail_y = top + title_galley.size().y + 3.0;
    painter.galley(Pos2::new(left, top), title_galley, ink);
    painter.galley(Pos2::new(left, detail_y), detail_galley, colors.faint);
    draw(
        painter,
        Glyph::ArrowRight,
        Pos2::new(arrow_x, rect.center().y),
        15.0,
        lerp(colors.muted, colors.text, hover),
        0.0,
    );
    response
}

pub(super) fn rule(ui: &mut egui::Ui, colors: Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, colors.border),
    );
}

pub(crate) fn single_line(
    ui: &egui::Ui,
    text: egui::RichText,
    max_width: f32,
    style: egui::TextStyle,
) -> std::sync::Arc<egui::Galley> {
    egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width.max(1.0),
        style,
    )
}

pub(super) fn fitted_size(
    ui: &egui::Ui,
    text: &str,
    target: f32,
    max_width: f32,
    build: impl Fn(&str, f32) -> egui::RichText,
) -> f32 {
    let width = egui::WidgetText::from(build(text, target))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x;
    if width > max_width && width > 0.0 {
        (target * max_width / width).floor().max(12.0)
    } else {
        target
    }
}

pub(crate) fn icon_button(
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
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.15);
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 6, colors.hover.gamma_multiply(hover));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            8,
            Stroke::new(2.0, colors.text),
            egui::StrokeKind::Outside,
        );
    }
    draw(
        ui.painter(),
        glyph,
        rect.center(),
        GLYPH + 1.0,
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
                ui.painter().circle_filled(rect.center(), 3.0, color);
            }
        }
        if glyph == Some(Glyph::Spinner) {
            ui.ctx().request_repaint();
        }
        ui.add(egui::Label::new(egui::RichText::new(text).size(13.0).color(color)).truncate())
            .on_hover_text(text);
    });
}

pub(super) fn progress(ui: &mut egui::Ui, colors: Palette, fraction: f32, width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let label = format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0));
        let bar = (width - 44.0).max(20.0);
        let (rect, _) = ui.allocate_exact_size(vec2(bar, 14.0), Sense::hover());
        let track = Rect::from_center_size(rect.center(), vec2(bar, 2.0));
        ui.painter().rect_filled(track, 0, colors.border);
        let mut fill = track;
        fill.set_width(bar * fraction.clamp(0.0, 1.0));
        ui.painter().rect_filled(fill, 0, colors.text);
        ui.label(egui::RichText::new(label).size(13.0).color(colors.muted));
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
    let stroke = Stroke::new((1.5 * s).max(1.2), color);
    match glyph {
        Glyph::ArrowLeft | Glyph::ArrowRight => {
            let d = if glyph == Glyph::ArrowLeft { -1.0 } else { 1.0 };
            painter.line_segment([p(-6.0 * d, 0.0), p(6.0 * d, 0.0)], stroke);
            painter.add(Shape::line(
                vec![p(1.0 * d, -5.0), p(6.0 * d, 0.0), p(1.0 * d, 5.0)],
                stroke,
            ));
        }
        Glyph::ChevronLeft => {
            painter.add(Shape::line(
                vec![p(2.5, -5.5), p(-3.0, 0.0), p(2.5, 5.5)],
                stroke,
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
