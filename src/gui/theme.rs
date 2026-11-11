//! The Signal design tokens and palette installation.
//!
//! Every palette exposes the same semantic colours, so components never branch on the theme
//! name. The default Dark and Light palettes use the Signal values from the design; the older
//! presets keep their character while adopting the new token set.

use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Stroke};

use crate::config::Theme;

/// Semantic colours shared by every screen and component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub bg: Color32,
    pub surface: Color32,
    pub elevated: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Resolve `System` against the currently active egui preference, then return that theme's
/// semantic colours.
pub fn tokens(theme: Theme, active: egui::Theme) -> Tokens {
    let theme = match theme {
        Theme::System => {
            if active == egui::Theme::Dark {
                Theme::Dark
            } else {
                Theme::Light
            }
        }
        fixed => fixed,
    };
    match theme {
        Theme::Light => Tokens {
            bg: rgb(0xF6, 0xF8, 0xFC),
            surface: rgb(0xFF, 0xFF, 0xFF),
            elevated: rgb(0xEE, 0xF2, 0xF9),
            border: rgb(0xD9, 0xE2, 0xF0),
            text: rgb(0x18, 0x21, 0x33),
            muted: rgb(0x5E, 0x6B, 0x80),
            accent: rgb(0x02, 0x84, 0xC7),
            success: rgb(0x05, 0x96, 0x69),
            warning: rgb(0xD9, 0x77, 0x06),
            danger: rgb(0xE1, 0x1D, 0x48),
        },
        Theme::Midnight => Tokens {
            bg: rgb(0x08, 0x0C, 0x18),
            surface: rgb(0x0F, 0x15, 0x26),
            elevated: rgb(0x17, 0x21, 0x3A),
            border: rgb(0x21, 0x2C, 0x44),
            text: rgb(0xDC, 0xE6, 0xFA),
            muted: rgb(0x91, 0xA0, 0xBC),
            accent: rgb(0x60, 0xA5, 0xFA),
            success: rgb(0x34, 0xD3, 0x99),
            warning: rgb(0xFB, 0xBF, 0x24),
            danger: rgb(0xFB, 0x71, 0x85),
        },
        Theme::Nord => Tokens {
            bg: rgb(0x2E, 0x34, 0x40),
            surface: rgb(0x3B, 0x42, 0x52),
            elevated: rgb(0x43, 0x4C, 0x5E),
            border: rgb(0x4C, 0x56, 0x6A),
            text: rgb(0xEC, 0xEF, 0xF4),
            muted: rgb(0xA3, 0xAE, 0xC2),
            accent: rgb(0x88, 0xC0, 0xD0),
            success: rgb(0xA3, 0xBE, 0x8C),
            warning: rgb(0xEB, 0xCB, 0x8B),
            danger: rgb(0xBF, 0x61, 0x6A),
        },
        Theme::SolarizedLight => Tokens {
            bg: rgb(0xFD, 0xF6, 0xE3),
            surface: rgb(0xEE, 0xE8, 0xD5),
            elevated: rgb(0xE4, 0xDD, 0xC8),
            border: rgb(0xD3, 0xCB, 0xB8),
            text: rgb(0x58, 0x6E, 0x75),
            muted: rgb(0x65, 0x7B, 0x83),
            accent: rgb(0x26, 0x8B, 0xD2),
            success: rgb(0x85, 0x99, 0x00),
            warning: rgb(0xB5, 0x89, 0x00),
            danger: rgb(0xDC, 0x32, 0x2F),
        },
        Theme::Dracula => Tokens {
            bg: rgb(0x28, 0x2A, 0x36),
            surface: rgb(0x2D, 0x2F, 0x3D),
            elevated: rgb(0x38, 0x3A, 0x4C),
            border: rgb(0x44, 0x47, 0x5A),
            text: rgb(0xF8, 0xF8, 0xF2),
            muted: rgb(0x9A, 0xA0, 0xB5),
            accent: rgb(0xBD, 0x93, 0xF9),
            success: rgb(0x50, 0xFA, 0x7B),
            warning: rgb(0xF1, 0xFA, 0x8C),
            danger: rgb(0xFF, 0x55, 0x55),
        },
        Theme::HighContrast => Tokens {
            bg: Color32::BLACK,
            surface: rgb(0x08, 0x08, 0x08),
            elevated: rgb(0x14, 0x14, 0x14),
            border: rgb(0x96, 0x96, 0x96),
            text: Color32::WHITE,
            muted: rgb(0xC8, 0xC8, 0xC8),
            accent: rgb(0x4D, 0xA3, 0xFF),
            success: rgb(0x3D, 0xFF, 0x8B),
            warning: rgb(0xFF, 0xD2, 0x3D),
            danger: rgb(0xFF, 0x5C, 0x7A),
        },
        Theme::System | Theme::Dark => Tokens {
            bg: rgb(0x0B, 0x12, 0x20),
            surface: rgb(0x11, 0x1A, 0x2B),
            elevated: rgb(0x16, 0x24, 0x3A),
            border: rgb(0x26, 0x36, 0x4F),
            text: rgb(0xE8, 0xEE, 0xF8),
            muted: rgb(0x93, 0xA4, 0xBD),
            accent: rgb(0x38, 0xBD, 0xF8),
            success: rgb(0x34, 0xD3, 0x99),
            warning: rgb(0xFB, 0xBF, 0x24),
            danger: rgb(0xFB, 0x71, 0x85),
        },
    }
}

/// Install the bundled fonts and the selected palette on an egui context. Safe to call more
/// than once (for example when the user changes theme in Settings).
pub fn install(ctx: &egui::Context, theme: Theme) {
    install_fonts(ctx);
    let palette = tokens(theme, ctx.theme());
    ctx.set_visuals(visuals_for(palette));
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter-regular".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jetbrains-mono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "inter-regular".into());
    proportional.insert(1, "inter-semibold".into());

    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains-mono".into());

    ctx.set_fonts(fonts);
}

fn visuals_for(palette: Tokens) -> egui::Visuals {
    let dark = relative_luminance(palette.bg) < 0.5;
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.surface;
    visuals.faint_bg_color = palette.elevated;
    visuals.extreme_bg_color = palette.surface;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.window_stroke = Stroke::new(1.0, palette.border);

    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.inactive.bg_fill = palette.elevated;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.hovered.bg_fill = palette.elevated;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.active.bg_fill = palette.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.bg);
    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = Stroke::new(1.0, palette.bg);
    visuals.text_cursor.stroke = Stroke::new(1.5, palette.accent);
    visuals
}

fn relative_luminance(color: Color32) -> f32 {
    let channel = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Theme;

    #[test]
    fn theme_tokens_are_distinct() {
        let dark = tokens(Theme::Dark, egui::Theme::Dark);
        let light = tokens(Theme::Light, egui::Theme::Light);
        assert_ne!(dark.bg, dark.surface);
        assert_ne!(dark.surface, dark.elevated);
        assert_ne!(dark.border, dark.surface);
        assert_ne!(dark.text, dark.muted);
        assert_ne!(dark.accent, dark.success);
        assert_ne!(dark.success, dark.warning);
        assert_ne!(dark.warning, dark.danger);
        assert_ne!(dark.bg, light.bg);
        assert_ne!(dark.text, light.text);
        assert_eq!(dark.text, egui::Color32::from_rgb(0xE8, 0xEE, 0xF8));
        assert_eq!(light.bg, egui::Color32::from_rgb(0xF6, 0xF8, 0xFC));
    }

    #[test]
    fn system_follows_the_active_theme() {
        assert_eq!(
            tokens(Theme::System, egui::Theme::Dark),
            tokens(Theme::Dark, egui::Theme::Dark)
        );
        assert_eq!(
            tokens(Theme::System, egui::Theme::Light),
            tokens(Theme::Light, egui::Theme::Light)
        );
    }

    #[test]
    fn every_preset_has_a_complete_palette() {
        for theme in Theme::ALL {
            let palette = tokens(theme, egui::Theme::Dark);
            assert_ne!(palette.bg, palette.text, "{theme:?} must contrast");
            assert_ne!(palette.surface, palette.text, "{theme:?} must contrast");
            assert_ne!(palette.accent, palette.bg, "{theme:?} accent must show");
        }
    }

    #[test]
    fn install_fonts_does_not_panic() {
        let ctx = egui::Context::default();
        install(&ctx, Theme::Dark);
        install(&ctx, Theme::Light);
        let mut label_height = 0.0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            label_height = ui.label("dnsrace 0.8.0").rect.height();
            ui.monospace("1.1.1.1:53");
        });
        output.textures_delta.clear();
        assert!(label_height > 0.0);
        assert_eq!(
            ctx.style_of(ctx.theme()).visuals.panel_fill,
            tokens(Theme::Light, egui::Theme::Light).bg
        );
    }
}
