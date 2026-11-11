//! The desktop chrome: a fixed left rail and a scrollable page.
//!
//! The rail carries the only primary run action, the four destinations and a compact network
//! status card. The page itself comes from `gui::pages`, so the same content can be framed by the
//! phone shell without being duplicated.

use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Vec2};

use crate::gui::actions::{Page, RunScope, UiAction};
use crate::gui::components::{self, ButtonKind, Tone};
use crate::gui::icons::{self, Icon};
use crate::gui::pages;
use crate::gui::state::{AppState, ViewContext};
use crate::gui::theme::{self, Tokens};

const RAIL_WIDTH: f32 = 232.0;
const CONTROL_HEIGHT: f32 = 40.0;

/// Draw the desktop shell into the available `Ui`.
pub fn render_desktop(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(RAIL_WIDTH);
            render_rail(state, cx, ui, actions, tokens);
        });
        ui.separator();
        ui.vertical(|ui| {
            render_content(state, cx, ui, actions, tokens);
        });
    });
}

fn render_rail(
    state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        paint_brand(ui);
        ui.vertical(|ui| {
            ui.label(RichText::new("dnsrace").size(18.0).strong());
            ui.label(RichText::new(cx.version).size(11.0).color(tokens.muted));
        });
    });

    ui.add_space(20.0);
    if components::button(
        ui,
        "Run benchmark",
        ButtonKind::Primary,
        false,
        !state.running,
    )
    .clicked()
    {
        let scope = if state.selected_in_catalog() == 0 {
            RunScope::All
        } else {
            RunScope::Selected
        };
        actions.push(UiAction::Run(scope));
    }

    ui.add_space(18.0);
    ui.label(
        RichText::new("WORKSPACE")
            .size(10.0)
            .strong()
            .color(tokens.muted),
    );
    ui.add_space(4.0);
    for page in Page::ALL {
        let response = nav_item(ui, page, state.page == page, tokens);
        if response.clicked() && state.page != page {
            actions.push(UiAction::Navigate(page));
        }
        ui.add_space(2.0);
    }

    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        network_card(state, ui, actions, tokens);
    });
}

fn render_content(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(page_heading(state.page)).size(22.0).strong());
            ui.label(
                RichText::new(state.page.description())
                    .size(13.0)
                    .color(tokens.muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, tone) = if state.running {
                ("Running", Tone::Accent)
            } else if state.result.is_some() {
                ("Results ready", Tone::Success)
            } else {
                ("Ready", Tone::Neutral)
            };
            components::pill(ui, label, tone);
        });
    });
    if let Some(notice) = state.notices.last() {
        ui.add_space(8.0);
        components::notice(ui, &notice.text, notice.error);
    }
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.page {
            Page::Overview => pages::render_overview(state, cx, ui, actions),
            Page::Servers => pages::render_servers(state, cx, ui, actions),
            Page::History => pages::render_history(state, cx, ui, actions),
            Page::Settings => pages::render_settings(state, cx, ui, actions),
        });
}

fn page_heading(page: Page) -> &'static str {
    match page {
        Page::Overview => "Connection overview",
        Page::Servers => "Resolver catalogue",
        Page::History => "Benchmark history",
        Page::Settings => "Preferences",
    }
}

fn nav_item(ui: &mut egui::Ui, page: Page, selected: bool, tokens: Tokens) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, CONTROL_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = if selected {
            tokens
                .accent
                .gamma_multiply(if ui.visuals().dark_mode { 0.16 } else { 0.10 })
        } else if hovered {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 10.0, fill);
        }
        if selected {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left(), rect.top() + 10.0),
                    Vec2::new(3.0, rect.height() - 20.0),
                ),
                2.0,
                tokens.accent,
            );
        }
        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 12.0, rect.center().y - 10.0),
            Vec2::splat(20.0),
        );
        icons::paint_at(
            ui.painter(),
            page_icon(page),
            icon_rect,
            if selected {
                tokens.accent
            } else {
                tokens.muted
            },
        );
        ui.painter().text(
            Pos2::new(rect.left() + 44.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            page.label(),
            egui::FontId::proportional(14.0),
            if selected { tokens.text } else { tokens.muted },
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, page.label()));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn page_icon(page: Page) -> Icon {
    match page {
        Page::Overview => Icon::Gauge,
        Page::Servers => Icon::Server,
        Page::History => Icon::History,
        Page::Settings => Icon::Settings,
    }
}

fn network_card(state: &AppState, ui: &mut egui::Ui, actions: &mut Vec<UiAction>, tokens: Tokens) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icons::paint_in(ui, Icon::Server, 16.0, tokens.success, Sense::hover());
            ui.label(RichText::new("Network status").size(13.0).strong());
        });
        ui.add_space(6.0);

        let current = state.current_dns();
        if current.is_empty() {
            ui.label(
                RichText::new("No DNS detected")
                    .size(12.0)
                    .color(tokens.muted),
            );
        } else {
            ui.label(
                RichText::new(join_addresses(&current))
                    .monospace()
                    .size(12.0),
            );
        }

        ui.add_space(8.0);
        if components::button(
            ui,
            "Copy current DNS",
            ButtonKind::Secondary,
            false,
            !current.is_empty(),
        )
        .clicked()
        {
            actions.push(UiAction::CopyCurrentDns);
        }
        if state.applied.is_some() {
            ui.add_space(4.0);
            if components::button(ui, "Restore previous DNS", ButtonKind::Ghost, false, true)
                .clicked()
            {
                actions.push(UiAction::Revert);
            }
        }
    });
}

fn paint_brand(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        icons::paint_logo(ui.painter(), rect);
    }
}

fn join_addresses(addresses: &[std::net::IpAddr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
