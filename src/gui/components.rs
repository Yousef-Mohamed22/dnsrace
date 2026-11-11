//! Shared Signal components used by both the desktop and mobile shells.
//!
//! Components own their sizing rules: a touch control never drops below 44 points, while a
//! desktop control stays at the 40-point density the rest of the window uses.

use eframe::egui::{
    self, Align, Color32, Layout, Margin, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::gui::icons::{self, Icon};

/// The smallest allowed touch target.
pub const TOUCH_MIN: f32 = 44.0;
/// The height of a primary action on a phone.
pub const TOUCH_PRIMARY: f32 = 48.0;
/// Desktop control height from the design.
pub const DESKTOP_CONTROL: f32 = 40.0;

const ACCENT: Color32 = Color32::from_rgb(0x38, 0xBD, 0xF8);
const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x12, 0x1F);
const DANGER: Color32 = Color32::from_rgb(0xFB, 0x71, 0x85);
const ON_DANGER: Color32 = Color32::from_rgb(0x2A, 0x07, 0x0E);

/// Visual weight of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Semantic colour of a pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

/// A labelled button with a consistent shape and a touch-aware minimum size.
pub fn button(
    ui: &mut egui::Ui,
    label: &str,
    kind: ButtonKind,
    touch: bool,
    enabled: bool,
) -> egui::Response {
    let height = if touch {
        TOUCH_PRIMARY
    } else {
        DESKTOP_CONTROL
    };
    let min_width = if touch { TOUCH_MIN } else { 88.0 };
    let text_color = ui.visuals().text_color();
    let (fill, stroke, label_color) = match kind {
        ButtonKind::Primary => (ACCENT, Stroke::NONE, ON_ACCENT),
        ButtonKind::Secondary => (
            ui.visuals().widgets.inactive.bg_fill,
            Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
            text_color,
        ),
        ButtonKind::Ghost => (Color32::TRANSPARENT, Stroke::NONE, text_color),
        ButtonKind::Danger => (DANGER, Stroke::NONE, ON_DANGER),
    };
    let text = RichText::new(label)
        .size(if touch { 16.0 } else { 14.0 })
        .color(label_color);
    let widget = egui::Button::new(text)
        .fill(fill)
        .stroke(stroke)
        .corner_radius(10.0)
        .min_size(Vec2::new(min_width, height));
    ui.add_enabled(enabled, widget)
}

/// An icon-only button with a tooltip and an accessible name.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, touch: bool) -> egui::Response {
    let size = if touch { TOUCH_MIN } else { 32.0 };
    let response = icons::paint_in(ui, icon, size, ui.visuals().text_color(), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
    response.on_hover_text(tooltip)
}

/// A flat card surface with a one-pixel border.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(12.0)
        .inner_margin(Margin::same(16))
        .show(ui, add)
}

fn tone_colors(ui: &egui::Ui, tone: Tone) -> (Color32, Color32) {
    let visuals = ui.visuals();
    let color = match tone {
        Tone::Neutral => visuals.weak_text_color(),
        Tone::Accent => ACCENT,
        Tone::Success => Color32::from_rgb(0x34, 0xD3, 0x99),
        Tone::Warning => Color32::from_rgb(0xFB, 0xBF, 0x24),
        Tone::Danger => DANGER,
    };
    let fill = color.gamma_multiply(if visuals.dark_mode { 0.18 } else { 0.12 });
    (color, fill)
}

/// A compact status pill.
pub fn pill(ui: &mut egui::Ui, text: &str, tone: Tone) {
    let (color, fill) = tone_colors(ui, tone);
    // Paint the pill directly instead of nesting a Label in a Frame. A Frame does not
    // report its width before placement, which makes wrapping rows place pills past the
    // available width instead of moving them to the next line.
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(12.0), color);
    let size = galley.size() + Vec2::new(20.0, 8.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            255.0,
            fill,
            Stroke::new(1.0, color.gamma_multiply(0.55)),
            StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

/// A full-width message. Errors use the danger tone and successes the success tone; the shell
/// shows the newest notice so an apply failure stays visible after the dialog closes.
pub fn notice(ui: &mut egui::Ui, text: &str, error: bool) {
    let (color, fill) = tone_colors(ui, if error { Tone::Danger } else { Tone::Success });
    let _ = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(10.0)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            // A horizontal layout extends labels by default, so a long "Finished: Current DNS
            // (...)" notice used to widen the whole scroll page past the phone screen. Wrap the
            // text explicitly and keep the icon beside the first line.
            ui.horizontal_top(|ui| {
                ui.add_space(2.0);
                icons::paint_in(
                    ui,
                    if error { Icon::Alert } else { Icon::Check },
                    16.0,
                    color,
                    Sense::hover(),
                );
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
            });
        });
}

/// A selectable filter chip. Returns the click response so the caller can emit an action.
pub fn chip(ui: &mut egui::Ui, label: &str, selected: bool, touch: bool) -> egui::Response {
    let height = if touch { TOUCH_MIN } else { 30.0 };
    let fill = if selected {
        ACCENT.gamma_multiply(if ui.visuals().dark_mode { 0.18 } else { 0.12 })
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if selected {
        Stroke::new(1.0, ACCENT)
    } else {
        Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color)
    };
    let text = RichText::new(label)
        .size(if touch { 14.0 } else { 12.0 })
        .color(if selected {
            ACCENT
        } else {
            ui.visuals().text_color()
        });
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .stroke(stroke)
            .corner_radius(255.0)
            .min_size(Vec2::new(if touch { TOUCH_MIN } else { 0.0 }, height)),
    )
}

/// A label, a prominent value and an optional hint.
pub fn metric(ui: &mut egui::Ui, label: &str, value: &str, hint: Option<&str>) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .size(12.0)
                .color(ui.visuals().weak_text_color()),
        );
        ui.label(
            RichText::new(value)
                .size(20.0)
                .strong()
                .color(ui.visuals().text_color()),
        );
        if let Some(hint) = hint {
            ui.label(
                RichText::new(hint)
                    .size(12.0)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
}

/// A row of mutually exclusive options. Returns true when the value changed this frame.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
    touch: bool,
) -> bool {
    let height = if touch { TOUCH_MIN } else { DESKTOP_CONTROL };
    let mut changed = false;
    ui.push_id(id, |ui| {
        let add = |ui: &mut egui::Ui| {
            for (option, label) in options {
                let selected = *value == *option;
                let response = ui.add(
                    egui::Button::selectable(selected, *label).min_size(Vec2::new(0.0, height)),
                );
                if response.clicked() && !selected {
                    *value = *option;
                    changed = true;
                }
            }
        };
        if touch {
            // Four profile choices wrap into an orphaned last button on a phone. A 2x2
            // grid keeps every touch target the same width and looks deliberate.
            let columns = 2;
            let gap = ui.spacing().item_spacing.x;
            let width = ((ui.available_width() - gap) / columns as f32).max(1.0);
            egui::Grid::new("touch-segments")
                .num_columns(columns)
                .spacing(Vec2::new(gap, ui.spacing().item_spacing.y))
                .show(ui, |ui| {
                    for (index, (option, label)) in options.iter().enumerate() {
                        let selected = *value == *option;
                        let response = ui.add_sized(
                            Vec2::new(width, height),
                            egui::Button::selectable(selected, *label),
                        );
                        if response.clicked() && !selected {
                            *value = *option;
                            changed = true;
                        }
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        } else {
            ui.horizontal(add);
        }
    });
    changed
}

/// A full-width search field with a hint and a stable id.
pub fn search_field(ui: &mut egui::Ui, id: &str, query: &mut String, hint: &str) -> egui::Response {
    let height = ui.spacing().interact_size.y.max(DESKTOP_CONTROL);
    ui.add_sized(
        [ui.available_width(), height],
        egui::TextEdit::singleline(query)
            .id_salt(id)
            .hint_text(hint)
            .margin(Vec2::new(10.0, 8.0)),
    )
}

/// A page title, a one-line description and right-aligned actions.
pub fn page_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .size(20.0)
                    .strong()
                    .color(ui.visuals().text_color()),
            );
            if !subtitle.is_empty() {
                ui.label(
                    RichText::new(subtitle)
                        .size(13.0)
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn run_ui(add: impl FnOnce(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        crate::gui::theme::install(&ctx, crate::config::Theme::Dark);
        let mut add = Some(add);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(add) = add.take() {
                add(ui);
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn primary_button_meets_touch_size() {
        let rect = Cell::new(egui::Rect::NOTHING);
        run_ui(|ui| {
            rect.set(button(ui, "Run benchmark", ButtonKind::Primary, true, true).rect);
        });
        assert!(rect.get().height() >= TOUCH_PRIMARY);
        assert!(rect.get().width() >= TOUCH_MIN);
    }

    #[test]
    fn secondary_button_meets_desktop_height() {
        let rect = Cell::new(egui::Rect::NOTHING);
        run_ui(|ui| {
            rect.set(button(ui, "Cancel", ButtonKind::Secondary, false, true).rect);
        });
        assert!(rect.get().height() >= 40.0);
        assert!(rect.get().width() >= 44.0);
    }

    #[test]
    fn segmented_returns_change() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Choice {
            One,
            Two,
        }

        let mut harness = Harness::new_ui_state(
            |ui, state: &mut (Choice, bool)| {
                let changed = segmented(
                    ui,
                    "choice",
                    &mut state.0,
                    &[(Choice::One, "One"), (Choice::Two, "Two")],
                    true,
                );
                if changed {
                    state.1 = true;
                }
            },
            (Choice::One, false),
        );
        harness.run();
        assert_eq!(harness.state().0, Choice::One);
        harness.get_by_label("Two").click();
        harness.run();
        assert_eq!(harness.state().0, Choice::Two);
        assert!(harness.state().1);
    }

    #[test]
    fn paint_all_icons_does_not_panic() {
        let mut painted = 0;
        run_ui(|ui| {
            for icon in Icon::ALL {
                let response = icons::paint(ui, icon, 20.0, ui.visuals().text_color());
                assert!(response.rect.width() >= 20.0);
                painted += 1;
            }
        });
        assert_eq!(painted, Icon::ALL.len());
    }
}
