use super::kit;
use eframe::egui::{self, Color32, RichText, Stroke};

const MEDIUM_FAMILY: &str = "sans-medium";

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) background: Color32,
    pub(crate) surface: Color32,
    pub(crate) inset: Color32,
    pub(crate) raised: Color32,
    pub(crate) hover: Color32,
    pub(crate) disabled: Color32,
    pub(crate) action: Color32,
    pub(crate) action_hover: Color32,
    pub(crate) action_ink: Color32,
    pub(crate) border: Color32,
    pub(crate) border_strong: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) faint: Color32,
    pub(crate) accent: Color32,
    pub(crate) success: Color32,
    pub(crate) warning: Color32,
    pub(crate) error: Color32,
}

impl Palette {
    pub(crate) fn new(dark: bool) -> Self {
        if dark {
            let text = Color32::from_rgb(238, 238, 236);
            let alert = Color32::from_rgb(230, 179, 158);
            Self {
                background: Color32::from_rgb(22, 22, 22),
                surface: Color32::from_rgb(32, 32, 32),
                inset: Color32::from_rgb(16, 16, 16),
                raised: Color32::from_rgb(32, 32, 32),
                hover: Color32::from_rgb(39, 39, 39),
                disabled: Color32::from_rgb(32, 32, 32),
                action: text,
                action_hover: Color32::WHITE,
                action_ink: Color32::from_rgb(20, 20, 20),
                border: Color32::from_rgb(45, 45, 45),
                border_strong: Color32::from_rgb(69, 69, 69),
                text,
                muted: Color32::from_rgb(164, 164, 161),
                faint: Color32::from_rgb(139, 139, 136),
                accent: text,
                success: text,
                warning: alert,
                error: alert,
            }
        } else {
            let text = Color32::from_rgb(22, 22, 22);
            let alert = Color32::from_rgb(158, 68, 38);
            Self {
                background: Color32::from_rgb(245, 245, 243),
                surface: Color32::WHITE,
                inset: Color32::from_rgb(236, 236, 233),
                raised: Color32::WHITE,
                hover: Color32::from_rgb(233, 233, 230),
                disabled: Color32::from_rgb(236, 236, 233),
                action: text,
                action_hover: Color32::BLACK,
                action_ink: Color32::from_rgb(245, 245, 243),
                border: Color32::from_rgb(221, 221, 219),
                border_strong: Color32::from_rgb(190, 190, 187),
                text,
                muted: Color32::from_rgb(86, 86, 83),
                faint: Color32::from_rgb(110, 110, 107),
                accent: text,
                success: text,
                warning: alert,
                error: alert,
            }
        }
    }
}

pub fn configure_theme(ctx: &egui::Context, light: bool) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        (
            "sans",
            include_bytes!("../../assets/fonts/Geist-Regular.ttf").as_slice(),
        ),
        (
            MEDIUM_FAMILY,
            include_bytes!("../../assets/fonts/Geist-Medium.ttf").as_slice(),
        ),
        (
            "mono",
            include_bytes!("../../assets/fonts/GeistMono-Regular.ttf").as_slice(),
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
    let mut medium = proportional.clone();
    medium.insert(0, MEDIUM_FAMILY.into());
    fonts
        .families
        .insert(egui::FontFamily::Name(MEDIUM_FAMILY.into()), medium);
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "mono".into());
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
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    let shadow = Color32::from_black_alpha(if light { 22 } else { 90 });
    visuals.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow,
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: shadow,
    };
    visuals.extreme_bg_color = colors.inset;
    visuals.text_edit_bg_color = Some(colors.inset);
    visuals.faint_bg_color = colors.surface;
    visuals.code_bg_color = colors.inset;
    visuals.override_text_color = Some(colors.text);
    visuals.hyperlink_color = colors.text;
    visuals.weak_text_color = Some(colors.muted);
    visuals.selection.bg_fill = colors
        .accent
        .gamma_multiply(if light { 0.16 } else { 0.24 });
    visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    visuals.text_cursor.stroke = Stroke::new(1.5, colors.accent);
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
        widget.corner_radius = egui::CornerRadius::same(6);
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
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            style.spacing.interact_size.y = 34.0;
            style.spacing.menu_margin = egui::Margin::same(10);
            style.spacing.menu_spacing = 4.0;
            style.spacing.scroll = egui::style::ScrollStyle::floating();
            style.animation_time = 0.15;
            for (text_style, font) in [
                (egui::TextStyle::Body, egui::FontId::proportional(15.0)),
                (egui::TextStyle::Button, egui::FontId::proportional(15.0)),
                (egui::TextStyle::Small, egui::FontId::proportional(13.0)),
                (egui::TextStyle::Heading, egui::FontId::proportional(20.0)),
                (egui::TextStyle::Monospace, egui::FontId::monospace(13.0)),
            ] {
                style.text_styles.insert(text_style, font);
            }
        },
    );
}

pub(super) fn medium_family(ctx: &egui::Context) -> egui::FontFamily {
    let family = egui::FontFamily::Name(MEDIUM_FAMILY.into());
    if ctx.fonts(|fonts| fonts.families().contains(&family)) {
        family
    } else {
        egui::FontFamily::Proportional
    }
}

pub(crate) fn strong(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text).family(medium_family(ui.ctx()))
}

pub(super) fn title(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .extra_letter_spacing(-0.035 * size)
}

pub(super) fn section_label(ui: &mut egui::Ui, colors: Palette, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(colors.faint));
}

pub(super) fn quiet(ui: &mut egui::Ui, colors: Palette, text: &str) -> egui::Response {
    kit::Button::new(kit::Kind::Ghost, text)
        .size(kit::Size::Small)
        .show(ui, colors)
}

pub(super) fn feedback(ui: &mut egui::Ui, view: &crate::runtime::View) {
    messages(ui, view.error.as_deref(), view.notice.as_deref());
}

pub(super) fn messages(ui: &mut egui::Ui, error: Option<&str>, notice: Option<&str>) {
    let colors = Palette::new(ui.visuals().dark_mode);
    if let Some(error) = error {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 20.0), egui::Sense::hover());
            kit::draw(
                ui.painter(),
                kit::Glyph::Alert,
                rect.center(),
                16.0,
                colors.error,
                0.0,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(strong(ui, "Não foi possível continuar").color(colors.error));
                ui.add(egui::Label::new(RichText::new(error).color(colors.muted)).wrap());
            });
        });
    }
    if let Some(notice) = notice {
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
