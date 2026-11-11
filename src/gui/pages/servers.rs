//! The Servers page: search the catalogue, filter it, choose what to test, and inspect one
//! resolver in detail.
//!
//! The table never mutates state. Every checkbox, star, row and detail action is emitted as a
//! `UiAction`, so the app remains the only place that changes selections or favourites.

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, StrokeKind, Vec2};
use egui_extras::{Column, TableBuilder};

use crate::bench::{Row, SecurityStatus};
use crate::gui::actions::{RunScope, UiAction};
use crate::gui::components::{self, ButtonKind, Tone};
use crate::gui::icons::{self, Icon};
use crate::gui::state::{AppState, LayoutMode, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::servers::{Candidate, Category, Family, Kind};

const PAGE_MAX_WIDTH: f32 = 1180.0;
const TABLE_MIN_WIDTH: f32 = 430.0;
const DETAIL_MIN_WIDTH: f32 = 280.0;
const ROW_HEIGHT: f32 = 38.0;
const CONTROL_HEIGHT: f32 = 40.0;

const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x12, 0x1F);

/// Draw the desktop Servers page.
pub fn render_servers(
    state: &mut AppState,
    cx: &ViewContext,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
) {
    let tokens = theme::tokens(state.settings.theme, ui.ctx().theme());
    let touch = cx.layout == LayoutMode::Mobile;
    ui.set_max_width(ui.available_width().min(PAGE_MAX_WIDTH));

    toolbar(state, ui, actions, tokens, touch);
    ui.add_space(8.0);
    filters(state, ui, actions, touch);
    ui.add_space(10.0);

    let visible: Vec<Candidate> = state.visible_catalog().into_iter().cloned().collect();

    if visible.is_empty() {
        components::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No resolvers match").size(16.0).strong());
            ui.label(
                RichText::new("Change the search or clear a filter to see the catalogue again.")
                    .size(13.0)
                    .color(tokens.muted),
            );
        });
        return;
    }

    if touch {
        mobile_cards(state, &visible, ui, actions, tokens);
        return;
    }

    if ui.available_width() >= TABLE_MIN_WIDTH + DETAIL_MIN_WIDTH + 32.0 {
        ui.horizontal_top(|ui| {
            let table_width = (ui.available_width() - DETAIL_MIN_WIDTH - 16.0).max(TABLE_MIN_WIDTH);
            ui.allocate_ui_with_layout(
                Vec2::new(table_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| table_card(state, &visible, ui, actions, tokens),
            );
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| detail_card(state, ui, actions, tokens),
            );
        });
    } else {
        table_card(state, &visible, ui, actions, tokens);
        ui.add_space(12.0);
        detail_card(state, ui, actions, tokens);
    }
}

fn toolbar(
    state: &AppState,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    let mut query = state.search.clone();
    if touch {
        ui.add_sized(
            [ui.available_width(), 48.0],
            egui::TextEdit::singleline(&mut query)
                .id_salt("servers-search")
                .hint_text("Search provider, label or address")
                .margin(Vec2::new(10.0, 12.0)),
        );
        if query != state.search {
            actions.push(UiAction::SetSearch(query));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if components::button(
                ui,
                "Test selected",
                ButtonKind::Primary,
                true,
                !state.running && state.selected_in_catalog() > 0,
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::Selected));
            }
            ui.label(
                RichText::new(format!(
                    "{} selected · {} available",
                    state.selected_in_catalog(),
                    state.visible_catalog().len()
                ))
                .size(12.0)
                .color(tokens.muted),
            );
        });
        return;
    }

    ui.horizontal(|ui| {
        let field_width = (ui.available_width() - 250.0).max(180.0);
        ui.allocate_ui_with_layout(
            Vec2::new(field_width, CONTROL_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.scope(|ui| {
                    ui.set_width((ui.available_width() - 40.0).max(140.0));
                    let response = components::search_field(
                        ui,
                        "servers-search",
                        &mut query,
                        "Search provider, label or address",
                    );
                    if response.changed() {
                        actions.push(UiAction::SetSearch(query.clone()));
                    }
                });
                if !state.search.is_empty()
                    && components::icon_button(ui, Icon::X, "Clear search", false).clicked()
                {
                    actions.push(UiAction::SetSearch(String::new()));
                }
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::button(
                ui,
                "Test selected",
                ButtonKind::Primary,
                false,
                !state.running && state.selected_in_catalog() > 0,
            )
            .clicked()
            {
                actions.push(UiAction::Run(RunScope::Selected));
            }
            ui.label(
                RichText::new(format!(
                    "{} selected · {} available",
                    state.selected_in_catalog(),
                    state.visible_catalog().len()
                ))
                .size(12.0)
                .color(tokens.muted),
            );
        });
    });
}

fn filters(state: &AppState, ui: &mut egui::Ui, actions: &mut Vec<UiAction>, touch: bool) {
    ui.horizontal_wrapped(|ui| {
        if components::chip(ui, "All categories", state.category_filter.is_none(), touch).clicked()
        {
            actions.push(UiAction::SetCategoryFilter(None));
        }
        for category in [
            Category::General,
            Category::Security,
            Category::Family,
            Category::Privacy,
            Category::Uncensored,
        ] {
            let selected = state.category_filter == Some(category);
            if components::chip(ui, category.label(), selected, touch).clicked() {
                actions.push(UiAction::SetCategoryFilter(if selected {
                    None
                } else {
                    Some(category)
                }));
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (kind, label) in [
            (None, "Any transport"),
            (Some(Kind::Plain), "Plain"),
            (Some(Kind::Dot), "DoT"),
            (Some(Kind::Doh), "DoH"),
        ] {
            let selected = state.kind_filter == kind;
            if components::chip(ui, label, selected, touch).clicked() {
                actions.push(UiAction::SetKindFilter(if selected { None } else { kind }));
            }
        }

        ui.add_space(6.0);
        for (family, label) in [
            (None, "Any address"),
            (Some(Family::V4), "IPv4"),
            (Some(Family::V6), "IPv6"),
        ] {
            let selected = state.family_filter == family;
            if components::chip(ui, label, selected, touch).clicked() {
                actions.push(UiAction::SetFamilyFilter(if selected {
                    None
                } else {
                    family
                }));
            }
        }

        ui.add_space(6.0);
        if components::chip(ui, "Favourites", state.favorites_only, touch).clicked() {
            actions.push(UiAction::SetFavoritesOnly(!state.favorites_only));
        }
    });
}

/// A touch-first catalogue: one card per resolver instead of a wide table. Tapping the name
/// opens the shared mobile detail sheet.
///
/// Phones have 100+ resolvers in the built-in catalogue. Laying every card out on every frame
/// makes the Servers tab crawl, so this list reserves one fixed slot per card and only builds
/// the cards that intersect the scroll viewport. Empty slots keep the scrollbar stable.
fn mobile_cards(
    state: &AppState,
    visible: &[Candidate],
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Resolvers").size(17.0).strong());
            ui.label(
                RichText::new(format!("{} shown", visible.len()))
                    .size(12.0)
                    .color(tokens.muted),
            );
        });
        ui.add_space(6.0);

        let line = ui.text_style_height(&egui::TextStyle::Small);
        let spacing = ui.spacing().item_spacing.y;
        let pill_height =
            ui.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(12.0))) + 8.0;
        let gap = 10.0;
        // Top row (44) + card margins (20) + endpoint line + pill line + inner spacing.
        // The slot must equal the card's real height, or the virtualised list drifts and
        // leaves blank space at the bottom of a long catalogue.
        let card_height = 44.0 + spacing + line + 4.0 + spacing + pill_height + 20.0;
        let slot = card_height + gap;
        let clip = ui.clip_rect();
        let list_top = ui.cursor().top();
        let total = visible.len();
        let first = (((clip.top() - list_top) / slot).floor().max(0.0) as usize).min(total);
        let last = ((((clip.bottom() - list_top) / slot).ceil() as usize) + 1).min(total);

        if first > 0 {
            ui.add_space(first as f32 * slot);
        }
        for candidate in &visible[first..last] {
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), card_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(ui.available_width(), card_height));
                    mobile_candidate(state, candidate, ui, actions, tokens);
                },
            );
            ui.add_space(gap);
        }
        if last < total {
            ui.add_space((total - last) as f32 * slot);
        }
    });
}

fn mobile_candidate(
    state: &AppState,
    candidate: &Candidate,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    let endpoint = candidate.endpoint();
    let display = if candidate.label.is_empty() {
        candidate.provider.clone()
    } else {
        candidate.label.clone()
    };
    let id = candidate.id.clone();
    let selected = state.settings.selected.contains(&id);
    let favourite = state.settings.favorites.contains(&id);
    let select_label = format!("Select {display} {endpoint}");
    let star_label = format!("Favourite {display} {endpoint}");
    let open_label = format!("Open {display} {endpoint}");
    let measured = state.find_row(&id);

    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(10.0)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if checkbox(ui, selected, &select_label, tokens, 44.0).clicked() {
                    actions.push(UiAction::ToggleSelected(id.clone()));
                }
                let text_width = (ui.available_width() - 52.0).max(80.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(text_width, 44.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        if mobile_row_button(ui, &display, &open_label, tokens).clicked() {
                            actions.push(UiAction::ChooseRow(id.clone()));
                        }
                    },
                );
                if star(ui, favourite, &star_label, tokens, 44.0).clicked() {
                    actions.push(UiAction::ToggleFavorite(id.clone()));
                }
            });

            ui.add(
                egui::Label::new(
                    RichText::new(&endpoint)
                        .monospace()
                        .size(12.0)
                        .color(tokens.muted),
                )
                .truncate(),
            );
            ui.add_space(4.0);
            // Keep the compact card to one status row: the category is available in the
            // detail sheet and in the filter chips above.
            ui.horizontal_wrapped(|ui| {
                components::pill(ui, kind_label(candidate.kind()), Tone::Accent);
                if let Some(row) = measured {
                    components::pill(
                        ui,
                        security_short(row.security_status()),
                        security_tone(row),
                    );
                } else {
                    components::pill(ui, "Not measured", Tone::Neutral);
                }
            });
        });
}

fn mobile_row_button(ui: &mut egui::Ui, text: &str, label: &str, tokens: Tokens) -> egui::Response {
    let response = ui.add_sized(
        [ui.available_width(), 44.0],
        egui::Button::new(RichText::new(text).size(15.0).strong().color(tokens.text))
            .frame(false)
            .wrap_mode(egui::TextWrapMode::Truncate)
            // A grow atom after the title pins it to the leading edge; a plain button
            // centers its text, which reads like a floating label between the controls.
            .right_text(" "),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn table_card(
    state: &AppState,
    visible: &[Candidate],
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Catalogue").size(16.0).strong());
            ui.label(
                RichText::new(format!("{} shown", visible.len()))
                    .size(12.0)
                    .color(tokens.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if components::button(
                    ui,
                    "Select none",
                    ButtonKind::Ghost,
                    false,
                    !state.settings.selected.is_empty(),
                )
                .clicked()
                {
                    actions.push(UiAction::SelectNone);
                }
                if components::button(
                    ui,
                    "Select all",
                    ButtonKind::Secondary,
                    false,
                    !visible.is_empty(),
                )
                .clicked()
                {
                    actions.push(UiAction::SelectAllVisible);
                }
            });
        });
        ui.add_space(6.0);

        TableBuilder::new(ui)
            .vscroll(false)
            .striped(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(28.0))
            .column(Column::exact(28.0))
            .column(Column::initial(110.0).at_least(70.0))
            .column(Column::initial(100.0).at_least(70.0))
            .column(Column::exact(54.0))
            .column(Column::exact(54.0))
            .column(Column::exact(80.0))
            .column(Column::remainder().at_least(60.0))
            .header(24.0, |mut header| {
                header.col(|_| {});
                header.col(|_| {});
                for label in [
                    "Provider",
                    "Endpoint",
                    "Cached ms",
                    "Uncached ms",
                    "DNSSEC",
                    "Health",
                ] {
                    header.col(|ui| table_header(ui, label, tokens));
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, visible.len(), |mut row| {
                    let candidate = &visible[row.index()];
                    let endpoint = candidate.endpoint();
                    let display = if candidate.label.is_empty() {
                        candidate.provider.clone()
                    } else {
                        candidate.label.clone()
                    };
                    let id = candidate.id.clone();
                    let selected = state.settings.selected.contains(&id);
                    let favourite = state.settings.favorites.contains(&id);
                    let select_label = format!("Select {display} {endpoint}");
                    let star_label = format!("Favourite {display} {endpoint}");
                    let open_label = format!("Open {display} {endpoint}");
                    let measured = state.find_row(&id);

                    row.col(|ui| {
                        if checkbox(ui, selected, &select_label, tokens, 20.0).clicked() {
                            actions.push(UiAction::ToggleSelected(id.clone()));
                        }
                    });
                    row.col(|ui| {
                        if star(ui, favourite, &star_label, tokens, 24.0).clicked() {
                            actions.push(UiAction::ToggleFavorite(id.clone()));
                        }
                    });
                    row.col(|ui| {
                        if row_button(ui, &display, &open_label, tokens).clicked() {
                            actions.push(UiAction::ChooseRow(id.clone()));
                        }
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&endpoint)
                                    .monospace()
                                    .size(12.0)
                                    .color(tokens.muted),
                            )
                            .truncate(),
                        );
                    });
                    row.col(|ui| {
                        number_cell(ui, measured.and_then(|row| row.cached), tokens);
                    });
                    row.col(|ui| {
                        number_cell(ui, measured.and_then(|row| row.uncached), tokens);
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(cell_text(
                                measured.map(|row| security_short(row.security_status())),
                            ))
                            .truncate(),
                        );
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(cell_text(measured.map(|row| row.note.label())))
                                .truncate(),
                        );
                    });
                });
            });
    });
}

fn detail_card(state: &AppState, ui: &mut egui::Ui, actions: &mut Vec<UiAction>, tokens: Tokens) {
    components::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Resolver details").size(16.0).strong());
        ui.add_space(6.0);

        let Some(candidate) = state
            .selected_row
            .as_deref()
            .and_then(|id| state.find_candidate(id))
        else {
            ui.label(
                RichText::new(
                    "Select a resolver from the table to see its endpoint, category and website.",
                )
                .size(13.0)
                .color(tokens.muted),
            );
            return;
        };

        ui.label(RichText::new(&candidate.label).size(20.0).strong());
        ui.label(
            RichText::new(format!("By {}", candidate.provider))
                .size(12.0)
                .color(tokens.muted),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(candidate.endpoint())
                .monospace()
                .size(12.0)
                .color(tokens.text),
        );
        ui.label(
            RichText::new(format!(
                "{} · {}",
                candidate.category.label(),
                kind_label(candidate.kind())
            ))
            .size(12.0)
            .color(tokens.muted),
        );

        if !candidate.note.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(&candidate.note).size(12.5));
        }

        if let Some(row) = state.find_row(&candidate.id) {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            row_metrics(ui, row, tokens);
        }

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if components::button(ui, "Choose", ButtonKind::Primary, false, true).clicked() {
                actions.push(UiAction::ChooseRow(candidate.id.clone()));
            }
            if components::button(ui, "Copy endpoint", ButtonKind::Secondary, false, true).clicked()
            {
                actions.push(UiAction::CopyEndpoint(candidate.id.clone()));
            }
            if components::button(
                ui,
                "Open website",
                ButtonKind::Ghost,
                false,
                !candidate.website.is_empty(),
            )
            .clicked()
            {
                actions.push(UiAction::OpenWebsite(candidate.id.clone()));
            }
        });
        if !candidate.website.is_empty() {
            ui.label(
                RichText::new(&candidate.website)
                    .size(11.5)
                    .color(tokens.muted),
            );
        }
    });
}

fn row_metrics(ui: &mut egui::Ui, row: &Row, tokens: Tokens) {
    ui.horizontal_wrapped(|ui| {
        components::metric(
            ui,
            "Cached",
            &format!("{:.1} ms", row.cached.unwrap_or(row.score)),
            None,
        );
        components::metric(
            ui,
            "Uncached",
            &row.uncached
                .map(|value| format!("{value:.1} ms"))
                .unwrap_or_else(|| "Not measured".to_string()),
            None,
        );
        components::metric(
            ui,
            "Jitter",
            &format!("{:.1} ms", row.cached_jitter.unwrap_or(0.0)),
            None,
        );
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        components::pill(
            ui,
            security_short(row.security_status()),
            security_tone(row),
        );
        components::pill(ui, row.note.label(), note_tone(row));
    });
    ui.label(
        RichText::new(format!(
            "{} timeout(s) · {}",
            row.timeouts,
            row.transport.label()
        ))
        .size(11.5)
        .color(tokens.muted),
    );
}

fn table_header(ui: &mut egui::Ui, label: &str, tokens: Tokens) {
    ui.label(RichText::new(label).size(11.0).strong().color(tokens.muted));
}

fn cell_text(value: Option<&str>) -> RichText {
    let text = value.unwrap_or("Unmeasured");
    RichText::new(text).size(11.5)
}

fn number_cell(ui: &mut egui::Ui, value: Option<f64>, tokens: Tokens) {
    let text = value.map_or_else(
        || RichText::new("Not tested").size(11.5).color(tokens.muted),
        |value| RichText::new(format!("{value:.1}")).monospace().size(11.5),
    );
    ui.add(egui::Label::new(text).truncate());
}

fn checkbox(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    tokens: Tokens,
    size: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            5.0,
            if selected {
                tokens.accent
            } else {
                Color32::TRANSPARENT
            },
        );
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(
                1.0,
                if selected {
                    tokens.accent
                } else {
                    ui.visuals().widgets.inactive.bg_stroke.color
                },
            ),
            StrokeKind::Inside,
        );
        if selected {
            icons::paint_at(ui.painter(), Icon::Check, rect.shrink(3.0), ON_ACCENT);
        }
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, selected, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn star(
    ui: &mut egui::Ui,
    favourite: bool,
    label: &str,
    tokens: Tokens,
    size: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        icons::paint_at(
            ui.painter(),
            if favourite {
                Icon::StarFilled
            } else {
                Icon::Star
            },
            rect.shrink(2.0),
            if favourite {
                tokens.warning
            } else {
                tokens.muted
            },
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, favourite, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn row_button(ui: &mut egui::Ui, text: &str, label: &str, tokens: Tokens) -> egui::Response {
    let response = ui.add(
        egui::Label::new(RichText::new(text).size(13.0).strong().color(tokens.text))
            .sense(Sense::click())
            .truncate(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Plain => "Plain DNS",
        Kind::Dot => "DNS-over-TLS",
        Kind::Doh => "DNS-over-HTTPS",
    }
}

fn security_short(status: SecurityStatus) -> &'static str {
    match status {
        SecurityStatus::Validated => "Validated",
        SecurityStatus::Insecure => "No DNSSEC",
        SecurityStatus::Partial => "Partial",
        SecurityStatus::BogusAccepted => "Bogus accepted",
        SecurityStatus::Unknown => "Unknown",
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

fn note_tone(row: &Row) -> Tone {
    match row.note {
        crate::bench::Note::Ok => Tone::Success,
        crate::bench::Note::Hijacks => Tone::Danger,
        crate::bench::Note::Unreliable => Tone::Warning,
    }
}
