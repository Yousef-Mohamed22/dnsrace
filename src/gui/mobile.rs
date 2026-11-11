//! The phone shell: a compact app bar, the shared page content, and a fixed bottom navigation
//! bar with four 56-point destinations.
//!
//! Content keeps the shared page state and actions. The shell only changes framing, spacing and
//! touch sizes, so a control tested on desktop has the same effect here.

use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Vec2};

use crate::gui::actions::{Page, RunScope, UiAction};
use crate::gui::components::{self, ButtonKind};
use crate::gui::icons::{self, Icon};
use crate::gui::pages;
use crate::gui::state::{AppState, ViewContext};
use crate::gui::theme::{self, Tokens};

const NAV_HEIGHT: f32 = 64.0;
const TAB_HEIGHT: f32 = 56.0;
const CONTENT_PADDING: f32 = 16.0;

/// Draw the mobile shell into the available `Ui`.
pub fn render_mobile(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    // Every interactive widget inherits a 44-point minimum from the shell. Explicit buttons
    // use 48 for primary actions; this covers checkboxes, drag values and text fields.
    ui.spacing_mut().interact_size = Vec2::splat(44.0);

    // A child that is a little too wide must never drag the whole shell past the phone edge.
    // Derive the shell width from the viewport and the panel inset instead of trusting the
    // (possibly already expanded) available width.
    let clip = ui.clip_rect();
    let panel_inset = (ui.max_rect().min.x - clip.min.x).max(0.0);
    let shell_width = (clip.width() - 2.0 * panel_inset).max(0.0);
    ui.set_width(shell_width);

    // Status/navigation bars and display cutouts are reported by the platform. The phone
    // shell keeps every control inside that safe area. Android insets arrive through the
    // activity host (egui-winit only reports them on iOS); take whichever source is wider.
    let reported = cx.safe_area;
    let window = ui.ctx().input(|input| input.safe_area_insets()).0;
    let safe_left = (reported.left as f32).max(window.left).max(0.0);
    let safe_right = (reported.right as f32).max(window.right).max(0.0);
    let safe_top = (reported.top as f32).max(window.top).max(0.0);
    let safe_bottom = (reported.bottom as f32).max(window.bottom).max(0.0);

    if safe_top > 0.0 {
        ui.add_space(safe_top);
    }
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: (CONTENT_PADDING + safe_left).round() as i8,
            right: (CONTENT_PADDING + safe_right).round() as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| app_bar(state, cx, ui, actions, tokens));
    ui.separator();
    ui.set_max_width(shell_width);

    // Reserve the content frame margins, the bottom bar and the gap before it so the bar
    // cannot be clipped by the window edge.
    let content_margin = 8.0;
    let nav_gap = ui.spacing().item_spacing.y;
    let content_height =
        (ui.available_height() - NAV_HEIGHT - safe_bottom - content_margin * 2.0 - nav_gap)
            .max(120.0);
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: (CONTENT_PADDING + safe_left).round() as i8,
            right: (CONTENT_PADDING + safe_right).round() as i8,
            top: content_margin as i8,
            bottom: content_margin as i8,
        })
        .show(ui, |ui| {
            // Keep the frame from growing past the phone width when page content is wide.
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt(("mobile-page", state.page.label()))
                .max_height(content_height)
                // A phone scrolls by touch; a floating desktop scrollbar only obscures cards.
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                )
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(notice) = state.notices.last() {
                        components::notice(ui, &notice.text, notice.error);
                        ui.add_space(8.0);
                    }
                    match state.page {
                        Page::Overview => pages::render_overview(state, cx, ui, actions),
                        Page::Servers => pages::render_servers(state, cx, ui, actions),
                        Page::History => pages::render_history(state, cx, ui, actions),
                        Page::Settings => pages::render_settings(state, cx, ui, actions),
                    }
                });
        });

    let nav_width = (shell_width - safe_left - safe_right).max(0.0);
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: safe_left.round() as i8,
            right: safe_right.round() as i8,
            top: 0,
            bottom: safe_bottom.round() as i8,
        })
        .show(ui, |ui| bottom_nav(state, ui, actions, tokens, nav_width));
}

fn app_bar(
    state: &AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
        if ui.is_rect_visible(rect) {
            icons::paint_logo(ui.painter(), rect);
        }
        ui.vertical(|ui| {
            ui.label(RichText::new("dnsrace").size(18.0).strong());
            ui.label(RichText::new(cx.version).size(11.0).color(tokens.muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::button(
                ui,
                "Run benchmark",
                ButtonKind::Primary,
                true,
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
        });
    });
    ui.add_space(4.0);
}

fn bottom_nav(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    width: f32,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, NAV_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            // Tabs divide the row exactly; the default item spacing would push the last tab
            // past the phone width.
            ui.spacing_mut().item_spacing.x = 0.0;
            let tab_width = width / Page::ALL.len() as f32;
            for page in Page::ALL {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(tab_width, TAB_HEIGHT), Sense::click());
                if ui.is_rect_visible(rect) {
                    if state.page == page {
                        let indicator = Rect::from_min_size(
                            Pos2::new(rect.center().x - 16.0, rect.top()),
                            Vec2::new(32.0, 3.0),
                        );
                        ui.painter().rect_filled(indicator, 2.0, tokens.accent);
                    }
                    let color = if state.page == page {
                        tokens.accent
                    } else {
                        tokens.muted
                    };
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(rect.center().x, rect.top() + 20.0),
                        Vec2::splat(21.0),
                    );
                    icons::paint_at(ui.painter(), page_icon(page), icon_rect, color);
                    ui.painter().text(
                        Pos2::new(rect.center().x, rect.top() + 40.0),
                        egui::Align2::CENTER_CENTER,
                        page.label(),
                        egui::FontId::proportional(11.0),
                        color,
                    );
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::Role::Button,
                        true,
                        state.page == page,
                        page.label(),
                    )
                });
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() && state.page != page {
                    actions.push(UiAction::Navigate(page));
                }
            }
        },
    );
}

fn page_icon(page: Page) -> Icon {
    match page {
        Page::Overview => Icon::Gauge,
        Page::Servers => Icon::Server,
        Page::History => Icon::History,
        Page::Settings => Icon::Settings,
    }
}

/// Status background for tests and future safe-area padding.
pub const _SURFACE: Color32 = Color32::TRANSPARENT;
