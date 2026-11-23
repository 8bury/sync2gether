use super::kit;
use eframe::egui::{self, Color32, RichText, Stroke};

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    pub(crate) background: Color32,
    pub(crate) surface: Color32,
    pub(crate) inset: Color32,
    pub(crate) raised: Color32,
    pub(crate) hover: Color32,
    pub(crate) disabled: Color32,
    pub(crate) action: Color32,
    pub(crate) action_top: Color32,
    pub(crate) border: Color32,
    pub(crate) border_strong: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) faint: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_strong: Color32,
    pub(crate) success: Color32,
    pub(crate) warning: Color32,
    pub(crate) error: Color32,
}

impl Palette {
    pub(crate) fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                background: Color32::from_rgb(13, 14, 18),
                surface: Color32::from_rgb(21, 22, 28),
                inset: Color32::from_rgb(17, 18, 23),
                raised: Color32::from_rgb(29, 31, 38),
                hover: Color32::from_rgb(37, 39, 48),
                disabled: Color32::from_rgb(25, 27, 33),
                action: Color32::from_rgb(70, 104, 240),
                action_top: Color32::from_rgb(101, 133, 255),
                border: Color32::from_rgb(38, 40, 49),
                border_strong: Color32::from_rgb(60, 63, 76),
                text: Color32::from_rgb(240, 242, 247),
                muted: Color32::from_rgb(150, 155, 170),
                faint: Color32::from_rgb(98, 103, 118),
                accent: Color32::from_rgb(122, 156, 255),
                accent_strong: Color32::from_rgb(160, 184, 255),
                success: Color32::from_rgb(92, 214, 164),
                warning: Color32::from_rgb(244, 192, 104),
                error: Color32::from_rgb(255, 134, 134),
            }
        } else {
            Self {
                dark,
                background: Color32::from_rgb(246, 247, 250),
                surface: Color32::WHITE,
                inset: Color32::from_rgb(243, 244, 248),
                raised: Color32::WHITE,
                hover: Color32::from_rgb(236, 238, 244),
                disabled: Color32::from_rgb(240, 241, 245),
                action: Color32::from_rgb(60, 92, 228),
                action_top: Color32::from_rgb(88, 120, 248),
                border: Color32::from_rgb(226, 229, 236),
                border_strong: Color32::from_rgb(200, 205, 216),
                text: Color32::from_rgb(17, 19, 27),
                muted: Color32::from_rgb(96, 102, 118),
                faint: Color32::from_rgb(150, 155, 168),
                accent: Color32::from_rgb(58, 90, 226),
                accent_strong: Color32::from_rgb(36, 64, 190),
                success: Color32::from_rgb(16, 128, 88),
                warning: Color32::from_rgb(164, 100, 8),
                error: Color32::from_rgb(200, 48, 62),
            }
        }
    }
}

pub fn configure_theme(ctx: &egui::Context, light: bool) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        (
            "sans",
            include_bytes!("../../assets/fonts/Inter-Regular.ttf").as_slice(),
        ),
        (
            "sans-semibold",
            include_bytes!("../../assets/fonts/Inter-SemiBold.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(data).into());
    }
    let proportional = fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap();
    proportional.insert(0, "sans".into());
    let mut semibold = proportional.clone();
    semibold.insert(0, "sans-semibold".into());
    fonts
        .families
        .insert(egui::FontFamily::Name("sans-semibold".into()), semibold);
    ctx.set_fonts(fonts);
    let colors = Palette::new(!light);
    let mut visuals = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::new(1.0, colors.border);
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.menu_corner_radius = egui::CornerRadius::same(12);
    let shadow = Color32::from_black_alpha(if light { 28 } else { 110 });
    visuals.window_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: shadow,
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow,
    };
    visuals.extreme_bg_color = colors.inset;
    visuals.text_edit_bg_color = Some(colors.inset);
    visuals.faint_bg_color = colors.surface;
    visuals.code_bg_color = colors.inset;
    visuals.override_text_color = Some(colors.text);
    visuals.hyperlink_color = colors.accent;
    visuals.weak_text_color = Some(colors.muted);
    visuals.selection.bg_fill = colors
        .accent
        .gamma_multiply(if light { 0.16 } else { 0.24 });
    visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    visuals.text_cursor.stroke = Stroke::new(2.0, colors.accent);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.active,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
        widget.bg_fill = colors.raised;
        widget.weak_bg_fill = colors.raised;
        widget.bg_stroke = Stroke::new(1.0, colors.border);
        widget.fg_stroke = Stroke::new(1.5, colors.text);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_fill = colors.surface;
    visuals.widgets.noninteractive.weak_bg_fill = colors.surface;
    visuals.widgets.noninteractive.fg_stroke.color = colors.muted;
    visuals.widgets.hovered.bg_fill = colors.hover;
    visuals.widgets.hovered.weak_bg_fill = colors.hover;
    visuals.widgets.hovered.bg_stroke.color = colors.border_strong;
    visuals.widgets.active.bg_fill = colors.hover;
    visuals.widgets.active.weak_bg_fill = colors.hover;
    visuals.widgets.active.bg_stroke.color = colors.accent;
    visuals.widgets.open.weak_bg_fill = colors.hover;
    visuals.widgets.open.bg_stroke.color = colors.border_strong;
    ctx.set_visuals(visuals);
    ctx.style_mut_of(
        if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        },
        |style| {
            style.spacing.item_spacing = egui::vec2(12.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
            style.spacing.interact_size.y = 34.0;
            style.spacing.menu_margin = egui::Margin::same(10);
            style.spacing.menu_spacing = 4.0;
            style.spacing.scroll = egui::style::ScrollStyle::floating();
            style.animation_time = 0.14;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        },
    );
}

pub(super) fn card(colors: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(16)
        .inner_margin(24)
        .shadow(egui::Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if colors.dark { 40 } else { 10 }),
        })
}

pub(super) fn semibold_family(ctx: &egui::Context) -> egui::FontFamily {
    let family = egui::FontFamily::Name("sans-semibold".into());
    if ctx.fonts(|fonts| fonts.families().contains(&family)) {
        family
    } else {
        egui::FontFamily::Proportional
    }
}

pub(crate) fn strong(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .strong()
        .family(semibold_family(ui.ctx()))
}

pub(super) fn title(ui: &egui::Ui, text: impl Into<String>, size: f32) -> RichText {
    strong(ui, text)
        .size(size)
        .extra_letter_spacing(-0.02 * size)
}

pub(super) fn section_label(ui: &mut egui::Ui, colors: Palette, text: &str) {
    ui.label(
        strong(ui, text.to_uppercase())
            .size(11.0)
            .extra_letter_spacing(0.8)
            .color(colors.faint),
    );
}

pub(super) fn quiet(ui: &mut egui::Ui, colors: Palette, text: &str) -> egui::Response {
    kit::Button::new(kit::Kind::Ghost, text)
        .size(kit::Size::Medium)
        .show(ui, colors)
}

pub(super) fn secondary(
    ui: &mut egui::Ui,
    colors: Palette,
    text: &str,
    enabled: bool,
) -> egui::Response {
    kit::Button::new(kit::Kind::Secondary, text)
        .fill_width(ui)
        .enabled(enabled)
        .show(ui, colors)
}

pub(super) fn feedback(ui: &mut egui::Ui, view: &crate::runtime::View) {
    let colors = Palette::new(ui.visuals().dark_mode);
    if let Some(error) = &view.error {
        egui::Frame::new()
            .fill(kit::lerp(colors.surface, colors.error, 0.08))
            .stroke(Stroke::new(1.0, colors.error.gamma_multiply(0.35)))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
                    kit::draw(
                        ui.painter(),
                        kit::Glyph::Alert,
                        rect.center(),
                        17.0,
                        colors.error,
                        0.0,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(strong(ui, "Não foi possível continuar").color(colors.error));
                        ui.add(egui::Label::new(RichText::new(error).color(colors.text)).wrap());
                    });
                });
            });
    }
    if let Some(notice) = &view.notice {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
            kit::draw(
                ui.painter(),
                kit::Glyph::Info,
                rect.center(),
                15.0,
                colors.muted,
                0.0,
            );
            ui.add(egui::Label::new(RichText::new(notice).color(colors.muted)).wrap());
        });
    }
}
