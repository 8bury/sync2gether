//! Desenhos vetoriais da identidade visual, sem imagens ou I/O.
use super::{Palette, kit};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

/// Marca no cabeçalho: ícone do aplicativo e nome.
pub(super) fn brand(ui: &mut egui::Ui, colors: Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
    app_icon(ui.painter(), rect, colors);
    ui.label(
        super::strong(ui, "sync2gether")
            .size(17.0)
            .extra_letter_spacing(-0.2)
            .color(colors.text),
    );
}

/// Ícone do aplicativo: duas telas sobre um bloco com gradiente.
pub(super) fn app_icon(painter: &egui::Painter, rect: egui::Rect, colors: Palette) {
    let side = rect.width().min(rect.height());
    let tile = egui::Rect::from_center_size(rect.center(), Vec2::splat(side));
    let radius = (side * 0.28).round().clamp(4.0, 255.0) as u8;
    if side >= 48.0 {
        painter.add(
            egui::Shadow {
                offset: [0, (side * 0.12) as i8],
                blur: (side * 0.45) as u8,
                spread: 0,
                color: colors
                    .action
                    .gamma_multiply(if colors.dark { 0.45 } else { 0.32 }),
            }
            .as_shape(tile.shrink(side * 0.08), radius),
        );
    }
    kit::gradient_rect(painter, tile, radius, colors.action_top, colors.action);
    painter.rect_stroke(
        tile.shrink(0.5),
        radius,
        Stroke::new(1.0, Color32::from_white_alpha(36)),
        egui::StrokeKind::Inside,
    );
    let inner = tile.shrink(side * 0.2);
    screens(
        painter,
        inner,
        Color32::WHITE,
        kit::lerp(colors.action_top, colors.action, 0.55),
    );
}

pub(super) fn glow(painter: &egui::Painter, center: Pos2, radius: Vec2, color: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    const SEGMENTS: u32 = 64;
    for i in 0..=SEGMENTS {
        let angle = i as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        mesh.colored_vertex(
            center + egui::vec2(angle.cos() * radius.x, angle.sin() * radius.y),
            Color32::TRANSPARENT,
        );
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Marca com duas telas. Não representa mídia ou participantes reais.
pub(super) fn screens(painter: &egui::Painter, rect: egui::Rect, accent: Color32, fill: Color32) {
    let scale = (rect.width() / 120.0).min(rect.height() / 88.0);
    let c = rect.center();
    let stroke = Stroke::new((7.0 * scale).max(1.3), accent);
    for offset in [egui::vec2(-15.0, -10.0), egui::vec2(15.0, 10.0)] {
        let screen =
            egui::Rect::from_center_size(c + offset * scale, egui::vec2(76.0, 54.0) * scale);
        painter.rect_filled(screen, 10.0 * scale, fill);
        painter.rect_stroke(screen, 10.0 * scale, stroke, egui::StrokeKind::Inside);
    }
    let center = c + egui::vec2(16.0, 10.0) * scale;
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-8.0, -12.0) * scale,
            center + egui::vec2(12.0, 0.0) * scale,
            center + egui::vec2(-8.0, 12.0) * scale,
        ],
        accent,
        Stroke::NONE,
    ));
}

/// Fundo ilustrativo do player no modo demo; não é um quadro de vídeo.
pub(super) fn demo_video(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0, Color32::from_rgb(8, 10, 20));
    glow(
        painter,
        rect.center() + egui::vec2(-rect.width() * 0.18, -rect.height() * 0.1),
        rect.size() * egui::vec2(0.55, 0.65),
        Color32::from_rgba_unmultiplied(64, 84, 200, 70),
    );
    glow(
        painter,
        rect.center() + egui::vec2(rect.width() * 0.22, rect.height() * 0.12),
        rect.size() * egui::vec2(0.5, 0.6),
        Color32::from_rgba_unmultiplied(130, 70, 190, 46),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Vídeo fictício",
        egui::FontId::proportional(15.0),
        Color32::from_white_alpha(110),
    );
}

fn person(painter: &egui::Painter, center: Pos2, scale: f32, color: Color32) {
    painter.circle_stroke(
        center + egui::vec2(0.0, -5.0) * scale,
        4.5 * scale,
        Stroke::new(1.8 * scale, color),
    );
    painter.add(egui::Shape::line(
        (0..=14)
            .map(|i| {
                let a = std::f32::consts::PI + i as f32 / 14.0 * std::f32::consts::PI;
                center + egui::vec2(a.cos() * 8.0, 11.0 + a.sin() * 6.5) * scale
            })
            .collect(),
        Stroke::new(1.8 * scale, color),
    ));
}

/// Dois participantes ligados por pontos animados durante a conexão.
pub(super) fn connection(painter: &egui::Painter, rect: egui::Rect, colors: Palette, time: f64) {
    let scale = (rect.height() / 66.0).min(rect.width() / 190.0);
    let c = rect.center();
    let radius = 30.0 * scale;
    for offset in [-58.0, 58.0] {
        let center = c + egui::vec2(offset, 0.0) * scale;
        painter.circle_filled(center, radius, colors.surface);
        painter.circle_stroke(center, radius, Stroke::new(1.0, colors.border_strong));
        person(painter, center, scale * 1.1, colors.accent);
    }
    for (index, offset) in [-14.0f32, 0.0, 14.0].into_iter().enumerate() {
        let phase = ((time * 2.2 - index as f64 * 0.35).rem_euclid(1.0)) as f32;
        let pulse = (1.0 - (phase * 2.0 - 1.0).abs()).powf(1.5);
        painter.circle_filled(
            c + egui::vec2(offset, 0.0) * scale,
            (2.2 + pulse * 1.2) * scale,
            colors.accent.gamma_multiply(0.3 + 0.7 * pulse),
        );
    }
}

/// Ícone abstrato de arquivo; não usa miniaturas da mídia.
pub(super) fn file(ui: &mut egui::Ui, color: Color32, size: f32, tile: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if tile {
        ui.painter()
            .rect_filled(rect, (size * 0.28) as u8, color.gamma_multiply(0.12));
        ui.painter().rect_stroke(
            rect,
            (size * 0.28) as u8,
            Stroke::new(1.0, color.gamma_multiply(0.22)),
            egui::StrokeKind::Inside,
        );
    }
    let scale = size / 48.0;
    let c = rect.center();
    let offset = |x, y| c + egui::vec2(x, y) * scale;
    let stroke = Stroke::new(if tile { 1.6 } else { 2.0 }, color);
    ui.painter().add(egui::Shape::closed_line(
        vec![
            offset(-9.0, -13.0),
            offset(3.0, -13.0),
            offset(10.0, -6.0),
            offset(10.0, 13.0),
            offset(-9.0, 13.0),
        ],
        stroke,
    ));
    ui.painter().add(egui::Shape::line(
        vec![offset(3.0, -13.0), offset(3.0, -6.0), offset(10.0, -6.0)],
        stroke,
    ));
    // Pequeno triângulo de play: o arquivo é um vídeo.
    ui.painter().add(egui::Shape::convex_polygon(
        vec![offset(-3.0, 0.0), offset(4.0, 4.0), offset(-3.0, 8.0)],
        color,
        Stroke::NONE,
    ));
}
