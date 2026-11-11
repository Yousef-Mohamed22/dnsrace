//! The Settings page: appearance, benchmark defaults, network options, data management and the
//! in-page About card.
//!
//! Every control reads `AppState` and emits a `UiAction`; nothing here writes a file or talks to
//! the platform.

use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

use crate::config::Theme;
use crate::gui::actions::{AboutLink, UiAction};
use crate::gui::components::{self, ButtonKind};
use crate::gui::icons;
use crate::gui::state::{AppState, LayoutMode, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::profile::Profile;
use crate::servers::{Family, Protocol};

const PAGE_MAX_WIDTH: f32 = 1180.0;
const CARD_GAP: f32 = 12.0;

/// Draw the desktop Settings page.
pub fn render_settings(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    let touch = cx.layout == LayoutMode::Mobile;
    ui.set_max_width(ui.available_width().min(PAGE_MAX_WIDTH));

    if state.about_open {
        if state.about_scroll_pending {
            // About is drawn above the settings cards. Without this, opening it from deep in
            // the page leaves the phone user looking at the middle of the form.
            ui.scroll_to_cursor(Some(Align::TOP));
            state.about_scroll_pending = false;
        }
        render_about(state, cx, ui, actions, tokens);
        ui.add_space(CARD_GAP);
    }

    actions_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    appearance_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    benchmark_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    network_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    data_card(state, ui, actions, tokens, touch);
}

/// The About card. It lives inside Settings instead of opening a floating window.
pub fn render_about(
    _state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    let touch = cx.layout == LayoutMode::Mobile;
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
            if ui.is_rect_visible(rect) {
                icons::paint_logo(ui.painter(), rect);
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("dnsrace").size(20.0).strong());
                ui.label(
                    RichText::new(format!("Version {}", cx.version))
                        .size(13.0)
                        .color(tokens.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::button(ui, "Close about", ButtonKind::Ghost, touch, true).clicked() {
                    actions.push(UiAction::CloseAbout);
                }
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(RichText::new("Find the fastest DNS resolver for your connection.").size(13.0));
        ui.label(
            RichText::new("MIT licence · 100% Rust · No account, no telemetry.")
                .size(12.0)
                .color(tokens.muted),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for (label, link) in [
                ("Project website", AboutLink::Project),
                ("Report an issue", AboutLink::Issues),
                ("MIT licence", AboutLink::Licence),
            ] {
                if components::button(ui, label, ButtonKind::Ghost, touch, true).clicked() {
                    actions.push(UiAction::OpenAboutLink(link));
                }
            }
        });
    });
}

fn actions_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        if touch {
            ui.vertical(|ui| {
                ui.label(RichText::new("Settings actions").size(16.0).strong());
                ui.label(
                    RichText::new(
                        "Saving writes these settings to this device. Reset restores the defaults.",
                    )
                    .size(12.5)
                    .color(tokens.muted),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if components::button(
                        ui,
                        "Save settings",
                        ButtonKind::Primary,
                        true,
                        !state.running,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::SaveSettings);
                    }
                    if components::button(
                        ui,
                        "Reset settings",
                        ButtonKind::Secondary,
                        true,
                        !state.running,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::ResetSettings);
                    }
                });
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Settings actions").size(16.0).strong());
                ui.label(
                    RichText::new(
                        "Saving writes these settings to this device. Reset restores the defaults.",
                    )
                    .size(12.5)
                    .color(tokens.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::button(
                    ui,
                    "Save settings",
                    ButtonKind::Primary,
                    false,
                    !state.running,
                )
                .clicked()
                {
                    actions.push(UiAction::SaveSettings);
                }
                if components::button(
                    ui,
                    "Reset settings",
                    ButtonKind::Secondary,
                    false,
                    !state.running,
                )
                .clicked()
                {
                    actions.push(UiAction::ResetSettings);
                }
            });
        });
    });
}

fn appearance_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Appearance").size(16.0).strong());
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for theme in Theme::ALL {
                if components::chip(ui, theme.label(), state.settings.theme == theme, touch)
                    .clicked()
                {
                    actions.push(UiAction::SetTheme(theme));
                }
            }
        });
        ui.label(
            RichText::new(state.settings.theme.description())
                .size(12.0)
                .color(tokens.muted),
        );
    });
}

fn benchmark_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Benchmark defaults").size(16.0).strong());
        ui.add_space(6.0);

        let mut profile = state.settings.profile;
        let options = Profile::ALL.map(|profile| (profile, profile.label()));
        if components::segmented(ui, "settings-profile", &mut profile, &options, touch) {
            actions.push(UiAction::SetProfile(profile));
        }
        ui.label(
            RichText::new(profile.description())
                .size(12.0)
                .color(tokens.muted),
        );

        ui.add_space(8.0);
        let style = FieldStyle { touch, tokens };
        macro_rules! field {
            ($ui:expr, $label:expr, $value:expr, $range:expr, $speed:expr, $suffix:expr, $action:expr) => {
                if let Some(value) =
                    number_field($ui, $label, $value, $range, $speed, $suffix, style)
                {
                    actions.push($action(value));
                }
            };
        }

        if touch {
            // Five controls cannot fit one wrapped row on a phone: the last group used to be
            // squeezed until its label stacked one letter per line. Give every field half the
            // card and a 44 dp target.
            let width = ((ui.available_width() - 12.0) / 2.0).max(120.0);
            egui::Grid::new("settings-advanced-touch")
                .num_columns(2)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        field!(
                            ui,
                            "Timeout",
                            state.settings.timeout_ms,
                            100..=10_000,
                            10.0,
                            " ms",
                            UiAction::SetTimeout
                        );
                    });
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        field!(
                            ui,
                            "Jobs",
                            state.settings.jobs,
                            1..=64,
                            1.0,
                            "",
                            UiAction::SetJobs
                        );
                    });
                    ui.end_row();
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        field!(
                            ui,
                            "Repeat",
                            state.settings.repeat,
                            1..=5,
                            1.0,
                            "",
                            UiAction::SetRepeat
                        );
                    });
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        field!(
                            ui,
                            "Cached rounds",
                            state.settings.cached_rounds,
                            1..=10,
                            1.0,
                            "",
                            UiAction::SetCachedRounds
                        );
                    });
                    ui.end_row();
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        field!(
                            ui,
                            "Random lookups",
                            state.settings.random_lookups,
                            0..=50,
                            1.0,
                            "",
                            UiAction::SetRandomLookups
                        );
                    });
                    ui.vertical(|ui| {
                        ui.set_width(width);
                    });
                    ui.end_row();
                });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(96.0);
                    field!(
                        ui,
                        "Timeout",
                        state.settings.timeout_ms,
                        100..=10_000,
                        10.0,
                        " ms",
                        UiAction::SetTimeout
                    );
                });
                ui.vertical(|ui| {
                    ui.set_min_width(96.0);
                    field!(
                        ui,
                        "Jobs",
                        state.settings.jobs,
                        1..=64,
                        1.0,
                        "",
                        UiAction::SetJobs
                    );
                });
                ui.vertical(|ui| {
                    ui.set_min_width(96.0);
                    field!(
                        ui,
                        "Repeat",
                        state.settings.repeat,
                        1..=5,
                        1.0,
                        "",
                        UiAction::SetRepeat
                    );
                });
                ui.vertical(|ui| {
                    ui.set_min_width(96.0);
                    field!(
                        ui,
                        "Cached rounds",
                        state.settings.cached_rounds,
                        1..=10,
                        1.0,
                        "",
                        UiAction::SetCachedRounds
                    );
                });
                ui.vertical(|ui| {
                    ui.set_min_width(96.0);
                    field!(
                        ui,
                        "Random lookups",
                        state.settings.random_lookups,
                        0..=50,
                        1.0,
                        "",
                        UiAction::SetRandomLookups
                    );
                });
            });
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("Editing any value switches the profile to Custom.")
                .size(11.5)
                .color(tokens.muted),
        );
    });
}

/// Style inputs shared by the advanced number fields.
#[derive(Clone, Copy)]
struct FieldStyle {
    touch: bool,
    tokens: Tokens,
}

/// One labelled advanced benchmark value. Touch fields fill their column and are 44 dp high;
/// desktop fields keep their natural width.
fn number_field<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: T,
    range: std::ops::RangeInclusive<T>,
    speed: f64,
    suffix: &str,
    style: FieldStyle,
) -> Option<T> {
    let label = ui.label(RichText::new(label).size(11.0).color(style.tokens.muted));
    let mut value = value;
    let field = egui::DragValue::new(&mut value)
        .range(range)
        .speed(speed)
        .suffix(suffix);
    let response = if style.touch {
        ui.add_sized([ui.available_width(), 44.0], field)
    } else {
        ui.add(field)
    };
    response.labelled_by(label.id).changed().then_some(value)
}

fn network_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Network and steering").size(16.0).strong());
        ui.add_space(6.0);

        ui.label(RichText::new("Protocol").size(11.0).color(tokens.muted));
        ui.horizontal_wrapped(|ui| {
            for protocol in Protocol::all() {
                if components::chip(
                    ui,
                    protocol.label(),
                    state.settings.protocol == protocol,
                    touch,
                )
                .clicked()
                {
                    actions.push(UiAction::SetProtocol(protocol));
                }
            }
        });

        ui.add_space(4.0);
        ui.label(
            RichText::new("Address family")
                .size(11.0)
                .color(tokens.muted),
        );
        ui.horizontal_wrapped(|ui| {
            for family in [Family::Any, Family::V4, Family::V6] {
                if components::chip(ui, family.label(), state.settings.family == family, touch)
                    .clicked()
                {
                    actions.push(UiAction::SetFamily(family));
                }
            }
        });

        ui.add_space(6.0);
        let mut include_system = state.settings.include_system;
        if ui
            .checkbox(&mut include_system, "Include servers found on this machine")
            .changed()
        {
            actions.push(UiAction::SetIncludeSystem(include_system));
        }
        let mut include_nearby = state.settings.include_nearby;
        if ui
            .checkbox(&mut include_nearby, "Include nearby open resolvers")
            .changed()
        {
            actions.push(UiAction::SetIncludeNearby(include_nearby));
        }

        ui.add_space(8.0);
        ui.label(
            RichText::new("Steering hostnames")
                .size(11.0)
                .color(tokens.muted),
        );
        let mut hostnames = state.settings.hostnames.clone();
        let response = ui.add(
            egui::TextEdit::multiline(&mut hostnames)
                .hint_text("One hostname per line")
                .desired_rows(3)
                .margin(Vec2::new(10.0, 8.0)),
        );
        if response.changed() {
            actions.push(UiAction::SetHostnames(hostnames));
        }
    });
}

fn data_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Data").size(16.0).strong());
        ui.add_space(6.0);

        ui.label(
            RichText::new(format!("{} saved benchmark run(s)", state.history.len()))
                .size(12.5)
                .color(tokens.muted),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if components::button(
                ui,
                "Clear history",
                ButtonKind::Secondary,
                touch,
                !state.history.is_empty(),
            )
            .clicked()
            {
                actions.push(UiAction::RequestClearHistory);
            }
            if components::button(ui, "About dnsrace", ButtonKind::Ghost, touch, true).clicked() {
                actions.push(UiAction::OpenAbout);
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(RichText::new("Custom resolvers").size(13.0).strong());
        if state.settings.custom.is_empty() {
            ui.label(
                RichText::new("No custom resolvers yet.")
                    .size(12.0)
                    .color(tokens.muted),
            );
        } else {
            for (index, resolver) in state.settings.custom.iter().enumerate() {
                // Wrap on narrow phone rows: two long action labels used to overflow the
                // right-to-left group and paint over the resolver name.
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&resolver.name).size(12.5));
                    if components::button(
                        ui,
                        &format!("Edit {}", resolver.name),
                        ButtonKind::Ghost,
                        touch,
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::EditCustom(index));
                    }
                    if components::button(
                        ui,
                        &format!("Remove {}", resolver.name),
                        ButtonKind::Ghost,
                        touch,
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::RemoveCustom(index));
                    }
                });
            }
        }
        ui.add_space(4.0);
        if components::button(
            ui,
            "Add custom resolver",
            ButtonKind::Secondary,
            touch,
            true,
        )
        .clicked()
        {
            actions.push(UiAction::OpenCustom);
        }
    });
}
