//! The Overview page: the network in use, the benchmark setup, the recommendation and the
//! measured results, in the order a person needs them.

use std::net::IpAddr;

use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

use crate::bench::{Row, SecurityStatus};
use crate::gui::actions::{RunScope, UiAction};
use crate::gui::components::{self, ButtonKind, Tone};
use crate::gui::icons::{self, Icon};
use crate::gui::state::{AppState, LayoutMode, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::profile::Profile;
use crate::recommend::Recommendation;

const CARD_GAP: f32 = 12.0;
const PAGE_MAX_WIDTH: f32 = 1180.0;

/// Draw the desktop Overview. The same renderer will also feed the tablet/mobile layout once the
/// mobile shell is in place.
pub fn render_overview(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    let touch = cx.layout == LayoutMode::Mobile;
    ui.set_max_width(ui.available_width().min(PAGE_MAX_WIDTH));

    // A side column narrower than ~300 pt makes the recommendation pills and metric tiles
    // cramped, especially after system-bar insets in landscape.
    let two_columns = ui.available_width() >= 820.0;
    if two_columns {
        ui.horizontal_top(|ui| {
            let gap = 16.0;
            let main_width = ((ui.available_width() - gap) * 0.62).max(420.0);
            ui.allocate_ui_with_layout(
                Vec2::new(main_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    current_dns_card(state, cx, ui, actions, tokens, touch);
                    ui.add_space(CARD_GAP);
                    profile_card(state, ui, actions, tokens, touch);
                    ui.add_space(CARD_GAP);
                    results_card(state, ui, actions, tokens, touch);
                },
            );
            ui.add_space(gap);
            let side_width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(side_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    recommendation_card(state, cx, ui, actions, tokens, touch);
                    ui.add_space(CARD_GAP);
                    progress_card(state, cx, ui, actions, tokens, touch);
                },
            );
        });
    } else {
        current_dns_card(state, cx, ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
        recommendation_card(state, cx, ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
        profile_card(state, ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
        progress_card(state, cx, ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
        results_card(state, ui, actions, tokens, touch);
    }
}

fn card_heading(ui: &mut egui::Ui, icon: Icon, title: &str, tokens: Tokens) {
    ui.horizontal(|ui| {
        icons::paint_in(ui, icon, 18.0, tokens.accent, Sense::hover());
        ui.label(RichText::new(title).size(16.0).strong());
    });
}

fn current_dns_card(
    state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            card_heading(ui, Icon::Server, "Current DNS", tokens);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::button(
                    ui,
                    "Refresh detection",
                    ButtonKind::Ghost,
                    touch,
                    cx.capabilities.can_detect_system_dns && !state.running,
                )
                .clicked()
                {
                    actions.push(UiAction::RefreshDetection);
                }
            });
        });
        ui.add_space(8.0);

        let current = state.current_dns();
        if current.is_empty() {
            ui.label(
                RichText::new("Not detected yet")
                    .size(15.0)
                    .color(tokens.muted),
            );
            ui.label(
                RichText::new("Refresh detection to compare a future run with this network.")
                    .size(12.0)
                    .color(tokens.muted),
            );
        } else {
            ui.label(
                RichText::new(join_addresses(&current))
                    .monospace()
                    .size(19.0)
                    .strong(),
            );
            if let Some(system) = &state.system {
                let mut details = Vec::new();
                if let Some(interface) = &system.interface {
                    details.push(format!("on {interface}"));
                }
                if let Some(source) = &system.source {
                    details.push(format!("via {source}"));
                }
                if let Some(matched) = state.recommendation().current {
                    details.push(format!("matched {}", matched.label));
                }
                if !details.is_empty() {
                    ui.label(
                        RichText::new(details.join(" · "))
                            .size(12.0)
                            .color(tokens.muted),
                    );
                }
            }
        }
    });
}

fn profile_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        card_heading(ui, Icon::Gauge, "Benchmark setup", tokens);
        ui.add_space(8.0);
        ui.label(RichText::new("Profile").size(12.0).color(tokens.muted));

        // A profile change during a run would desync the report from the settings that
        // produced it; disable the whole control until the run finishes.
        ui.add_enabled_ui(!state.running, |ui| {
            let mut profile = state.settings.profile;
            let options = Profile::ALL.map(|profile| (profile, profile.label()));
            if components::segmented(ui, "overview-profile", &mut profile, &options, touch) {
                actions.push(UiAction::SetProfile(profile));
            }
            ui.label(
                RichText::new(profile.description())
                    .size(13.0)
                    .color(tokens.muted),
            );
        });

        let values = state.settings.profile.values();
        let detail = match values {
            Some(values) => format!(
                "{} ms timeout · {} jobs · {} repeat · {} cached rounds · {} random lookups",
                values.timeout_ms,
                values.jobs,
                values.repeat,
                values.cached_rounds,
                values.random_lookups
            ),
            None => {
                let depth = state.settings.depth();
                format!(
                    "{} ms timeout · {} jobs · {} repeat · {} cached rounds · {} random lookups",
                    state.settings.timeout_ms,
                    state.settings.jobs,
                    state.settings.repeat,
                    depth.cached_rounds,
                    depth.random_lookups
                )
            }
        };
        ui.label(
            RichText::new(detail)
                .monospace()
                .size(11.5)
                .color(tokens.muted),
        );

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        let selected = state.selected_in_catalog();
        let visible = state.visible_catalog().len();
        ui.horizontal_wrapped(|ui| {
            if components::button(
                ui,
                "Test selected",
                ButtonKind::Secondary,
                touch,
                !state.running && selected > 0,
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::Selected));
            }
            if components::button(
                ui,
                "Test all",
                ButtonKind::Secondary,
                touch,
                !state.running && visible > 0,
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::All));
            }
        });
        // Keep the counts on their own line: beside two touch-sized buttons they wrap
        // mid-phrase on a 360 dp phone.
        ui.label(
            RichText::new(format!("{selected} selected · {visible} available"))
                .size(12.0)
                .color(tokens.muted),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let favorites = state.favourites_in_catalog();
            if components::button(
                ui,
                "Test favourites",
                ButtonKind::Ghost,
                touch,
                !state.running && favorites > 0,
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::Favorites));
            }
            if components::button(
                ui,
                "Test current",
                ButtonKind::Ghost,
                touch,
                !state.running && !state.current_dns().is_empty(),
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::CurrentDns));
            }
        });
        if state.favourites_in_catalog() == 0 {
            ui.label(
                RichText::new(if state.settings.favorites.is_empty() {
                    "Star at least one resolver in Servers to enable Test favourites."
                } else {
                    "No favourite is available for the current protocol."
                })
                .size(11.5)
                .color(tokens.muted),
            );
        }
    });
}

fn recommendation_card(
    state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        card_heading(ui, Icon::Check, "Recommendation", tokens);
        ui.add_space(8.0);

        let recommendation = state.recommendation();
        let Some(overall) = recommendation.overall.clone() else {
            ui.label(
                RichText::new("No recommendation yet")
                    .size(15.0)
                    .color(tokens.muted),
            );
            ui.label(
                RichText::new("Run a benchmark and the best resolver will be explained here.")
                    .size(12.0)
                    .color(tokens.muted),
            );
            return;
        };

        // A long provider name next to the latency block used to widen the card past the
        // phone viewport, which then pushed the whole shell (and the bottom navigation) out
        // of bounds. Stack the two blocks when the card is narrow.
        if ui.available_width() < 360.0 {
            ui.label(RichText::new(&overall.label).size(22.0).strong());
            ui.label(
                RichText::new(&overall.endpoint)
                    .monospace()
                    .size(12.0)
                    .color(tokens.muted),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Latency").size(11.0).color(tokens.muted));
                ui.label(
                    RichText::new(format!("{:.1} ms", overall.score))
                        .monospace()
                        .size(22.0)
                        .strong()
                        .color(tokens.accent),
                );
            });
        } else {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&overall.label).size(22.0).strong());
                    ui.label(
                        RichText::new(&overall.endpoint)
                            .monospace()
                            .size(12.0)
                            .color(tokens.muted),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Latency").size(11.0).color(tokens.muted));
                        ui.label(
                            RichText::new(format!("{:.1} ms", overall.score))
                                .monospace()
                                .size(26.0)
                                .strong()
                                .color(tokens.accent),
                        );
                    });
                });
            });
        }

        if !overall.description.is_empty() {
            ui.label(
                RichText::new(&overall.description)
                    .size(12.0)
                    .color(tokens.muted),
            );
        }

        if !recommendation.reasons.is_empty() {
            ui.add_space(6.0);
            // Wrap whole pills; a fixed pair per row overflows a narrow phone card.
            ui.horizontal_wrapped(|ui| {
                for reason in &recommendation.reasons {
                    components::pill(ui, reason.label(), Tone::Accent);
                }
            });
        }

        if let Some(delta) = delta_text(&recommendation) {
            let faster = recommendation.delta_ms.is_some_and(|value| value >= 0.0);
            ui.label(RichText::new(delta).size(12.0).color(if faster {
                tokens.success
            } else {
                tokens.warning
            }));
        }
        if let Some(gap) = recommendation.close_call_ms {
            ui.label(
                RichText::new(format!(
                    "The top two are within {gap:.1} ms; either is a good choice."
                ))
                .size(12.0)
                .color(tokens.warning),
            );
        }

        if cx.capabilities.can_apply_dns {
            ui.add_space(6.0);
            let chosen_encrypted = recommendation
                .chosen
                .as_ref()
                .is_some_and(|row| !row.is_plain());
            if chosen_encrypted {
                ui.label(
                    RichText::new(
                        "You selected an encrypted resolver. This system can only apply a plain \
                         resolver, so choose one from the results first.",
                    )
                    .size(12.0)
                    .color(tokens.warning),
                );
                let _ =
                    components::button(ui, "Use this resolver", ButtonKind::Primary, touch, false);
            } else if let Some(target) = recommendation.best_plain.clone() {
                if target.id != overall.id {
                    ui.label(
                        RichText::new(format!(
                            "Fastest applicable resolver: {} ({})",
                            target.label, target.endpoint
                        ))
                        .size(12.0)
                        .color(tokens.muted),
                    );
                }
                if components::button(
                    ui,
                    "Use this resolver",
                    ButtonKind::Primary,
                    touch,
                    !state.running,
                )
                .clicked()
                {
                    actions.push(UiAction::Apply(target.id));
                }
            } else {
                ui.label(
                    RichText::new(
                        "No plain resolver was measured, so this system has nothing to apply from \
                         this run.",
                    )
                    .size(12.0)
                    .color(tokens.warning),
                );
            }
        } else if cx.capabilities.is_android() {
            ui.add_space(6.0);
            if components::button(
                ui,
                "Set up this resolver",
                ButtonKind::Primary,
                touch,
                !state.running,
            )
            .clicked()
            {
                actions.push(UiAction::OpenGuide(overall.id.clone()));
            }
        }

        if let Some(encrypted) = recommendation
            .best_encrypted
            .clone()
            .filter(|row| row.id != overall.id)
        {
            ui.separator();
            ui.label(
                RichText::new("Best encrypted")
                    .size(11.0)
                    .color(tokens.muted),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&encrypted.label).size(13.0).strong());
                ui.label(
                    RichText::new(format!(
                        "{:.1} ms · {}",
                        encrypted.score, encrypted.endpoint
                    ))
                    .monospace()
                    .size(11.5)
                    .color(tokens.muted),
                );
            });
        }
    });
}

fn progress_card(
    state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        card_heading(ui, Icon::Gauge, "Progress", tokens);
        ui.add_space(8.0);

        if state.running {
            ui.label(
                RichText::new(
                    state
                        .stage
                        .map_or("Preparing the benchmark", |stage| stage.label()),
                )
                .size(13.0),
            );
            let (done, total) = state.progress;
            let fraction = if total == 0 {
                0.0
            } else {
                done as f32 / total as f32
            };
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .text(format!("{done} of {total}")),
            );
            ui.add_space(4.0);
            if components::button(ui, "Cancel", ButtonKind::Danger, touch, true).clicked() {
                actions.push(UiAction::Cancel);
            }
            return;
        }

        let Some(result) = &state.result else {
            ui.label(RichText::new("No run yet").size(15.0).color(tokens.muted));
            ui.label(
                RichText::new("The last run's duration, depth and data center appear here.")
                    .size(12.0)
                    .color(tokens.muted),
            );
            return;
        };

        ui.label(RichText::new("Last run").size(12.0).color(tokens.muted));
        ui.horizontal_wrapped(|ui| {
            components::metric(
                ui,
                "Duration",
                &format!("{:.1} s", result.duration_secs),
                None,
            );
            components::metric(
                ui,
                "Tested",
                &result.tested.to_string(),
                Some(&format!("of {} scanned", result.scanned)),
            );
        });
        let profile = result.profile.map_or("Custom", Profile::label);
        let mut detail = format!("Profile: {profile}");
        if let Some(depth) = result.depth {
            detail.push_str(&format!(
                " · {} cached rounds · {} random lookups",
                depth.cached_rounds, depth.random_lookups
            ));
        }
        if let Some(data_center) = &result.data_center {
            detail.push_str(&format!(" · {data_center}"));
        }
        ui.label(
            RichText::new(detail)
                .monospace()
                .size(11.5)
                .color(tokens.muted),
        );

        if cx.capabilities.is_android() {
            ui.add_space(8.0);
            if components::button(ui, "Share report", ButtonKind::Secondary, touch, true).clicked()
            {
                actions.push(UiAction::ShareReport(
                    crate::gui::actions::ReportFormat::Json,
                ));
            }
        }
    });
}

fn results_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            card_heading(ui, Icon::Server, "Top results", tokens);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let measured = state.result.as_ref().map_or(0, |result| result.rows.len());
                ui.label(
                    RichText::new(format!("{measured} measured"))
                        .size(12.0)
                        .color(tokens.muted),
                );
            });
        });
        ui.add_space(6.0);

        let Some(result) = &state.result else {
            ui.label(
                RichText::new("Results appear here after a benchmark.")
                    .size(13.0)
                    .color(tokens.muted),
            );
            return;
        };

        let mut rows: Vec<&Row> = result
            .rows
            .iter()
            .filter(|row| row.recommendable())
            .collect();
        if rows.is_empty() {
            rows = result.rows.iter().collect();
        }
        rows.sort_by(|a, b| a.score.total_cmp(&b.score));

        if rows.is_empty() {
            ui.label(
                RichText::new("This run did not produce a usable row.")
                    .size(13.0)
                    .color(tokens.muted),
            );
            return;
        }

        if touch {
            touch_result_rows(state, ui, actions, tokens, &rows);
            return;
        }

        egui::Grid::new("overview-top-results")
            .num_columns(3)
            .spacing([12.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Resolver").size(11.0).color(tokens.muted));
                ui.label(RichText::new("Score").size(11.0).color(tokens.muted));
                ui.label(RichText::new("Security").size(11.0).color(tokens.muted));
                ui.end_row();

                for row in rows.iter().take(6) {
                    let selected = state.selected_row.as_deref() == Some(row.id.as_str());
                    if ui
                        .selectable_label(selected, RichText::new(&row.label).size(13.0))
                        .clicked()
                    {
                        actions.push(UiAction::ChooseRow(row.id.clone()));
                    }
                    ui.label(
                        RichText::new(format!("{:.1} ms", row.score))
                            .monospace()
                            .size(12.5),
                    );
                    components::pill(
                        ui,
                        security_label(row.security_status()),
                        security_tone(row),
                    );
                    ui.end_row();
                }
            });
    });
}

/// Mobile results: a 48 dp row per resolver instead of a three-column grid. Long labels such
/// as `Current DNS (163.121.128.134)` used to widen the grid and push the security column off
/// the phone screen.
fn touch_result_rows(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    rows: &[&Row],
) {
    for row in rows.iter().take(6) {
        let selected = state.selected_row.as_deref() == Some(row.id.as_str());
        let score_width = 74.0;
        let button_width = (ui.available_width() - score_width - 8.0).max(120.0);
        ui.horizontal(|ui| {
            let response = ui.add_sized(
                [button_width, 48.0],
                egui::Button::selectable(selected, RichText::new(&row.label).size(14.0).strong())
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    // Pin the name to the leading edge; a plain button centres it.
                    .right_text(" "),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::Role::Button,
                    true,
                    selected,
                    format!("Choose {}", row.label),
                )
            });
            if response.clicked() {
                actions.push(UiAction::ChooseRow(row.id.clone()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.1} ms", row.score))
                        .monospace()
                        .size(12.5)
                        .color(tokens.text),
                );
            });
        });
        ui.horizontal_wrapped(|ui| {
            components::pill(
                ui,
                security_label(row.security_status()),
                security_tone(row),
            );
        });
        ui.add_space(4.0);
    }
}

fn security_tone(row: &Row) -> Tone {
    match row.security_status() {
        SecurityStatus::Validated => Tone::Success,
        SecurityStatus::Insecure => Tone::Neutral,
        SecurityStatus::Partial | SecurityStatus::Unknown => Tone::Warning,
        SecurityStatus::BogusAccepted => Tone::Danger,
    }
}

fn security_label(status: SecurityStatus) -> &'static str {
    match status {
        SecurityStatus::Validated => "DNSSEC",
        SecurityStatus::Insecure => "No DNSSEC",
        SecurityStatus::Partial => "DNSSEC partial",
        SecurityStatus::BogusAccepted => "Bogus accepted",
        SecurityStatus::Unknown => "DNSSEC unknown",
    }
}

fn delta_text(recommendation: &Recommendation) -> Option<String> {
    let delta = recommendation.delta_ms?;
    if delta.abs() < 0.5 {
        return Some("Same as your current DNS".to_string());
    }
    let percentage = recommendation
        .delta_pct
        .map(|value| format!(" ({value:.0}%)"))
        .unwrap_or_default();
    if delta > 0.0 {
        Some(format!(
            "{delta:.1} ms faster than your current DNS{percentage}"
        ))
    } else {
        Some(format!(
            "{:.1} ms slower than your current DNS{percentage}",
            -delta
        ))
    }
}

fn join_addresses(addresses: &[IpAddr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
