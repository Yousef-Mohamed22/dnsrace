//! The Settings page: appearance, benchmark defaults, network options, data management and the
//! in-page About card.
//!
//! Every control reads `AppState` and emits a `UiAction`; nothing here writes a file or talks to
//! the platform.

use eframe::egui::{self, Align, Sense, Vec2};

use crate::config::Theme;
use crate::gui::actions::{AboutLink, UiAction};
use crate::gui::components::{self, ButtonKind};
use crate::gui::icons;
use crate::gui::state::{AppState, LayoutMode, UpdatePhase, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::i18n::{self, Language, t};
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
    updates_card(state, ui, actions, cx, tokens, touch);
    ui.add_space(CARD_GAP);
    benchmark_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    network_card(state, ui, actions, tokens, touch);
    ui.add_space(CARD_GAP);
    data_card(state, cx, ui, actions, tokens, touch);
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
        i18n::horizontal(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
            if ui.is_rect_visible(rect) {
                icons::paint_logo(ui.painter(), rect);
            }
            i18n::vertical(ui, |ui| {
                ui.label(crate::i18n::rich("dnsrace").size(20.0).strong());
                ui.label(
                    crate::i18n::rich(i18n::fill("Version {}", &[&cx.version]))
                        .size(13.0)
                        .color(tokens.muted),
                );
            });
            ui.with_layout(i18n::trailing_layout(Align::Center), |ui| {
                if components::button(ui, "Close about", ButtonKind::Ghost, touch, true).clicked() {
                    actions.push(UiAction::CloseAbout);
                }
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(
            crate::i18n::rich("Find the fastest DNS resolver for your connection.").size(13.0),
        );
        ui.label(
            crate::i18n::rich("MIT licence · 100% Rust · No account, no telemetry.")
                .size(12.0)
                .color(tokens.muted),
        );
        ui.add_space(8.0);
        i18n::horizontal_wrapped(ui, |ui| {
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
            i18n::vertical(ui, |ui| {
                ui.label(crate::i18n::rich("Settings actions").size(16.0).strong());
                ui.label(
                    crate::i18n::rich(
                        "Saving writes these settings to this device. Reset restores the defaults.",
                    )
                    .size(12.5)
                    .color(tokens.muted),
                );
                ui.add_space(8.0);
                i18n::horizontal_wrapped(ui, |ui| {
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
        i18n::horizontal(ui, |ui| {
            i18n::vertical(ui, |ui| {
                ui.label(crate::i18n::rich("Settings actions").size(16.0).strong());
                ui.label(
                    crate::i18n::rich(
                        "Saving writes these settings to this device. Reset restores the defaults.",
                    )
                    .size(12.5)
                    .color(tokens.muted),
                );
            });
            ui.with_layout(i18n::trailing_layout(Align::Center), |ui| {
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
        ui.label(crate::i18n::rich("Appearance").size(16.0).strong());
        ui.add_space(6.0);
        i18n::horizontal_wrapped(ui, |ui| {
            for theme in Theme::ALL {
                if components::chip(ui, theme.label(), state.settings.theme == theme, touch)
                    .clicked()
                {
                    actions.push(UiAction::SetTheme(theme));
                }
            }
        });
        ui.label(
            crate::i18n::rich(state.settings.theme.description())
                .size(12.0)
                .color(tokens.muted),
        );

        ui.add_space(8.0);
        ui.label(crate::i18n::rich("Language").size(11.0).color(tokens.muted));
        i18n::horizontal_wrapped(ui, |ui| {
            for language in Language::ALL {
                if components::chip(
                    ui,
                    language.label(),
                    state.settings.language == language,
                    touch,
                )
                .clicked()
                {
                    actions.push(UiAction::SetLanguage(language));
                }
            }
        });
        ui.label(
            crate::i18n::rich(state.settings.language.description())
                .size(12.0)
                .color(tokens.muted),
        );
    });
}

fn updates_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    cx: &ViewContext,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(crate::i18n::rich("Updates").size(16.0).strong());
        ui.add_space(6.0);
        ui.label(
            crate::i18n::rich(i18n::fill("Current version {}", &[&cx.version]))
                .size(12.5)
                .color(tokens.muted),
        );
        let mut check_updates = state.settings.check_updates;
        if ui
            .checkbox(&mut check_updates, t("Check automatically on startup"))
            .changed()
        {
            actions.push(UiAction::SetCheckUpdates(check_updates));
        }
        ui.add_space(8.0);
        match state.update.phase {
            UpdatePhase::Idle | UpdatePhase::UpToDate | UpdatePhase::Failed => {
                i18n::horizontal_wrapped(ui, |ui| {
                    if components::button(
                        ui,
                        "Check for updates",
                        ButtonKind::Secondary,
                        touch,
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::CheckUpdates);
                    }
                });
                if state.update.phase == UpdatePhase::UpToDate {
                    ui.label(
                        crate::i18n::rich("You are up to date.")
                            .size(12.5)
                            .color(tokens.success),
                    );
                }
                if let Some(error) = state.update.error.as_deref() {
                    ui.label(crate::i18n::rich(error).size(12.0).color(tokens.muted));
                }
            }
            UpdatePhase::Checking => {
                i18n::horizontal(ui, |ui| {
                    ui.spinner();
                    ui.label(crate::i18n::rich("Checking for updates…").size(12.5));
                });
            }
            UpdatePhase::Available => {
                if let Some(release) = state.update.release.as_ref() {
                    ui.label(
                        crate::i18n::rich(format!("{} {}", t("Update available"), release.version))
                            .size(13.5)
                            .strong(),
                    );
                    if !release.notes.trim().is_empty() {
                        ui.label(
                            crate::i18n::rich("What's new")
                                .size(11.0)
                                .color(tokens.muted),
                        );
                        ui.label(
                            crate::i18n::rich(release.notes.trim())
                                .size(12.0)
                                .color(tokens.muted),
                        );
                    }
                    ui.add_space(6.0);
                    i18n::horizontal_wrapped(ui, |ui| {
                        let label = if crate::update::install_kind()
                            == crate::update::InstallKind::Android
                        {
                            "Download update"
                        } else {
                            "Download and install"
                        };
                        if components::button(ui, label, ButtonKind::Primary, touch, true).clicked()
                        {
                            actions.push(UiAction::InstallUpdate);
                        }
                        if components::button(
                            ui,
                            "Open the download page",
                            ButtonKind::Ghost,
                            touch,
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::OpenAboutLink(AboutLink::Releases));
                        }
                    });
                }
            }
            UpdatePhase::Downloading => {
                let (received, total) = state.update.progress.unwrap_or((0, None));
                let text = match total {
                    Some(total) if total > 0 => format!(
                        "{} {:.0}%",
                        t("Downloading update…"),
                        received as f64 / total as f64 * 100.0
                    ),
                    _ => t("Downloading update…").to_owned(),
                };
                ui.label(crate::i18n::rich(text).size(12.5));
                ui.add(
                    egui::ProgressBar::new(match total {
                        Some(total) if total > 0 => received as f32 / total as f32,
                        _ => 0.0,
                    })
                    .animate(true),
                );
            }
            UpdatePhase::Verifying => {
                ui.label(crate::i18n::rich("Verifying update…").size(12.5));
            }
            UpdatePhase::Installing => {
                ui.label(crate::i18n::rich("Installing update…").size(12.5));
            }
            UpdatePhase::Restarting => {
                ui.label(
                    crate::i18n::rich("Update ready. dnsrace will restart.")
                        .size(12.5)
                        .color(tokens.success),
                );
            }
        }
        if crate::update::install_kind() == crate::update::InstallKind::Android {
            ui.label(
                crate::i18n::rich(
                    "This update cannot install itself on Android. Download the APK and open it to update.",
                )
                .size(11.5)
                .color(tokens.muted),
            );
        }
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
        ui.label(crate::i18n::rich("Benchmark defaults").size(16.0).strong());
        ui.add_space(6.0);

        let mut profile = state.settings.profile;
        let options = Profile::ALL.map(|profile| (profile, profile.label()));
        if components::segmented(ui, "settings-profile", &mut profile, &options, touch) {
            actions.push(UiAction::SetProfile(profile));
        }
        ui.label(
            crate::i18n::rich(profile.description())
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
                    i18n::vertical(ui, |ui| {
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
                    i18n::vertical(ui, |ui| {
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
                    i18n::vertical(ui, |ui| {
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
                    i18n::vertical(ui, |ui| {
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
                    i18n::vertical(ui, |ui| {
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
                    i18n::vertical(ui, |ui| {
                        ui.set_width(width);
                    });
                    ui.end_row();
                });
        } else {
            i18n::horizontal_wrapped(ui, |ui| {
                i18n::vertical(ui, |ui| {
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
                i18n::vertical(ui, |ui| {
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
                i18n::vertical(ui, |ui| {
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
                i18n::vertical(ui, |ui| {
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
                i18n::vertical(ui, |ui| {
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
            crate::i18n::rich("Editing any value switches the profile to Custom.")
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
    let label = ui.label(
        crate::i18n::rich(label)
            .size(11.0)
            .color(style.tokens.muted),
    );
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
        ui.label(
            crate::i18n::rich("Network and steering")
                .size(16.0)
                .strong(),
        );
        ui.add_space(6.0);

        ui.label(crate::i18n::rich("Protocol").size(11.0).color(tokens.muted));
        i18n::horizontal_wrapped(ui, |ui| {
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
            crate::i18n::rich("Address family")
                .size(11.0)
                .color(tokens.muted),
        );
        i18n::horizontal_wrapped(ui, |ui| {
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
            .checkbox(
                &mut include_system,
                t("Include servers found on this machine"),
            )
            .changed()
        {
            actions.push(UiAction::SetIncludeSystem(include_system));
        }
        let mut include_nearby = state.settings.include_nearby;
        if ui
            .checkbox(&mut include_nearby, t("Include nearby open resolvers"))
            .changed()
        {
            actions.push(UiAction::SetIncludeNearby(include_nearby));
        }

        ui.add_space(8.0);
        ui.label(
            crate::i18n::rich("Steering hostnames")
                .size(11.0)
                .color(tokens.muted),
        );
        let mut hostnames = state.settings.hostnames.clone();
        let response = ui.add(
            egui::TextEdit::multiline(&mut hostnames)
                .hint_text(t("One hostname per line"))
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
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(crate::i18n::rich("Data").size(16.0).strong());
        ui.add_space(6.0);

        ui.label(
            crate::i18n::rich(i18n::fill(
                "{} saved benchmark run(s)",
                &[&state.history.len()],
            ))
            .size(12.5)
            .color(tokens.muted),
        );
        ui.add_space(4.0);
        i18n::horizontal_wrapped(ui, |ui| {
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
        ui.label(crate::i18n::rich("Custom resolvers").size(13.0).strong());
        if state.settings.custom.is_empty() {
            ui.label(
                crate::i18n::rich("No custom resolvers yet.")
                    .size(12.0)
                    .color(tokens.muted),
            );
        } else {
            for (index, resolver) in state.settings.custom.iter().enumerate() {
                // Wrap on narrow phone rows: two long action labels used to overflow the
                // right-to-left group and paint over the resolver name.
                i18n::horizontal_wrapped(ui, |ui| {
                    ui.label(crate::i18n::rich(&resolver.name).size(12.5));
                    if components::button(
                        ui,
                        &i18n::fill("Edit {}", &[&resolver.name]),
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
                        &i18n::fill("Remove {}", &[&resolver.name]),
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

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(crate::i18n::rich("Diagnostics").size(13.0).strong());
        i18n::horizontal_wrapped(ui, |ui| {
            if components::button(ui, "Copy diagnostics", ButtonKind::Secondary, touch, true)
                .clicked()
            {
                actions.push(UiAction::CopyDiagnostics);
            }
        });

        if !cx.capabilities.can_export_files {
            return;
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(
            crate::i18n::rich("Import resolvers and favourites")
                .size(13.0)
                .strong(),
        );
        ui.label(
            crate::i18n::rich("Leave the file blank to use the default export location.")
                .size(11.5)
                .color(tokens.muted),
        );
        let mut path = state.import_path.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut path)
                .hint_text(t("Path to a JSON file"))
                .desired_width(f32::INFINITY)
                .margin(Vec2::new(10.0, 8.0)),
        );
        if response.changed() {
            actions.push(UiAction::SetImportPath(path));
        }
        i18n::horizontal_wrapped(ui, |ui| {
            if components::button(ui, "Import", ButtonKind::Secondary, touch, true).clicked() {
                actions.push(UiAction::ImportResolverData);
            }
            if components::button(
                ui,
                "Export resolvers and favourites",
                ButtonKind::Secondary,
                touch,
                true,
            )
            .clicked()
            {
                actions.push(UiAction::ExportResolverData);
            }
        });
    });
}
