//! Desktop shell and page tests. These run headlessly with egui_kittest: every assertion is a
//! real widget tree built by the same renderer the app uses.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::config::Theme;
use crate::gui::actions::{Page, ReportFormat, UiAction};
use crate::gui::desktop::render_desktop;
use crate::gui::state::{AppState, ViewContext};
use crate::profile::Profile;
use crate::servers::Category;

struct DesktopState {
    state: AppState,
    cx: ViewContext,
    actions: Vec<UiAction>,
}

fn harness() -> Harness<'static, DesktopState> {
    Harness::new_ui_state(
        |ui, state: &mut DesktopState| {
            render_desktop(&mut state.state, &state.cx, ui, &mut state.actions);
        },
        DesktopState {
            state: AppState::demo(),
            cx: ViewContext::desktop_for_test(),
            actions: Vec::new(),
        },
    )
}

fn servers_harness(configure: impl FnOnce(&mut AppState)) -> Harness<'static, DesktopState> {
    page_harness(Page::Servers, configure)
}

fn history_harness(configure: impl FnOnce(&mut AppState)) -> Harness<'static, DesktopState> {
    page_harness(Page::History, configure)
}

fn settings_harness(configure: impl FnOnce(&mut AppState)) -> Harness<'static, DesktopState> {
    page_harness(Page::Settings, configure)
}

fn page_harness(
    page: Page,
    configure: impl FnOnce(&mut AppState),
) -> Harness<'static, DesktopState> {
    let mut state = AppState::demo();
    state.page = page;
    configure(&mut state);
    Harness::new_ui_state(
        |ui, state: &mut DesktopState| {
            render_desktop(&mut state.state, &state.cx, ui, &mut state.actions);
        },
        DesktopState {
            state,
            cx: ViewContext::desktop_for_test(),
            actions: Vec::new(),
        },
    )
}

#[test]
fn desktop_overview_shows_run_actions() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Run benchmark");
    harness.get_by_label("Test selected");
    harness.get_by_label("Test all");
    harness.get_by_label("Overview");
    harness.get_by_label("Servers");
    harness.get_by_label("History");
    harness.get_by_label("Settings");
    harness.get_by_label("Copy current DNS");
}

#[test]
fn desktop_nav_emits_navigate() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Servers").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::Navigate(Page::Servers))
    );
}

#[test]
fn overview_profile_emits_set_profile() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Thorough").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SetProfile(Profile::Thorough))
    );
}

#[test]
fn overview_current_dns_copy_emits_action() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Copy current DNS").click();
    harness.run();
    assert!(harness.state().actions.contains(&UiAction::CopyCurrentDns));
}

#[test]
fn overview_recommendation_shows_use_action() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Use this resolver").click();
    harness.run();
    assert!(matches!(
        harness.state().actions.as_slice(),
        [UiAction::Apply(_)]
    ));
}

#[test]
fn servers_search_clear_emits_action() {
    let mut harness = servers_harness(|state| state.search = "cloudflare".into());
    harness.run();
    harness.get_by_label("Clear search").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SetSearch(String::new()))
    );
}

#[test]
fn servers_filter_chip_emits_action() {
    let mut harness = servers_harness(|_| {});
    harness.run();
    harness.get_by_label("Security").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SetCategoryFilter(Some(Category::Security)))
    );
}

#[test]
fn servers_select_all_emits_action() {
    let mut harness = servers_harness(|_| {});
    harness.run();
    harness.get_by_label("Select all").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SelectAllVisible)
    );
}

#[test]
fn server_row_toggle_emits_toggle_selected() {
    let mut harness = servers_harness(|state| state.search = "cloudflare".into());
    harness.run();
    harness.get_by_label("Select Cloudflare 1.1.1.1").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| matches!(
        action,
        UiAction::ToggleSelected(id) if id == "plain:1.1.1.1:53"
    )));
}

#[test]
fn server_star_emits_toggle_favorite() {
    let mut harness = servers_harness(|state| state.search = "cloudflare".into());
    harness.run();
    harness.get_by_label("Favourite Cloudflare 1.1.1.1").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| matches!(
        action,
        UiAction::ToggleFavorite(id) if id == "plain:1.1.1.1:53"
    )));
}

#[test]
fn server_row_click_opens_details() {
    let mut harness = servers_harness(|state| state.search = "cloudflare".into());
    harness.run();
    harness.get_by_label("Open Cloudflare 1.1.1.1").click();
    harness.run();
    assert!(harness.state().actions.iter().any(|action| matches!(
        action,
        UiAction::ChooseRow(id) if id == "plain:1.1.1.1:53"
    )));
}

#[test]
fn servers_test_selected_is_disabled_when_empty() {
    let mut harness = servers_harness(|state| state.settings.selected.clear());
    harness.run();
    assert!(
        harness
            .get_by_label("Test selected")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn history_reopen_emits_action() {
    let mut harness = history_harness(|state| state.history_selected = Some(0));
    harness.run();
    harness.get_by_label("Reopen run").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::ReopenHistory(0))
    );
}

#[test]
fn history_export_emits_action() {
    let mut harness = history_harness(|state| state.history_selected = Some(0));
    harness.run();
    harness.get_by_label("Export JSON").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::ExportHistory(0, ReportFormat::Json))
    );
}

#[test]
fn history_delete_emits_action() {
    let mut harness = history_harness(|state| state.history_selected = Some(0));
    harness.run();
    harness.get_by_label("Delete run").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::DeleteHistory(0))
    );
}

#[test]
fn history_clear_emits_action() {
    let mut harness = history_harness(|_| {});
    harness.run();
    harness.get_by_label("Clear history").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::RequestClearHistory)
    );
}

#[test]
fn settings_theme_emits_action() {
    let mut harness = settings_harness(|_| {});
    harness.run();
    harness.get_by_label("Light").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SetTheme(Theme::Light))
    );
}

#[test]
fn settings_profile_emits_action() {
    let mut harness = settings_harness(|_| {});
    harness.run();
    harness.get_by_label("Thorough").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::SetProfile(Profile::Thorough))
    );
}

#[test]
fn settings_save_emits_action() {
    let mut harness = settings_harness(|_| {});
    harness.run();
    harness.get_by_label("Save settings").click();
    harness.run();
    assert!(harness.state().actions.contains(&UiAction::SaveSettings));
}

#[test]
fn settings_reset_emits_action() {
    let mut harness = settings_harness(|_| {});
    harness.run();
    harness.get_by_label("Reset settings").click();
    harness.run();
    assert!(harness.state().actions.contains(&UiAction::ResetSettings));
}

#[test]
fn about_shows_version_and_closes() {
    let mut harness = settings_harness(|state| state.about_open = true);
    harness.run();
    harness.get_by_label("Version 0.8.0");
    harness.get_by_label("Close about").click();
    harness.run();
    assert!(harness.state().actions.contains(&UiAction::CloseAbout));
}
