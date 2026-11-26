#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteTokens {
    pub background: Rgb,
    pub surface: Rgb,
    pub inset: Rgb,
    pub raised: Rgb,
    pub hover: Rgb,
    pub disabled: Rgb,
    pub action: Rgb,
    pub action_hover: Rgb,
    pub action_ink: Rgb,
    pub border: Rgb,
    pub border_strong: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub faint: Rgb,
    pub accent: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
}

pub const DARK: PaletteTokens = PaletteTokens {
    background: Rgb(22, 22, 22),
    surface: Rgb(32, 32, 32),
    inset: Rgb(16, 16, 16),
    raised: Rgb(32, 32, 32),
    hover: Rgb(39, 39, 39),
    disabled: Rgb(32, 32, 32),
    action: Rgb(238, 238, 236),
    action_hover: Rgb(255, 255, 255),
    action_ink: Rgb(20, 20, 20),
    border: Rgb(45, 45, 45),
    border_strong: Rgb(69, 69, 69),
    text: Rgb(238, 238, 236),
    muted: Rgb(164, 164, 161),
    faint: Rgb(139, 139, 136),
    accent: Rgb(238, 238, 236),
    success: Rgb(238, 238, 236),
    warning: Rgb(230, 179, 158),
    error: Rgb(230, 179, 158),
};

pub const LIGHT: PaletteTokens = PaletteTokens {
    background: Rgb(245, 245, 243),
    surface: Rgb(255, 255, 255),
    inset: Rgb(236, 236, 233),
    raised: Rgb(255, 255, 255),
    hover: Rgb(233, 233, 230),
    disabled: Rgb(236, 236, 233),
    action: Rgb(22, 22, 22),
    action_hover: Rgb(0, 0, 0),
    action_ink: Rgb(245, 245, 243),
    border: Rgb(221, 221, 219),
    border_strong: Rgb(190, 190, 187),
    text: Rgb(22, 22, 22),
    muted: Rgb(86, 86, 83),
    faint: Rgb(110, 110, 107),
    accent: Rgb(22, 22, 22),
    success: Rgb(22, 22, 22),
    warning: Rgb(158, 68, 38),
    error: Rgb(158, 68, 38),
};

pub const FONT_SANS: &str = "Geist";
pub const FONT_MONO: &str = "Geist Mono";

pub const TEXT_BODY_PX: u16 = 15;
pub const TEXT_SMALL_PX: u16 = 13;
pub const TEXT_HEADING_PX: u16 = 20;
pub const TEXT_MONO_PX: u16 = 13;

pub const RADIUS_WINDOW_PX: u8 = 10;
pub const RADIUS_CONTROL_PX: u8 = 8;
pub const RADIUS_SMALL_PX: u8 = 6;

pub const ITEM_SPACING_X_PX: u16 = 12;
pub const ITEM_SPACING_Y_PX: u16 = 10;
pub const BUTTON_PAD_X_PX: u16 = 14;
pub const BUTTON_PAD_Y_PX: u16 = 8;
pub const BUTTON_LARGE_H_PX: u16 = 48;
pub const BUTTON_MEDIUM_H_PX: u16 = 40;
pub const BUTTON_SMALL_H_PX: u16 = 32;

pub const PLAYER_WIDE_PX: u16 = 860;
pub const PLAYER_CONTROLS_H_PX: u16 = 106;
pub const PLAYER_ICON_PX: u16 = 38;
pub const PLAYER_SIDE_WIDE_PX: u16 = 28;
pub const PLAYER_SIDE_NARROW_PX: u16 = 16;
pub const PLAYER_INK: Rgb = Rgb(222, 222, 219);
pub const PLAYER_MUTED: Rgb = Rgb(216, 216, 213);

fn write_palette(out: &mut String, indent: &str, p: PaletteTokens) {
    for (name, rgb) in [
        ("background", p.background),
        ("surface", p.surface),
        ("inset", p.inset),
        ("raised", p.raised),
        ("hover", p.hover),
        ("disabled", p.disabled),
        ("action", p.action),
        ("action-hover", p.action_hover),
        ("action-ink", p.action_ink),
        ("border", p.border),
        ("border-strong", p.border_strong),
        ("text", p.text),
        ("muted", p.muted),
        ("faint", p.faint),
        ("accent", p.accent),
        ("success", p.success),
        ("warning", p.warning),
        ("error", p.error),
    ] {
        out.push_str(&format!("{indent}--{name}: {};\n", rgb.hex()));
    }
}

pub fn css_tokens() -> String {
    let mut out = String::new();
    out.push_str(":root {\n");
    write_palette(&mut out, "  ", DARK);
    out.push_str(&format!(
        "  --font-sans: \"{FONT_SANS}\", system-ui, sans-serif;\n"
    ));
    out.push_str(&format!(
        "  --font-mono: \"{FONT_MONO}\", ui-monospace, monospace;\n"
    ));
    out.push_str(&format!("  --text-body: {TEXT_BODY_PX}px;\n"));
    out.push_str(&format!("  --text-small: {TEXT_SMALL_PX}px;\n"));
    out.push_str(&format!("  --text-heading: {TEXT_HEADING_PX}px;\n"));
    out.push_str(&format!("  --text-mono: {TEXT_MONO_PX}px;\n"));
    out.push_str(&format!("  --radius-window: {RADIUS_WINDOW_PX}px;\n"));
    out.push_str(&format!("  --radius-control: {RADIUS_CONTROL_PX}px;\n"));
    out.push_str(&format!("  --radius-small: {RADIUS_SMALL_PX}px;\n"));
    out.push_str(&format!("  --space-x: {ITEM_SPACING_X_PX}px;\n"));
    out.push_str(&format!("  --space-y: {ITEM_SPACING_Y_PX}px;\n"));
    out.push_str(&format!("  --button-pad-x: {BUTTON_PAD_X_PX}px;\n"));
    out.push_str(&format!("  --button-pad-y: {BUTTON_PAD_Y_PX}px;\n"));
    out.push_str(&format!("  --button-large-h: {BUTTON_LARGE_H_PX}px;\n"));
    out.push_str(&format!("  --button-medium-h: {BUTTON_MEDIUM_H_PX}px;\n"));
    out.push_str(&format!("  --button-small-h: {BUTTON_SMALL_H_PX}px;\n"));
    out.push_str(&format!(
        "  --player-controls-h: {PLAYER_CONTROLS_H_PX}px;\n"
    ));
    out.push_str(&format!("  --player-icon: {PLAYER_ICON_PX}px;\n"));
    out.push_str(&format!("  --player-side-wide: {PLAYER_SIDE_WIDE_PX}px;\n"));
    out.push_str(&format!(
        "  --player-side-narrow: {PLAYER_SIDE_NARROW_PX}px;\n"
    ));
    out.push_str(&format!("  --player-ink: {};\n", PLAYER_INK.hex()));
    out.push_str(&format!("  --player-muted: {};\n", PLAYER_MUTED.hex()));
    out.push_str(&format!("  --player-accent: {};\n", DARK.accent.hex()));
    out.push_str(&format!("  --player-warning: {};\n", DARK.warning.hex()));
    out.push_str("  color-scheme: dark;\n");
    out.push_str("}\n");
    out.push_str("@media (prefers-color-scheme: light) {\n");
    out.push_str("  :root:not([data-theme=\"dark\"]) {\n");
    write_palette(&mut out, "    ", LIGHT);
    out.push_str("    color-scheme: light;\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(":root[data-theme=\"light\"] {\n");
    write_palette(&mut out, "  ", LIGHT);
    out.push_str("  color-scheme: light;\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_tokens_include_dark_and_light_palettes() {
        let css = css_tokens();
        assert!(css.contains("--background: #161616;"));
        assert!(css.contains("--background: #f5f5f3;"));
        assert!(css.contains("--warning: #e6b39e;"));
        assert!(css.contains("--warning: #9e4426;"));
        assert!(css.contains("--font-sans: \"Geist\""));
    }
}
