//! Desenhos vetoriais da identidade visual, sem imagens ou I/O.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

pub(super) fn brand(ui: &mut egui::Ui, text: Color32, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    let c = rect.center();
    for (offset, color) in [(-5.0, accent.gamma_multiply(0.45)), (3.0, accent)] {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(offset - 6.0, -9.0),
                c + egui::vec2(offset + 7.0, 0.0),
                c + egui::vec2(offset - 6.0, 9.0),
            ],
            color,
            Stroke::NONE,
        ));
    }
    ui.label(
        egui::RichText::new("sync2gether")
            .size(21.0)
            .strong()
            .color(text),
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

/// Duas telas abstratas. Não representam salas, filmes ou participantes reais.
pub(super) fn cinema(painter: &egui::Painter, rect: egui::Rect, dark: bool) {
    let c = rect.center();
    let scale = (rect.width() / 340.0).min(rect.height() / 270.0);
    let blue = Color32::from_rgb(104, 145, 255);
    glow(
        painter,
        c,
        egui::vec2(230.0, 220.0) * scale,
        Color32::from_rgba_unmultiplied(49, 86, 210, if dark { 45 } else { 20 }),
    );
    for (offset, opacity) in [(-24.0, 0.35), (24.0, 0.8)] {
        let screen = egui::Rect::from_center_size(
            c + egui::vec2(offset, -offset * 0.7) * scale,
            egui::vec2(252.0, 155.0) * scale,
        );
        painter.rect_filled(
            screen,
            14,
            if dark {
                Color32::from_rgba_unmultiplied(21, 32, 59, 170)
            } else {
                Color32::from_white_alpha(160)
            },
        );
        painter.rect_stroke(
            screen,
            14,
            Stroke::new(1.0, blue.gamma_multiply(opacity)),
            egui::StrokeKind::Inside,
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                screen.center() + egui::vec2(-8.0, -15.0) * scale,
                screen.center() + egui::vec2(16.0, 0.0) * scale,
                screen.center() + egui::vec2(-8.0, 15.0) * scale,
            ],
            blue.gamma_multiply(opacity),
            Stroke::NONE,
        ));
    }
}

pub(super) fn film(ui: &mut egui::Ui, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 52.0), egui::Sense::hover());
    let rect = rect.shrink2(egui::vec2(5.0, 6.0));
    let painter = ui.painter();
    painter.rect_filled(rect, 6, accent.gamma_multiply(0.12));
    painter.rect_stroke(
        rect,
        6,
        Stroke::new(1.0, accent.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    for y in [rect.top() + 8.0, rect.bottom() - 8.0] {
        for x in [rect.left() + 6.0, rect.right() - 6.0] {
            painter.rect_filled(
                egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(3.0, 4.0)),
                1,
                accent,
            );
        }
    }
    let c = rect.center();
    painter.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(-3.0, -5.0),
            c + egui::vec2(5.0, 0.0),
            c + egui::vec2(-3.0, 5.0),
        ],
        accent,
        Stroke::NONE,
    ));
}
