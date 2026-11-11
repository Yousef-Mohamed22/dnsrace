//! The History page: a run list, a winner trend, and the details of one saved run.
//!
//! History entries are immutable snapshots. Actions are emitted for reopen, export, delete and
//! clear-all; the app owns the history vector and the confirmation state.

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};

use crate::config::HistoryEntry;
use crate::gui::actions::{ReportFormat, UiAction};
use crate::gui::components::{self, ButtonKind, Tone};
use crate::gui::icons::{self, Icon};
use crate::gui::state::{AppState, LayoutMode, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::profile::Profile;
use crate::util::format_time;

const PAGE_MAX_WIDTH: f32 = 1180.0;
const CARD_GAP: f32 = 12.0;
const DETAIL_MIN_WIDTH: f32 = 300.0;
const LIST_MIN_WIDTH: f32 = 420.0;

/// Draw the desktop History page.
pub fn render_history(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    let touch = cx.layout == LayoutMode::Mobile;
    ui.set_max_width(ui.available_width().min(PAGE_MAX_WIDTH));

    if state.confirm_clear_history {
        clear_confirm_card(ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
    }

    if state.history.is_empty() {
        components::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No saved runs yet").size(16.0).strong());
            ui.label(
                RichText::new("Finish a benchmark and it will appear here with its winner.")
                    .size(13.0)
                    .color(tokens.muted),
            );
        });
        return;
    }

    let wide = ui.available_width() >= LIST_MIN_WIDTH + DETAIL_MIN_WIDTH + 32.0;

    if !wide && state.history_selected.is_some() {
        detail_card(state, ui, actions, tokens, touch);
        ui.add_space(CARD_GAP);
    }

    trend_card(state, ui, tokens);
    ui.add_space(CARD_GAP);

    if wide {
        ui.horizontal_top(|ui| {
            let list_width = (ui.available_width() - DETAIL_MIN_WIDTH - 16.0).max(LIST_MIN_WIDTH);
            ui.allocate_ui_with_layout(
                Vec2::new(list_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| list_card(state, ui, actions, tokens, touch),
            );
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    if state.history_selected.is_some() {
                        detail_card(state, ui, actions, tokens, touch);
                    } else {
                        detail_placeholder(ui, tokens);
                    }
                },
            );
        });
    } else {
        list_card(state, ui, actions, tokens, touch);
        if state.history_selected.is_none() {
            ui.add_space(CARD_GAP);
            detail_placeholder(ui, tokens);
        }
    }
}

fn trend_card(state: &AppState, ui: &mut egui::Ui, tokens: Tokens) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icons::paint_in(ui, Icon::History, 18.0, tokens.accent, Sense::hover());
            ui.label(RichText::new("Winner trend").size(16.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let scores = recent_scores(state);
                let measurable = scores.iter().filter(|score| score.is_some()).count();
                ui.label(
                    RichText::new(format!("{measurable} measured run(s)"))
                        .size(12.0)
                        .color(tokens.muted),
                );
            });
        });
        ui.add_space(6.0);

        let scores = recent_scores(state);
        let finite: Vec<f64> = scores.iter().flatten().copied().collect();
        if finite.len() < 2 {
            ui.label(
                RichText::new("A trend appears after two runs with a measurable winner.")
                    .size(13.0)
                    .color(tokens.muted),
            );
            return;
        }

        let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
        let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = (max - min).max(1.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 92.0), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let left = rect.left() + 8.0;
        let right = rect.right() - 8.0;
        let top = rect.top() + 8.0;
        let bottom = rect.bottom() - 8.0;
        let slots = scores.len().max(2);
        let x_at = |index: usize| left + (right - left) * (index as f32 / (slots - 1) as f32);
        let y_at = |score: f64| bottom - (((score - min) / span) as f32) * (bottom - top);

        ui.painter().line_segment(
            [Pos2::new(left, bottom), Pos2::new(right, bottom)],
            Stroke::new(1.0, tokens.border),
        );

        let points: Vec<Pos2> = scores
            .iter()
            .enumerate()
            .filter_map(|(index, score)| score.map(|score| Pos2::new(x_at(index), y_at(score))))
            .collect();
        if points.len() >= 2 {
            ui.painter()
                .add(Shape::line(points.clone(), Stroke::new(2.0, tokens.accent)));
        }
        for (index, score) in scores.iter().enumerate() {
            let Some(score) = score else { continue };
            let point = Pos2::new(x_at(index), y_at(*score));
            ui.painter().circle_filled(point, 3.2, tokens.accent);
            let hit = Rect::from_center_size(point, Vec2::splat(18.0));
            ui.interact(hit, ui.scope_id().with(("trend", index)), Sense::hover())
                .on_hover_text(format!("{score:.1} ms"));
        }
    });
}

fn recent_scores(state: &AppState) -> Vec<Option<f64>> {
    state
        .history
        .iter()
        .rev()
        .take(10)
        .map(|entry| entry.winner().map(|row| row.score))
        .collect()
}

fn list_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Saved runs").size(16.0).strong());
            ui.label(
                RichText::new(format!("{} run(s)", state.history.len()))
                    .size(12.0)
                    .color(tokens.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::button(
                    ui,
                    "Clear history",
                    ButtonKind::Ghost,
                    touch,
                    !state.history.is_empty(),
                )
                .clicked()
                {
                    actions.push(UiAction::RequestClearHistory);
                }
            });
        });
        ui.add_space(6.0);

        for (index, entry) in state.history.iter().enumerate() {
            let selected = state.history_selected == Some(index);
            run_row(ui, index, entry, selected, actions, tokens);
            ui.add_space(2.0);
        }
    });
}

fn run_row(
    ui: &mut egui::Ui,
    index: usize,
    entry: &HistoryEntry,
    selected: bool,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            tokens.accent.gamma_multiply(0.14)
        } else if response.hovered() {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 8.0, fill);
        }
        let winner = entry
            .winner()
            .map(|row| format!("{} · {:.1} ms", row.label, row.score))
            .unwrap_or_else(|| "No recommendation".to_string());
        let detail = format!(
            "{} · {}",
            entry.protocol.label(),
            entry.profile.map_or("Custom", Profile::label)
        );
        ui.painter().text(
            Pos2::new(rect.left() + 10.0, rect.top() + 9.0),
            egui::Align2::LEFT_TOP,
            format_time(entry.timestamp),
            FontId::proportional(13.0),
            tokens.text,
        );
        ui.painter().text(
            Pos2::new(rect.left() + 10.0, rect.top() + 31.0),
            egui::Align2::LEFT_TOP,
            winner,
            FontId::proportional(12.0),
            tokens.muted,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.top() + 9.0),
            egui::Align2::RIGHT_TOP,
            detail,
            FontId::proportional(11.0),
            tokens.muted,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
            format!("Open run {}", index + 1),
        )
    });
    if response.clicked() {
        actions.push(UiAction::OpenHistory(index));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn detail_card(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        let Some(index) = state.history_selected else {
            detail_placeholder_contents(ui, tokens);
            return;
        };
        let Some(entry) = state.history.get(index) else {
            detail_placeholder_contents(ui, tokens);
            return;
        };

        ui.label(RichText::new("Run details").size(16.0).strong());
        ui.add_space(6.0);
        ui.label(RichText::new(entry.label()).size(13.5).strong());
        ui.label(
            RichText::new(format!(
                "Profile: {} · {} cached rounds · {} random lookups",
                entry.profile.map_or("Custom", Profile::label),
                entry.depth.map_or(0, |depth| depth.cached_rounds),
                entry.depth.map_or(0, |depth| depth.random_lookups)
            ))
            .monospace()
            .size(11.5)
            .color(tokens.muted),
        );

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            components::metric(
                ui,
                "Duration",
                &format!("{:.1} s", entry.duration_secs),
                None,
            );
            components::metric(ui, "Tested", &entry.tested.to_string(), None);
            components::metric(ui, "Scanned", &entry.scanned.to_string(), None);
        });

        if let Some(winner) = entry.winner() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                components::pill(
                    ui,
                    winner.security_status().label(),
                    match winner.security_status() {
                        crate::bench::SecurityStatus::Validated => Tone::Success,
                        crate::bench::SecurityStatus::BogusAccepted => Tone::Danger,
                        crate::bench::SecurityStatus::Insecure => Tone::Neutral,
                        _ => Tone::Warning,
                    },
                );
                components::pill(ui, winner.note.label(), Tone::Neutral);
            });
        }

        if !entry.current_dns.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Current DNS: {}",
                    join_addresses(&entry.current_dns)
                ))
                .monospace()
                .size(11.5)
                .color(tokens.muted),
            );
        }
        if let Some(data_center) = &entry.data_center {
            ui.label(
                RichText::new(format!("Data center: {data_center}"))
                    .size(11.5)
                    .color(tokens.muted),
            );
        }

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if components::button(ui, "Reopen run", ButtonKind::Primary, touch, true).clicked() {
                actions.push(UiAction::ReopenHistory(index));
            }
            if touch {
                // Android has no practical file-export path; the share sheet is the way to
                // get a report out of the phone.
                if components::button(ui, "Share report", ButtonKind::Secondary, true, true)
                    .clicked()
                {
                    actions.push(UiAction::ShareHistory(index, ReportFormat::Json));
                }
            } else {
                for format in [ReportFormat::Json, ReportFormat::Csv, ReportFormat::Html] {
                    if components::button(
                        ui,
                        &format!("Export {}", format.label()),
                        ButtonKind::Secondary,
                        touch,
                        true,
                    )
                    .clicked()
                    {
                        actions.push(UiAction::ExportHistory(index, format));
                    }
                }
            }
            if components::button(ui, "Delete run", ButtonKind::Danger, touch, true).clicked() {
                actions.push(UiAction::DeleteHistory(index));
            }
        });
    });
}

fn detail_placeholder(ui: &mut egui::Ui, tokens: Tokens) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        detail_placeholder_contents(ui, tokens);
    });
}

fn detail_placeholder_contents(ui: &mut egui::Ui, tokens: Tokens) {
    ui.label(RichText::new("Run details").size(16.0).strong());
    ui.add_space(4.0);
    ui.label(
        RichText::new("Select a saved run to reopen, export or delete it.")
            .size(13.0)
            .color(tokens.muted),
    );
}

fn clear_confirm_card(ui: &mut egui::Ui, actions: &mut Vec<UiAction>, tokens: Tokens, touch: bool) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.label(RichText::new("Delete all history?").size(15.0).strong());
            ui.label(
                RichText::new("This removes every saved run from this device.")
                    .size(12.5)
                    .color(tokens.muted),
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if components::button(ui, "Delete all runs", ButtonKind::Danger, touch, true)
                    .clicked()
                {
                    actions.push(UiAction::ClearHistory);
                }
                if components::button(ui, "Keep history", ButtonKind::Secondary, touch, true)
                    .clicked()
                {
                    actions.push(UiAction::CancelClearHistory);
                }
            });
        });
    });
}

fn join_addresses(addresses: &[std::net::IpAddr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
