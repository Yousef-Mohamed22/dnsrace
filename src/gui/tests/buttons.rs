//! Task 17: every-button coverage.
//!
//! These harnesses render the real desktop and mobile shells, including the modal layer, and
//! click the labelled controls a person can reach. Page-level tests assert the exact `UiAction`;
//! dispatch tests elsewhere assert the backend effect.

use std::collections::BTreeMap;

use eframe::egui::{self, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::android::SafeArea;
use crate::android_guide;
use crate::bench::Row;
use crate::config::{CustomResolver, Theme};
use crate::gui::actions::{AboutLink, Page, ReportFormat, RunScope, UiAction};
use crate::gui::desktop::render_desktop;
use crate::gui::mobile::render_mobile;
use crate::gui::modals;
use crate::gui::state::{AppState, PendingApply, ViewContext};
use crate::platform::AppliedDns;
use crate::servers::{Family, Kind, Protocol};

const DESKTOP: Vec2 = Vec2::new(1280.0, 840.0);
const COMPACT_DESKTOP: Vec2 = Vec2::new(800.0, 700.0);
const PHONE: Vec2 = Vec2::new(390.0, 844.0);
const SMALL_PHONE: Vec2 = Vec2::new(360.0, 800.0);
const LANDSCAPE: Vec2 = Vec2::new(844.0, 390.0);

struct DesktopState {
    state: AppState,
    cx: ViewContext,
    actions: Vec<UiAction>,
}

struct MobileState {
    state: AppState,
    cx: ViewContext,
    actions: Vec<UiAction>,
}

fn scale_text(ctx: &egui::Context, scale: f32) {
    if (scale - 1.0).abs() > f32::EPSILON {
        ctx.set_zoom_factor(scale);
    }
}

fn desktop_harness_with(
    size: Vec2,
    scale: f32,
    configure: impl FnOnce(&mut AppState),
) -> Harness<'static, DesktopState> {
    let mut state = AppState::demo();
    configure(&mut state);
    Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| {
            crate::gui::theme::install(ctx, Theme::Dark);
            scale_text(ctx, scale);
        })
        .build_ui_state(
            |ui, state: &mut DesktopState| {
                render_desktop(&mut state.state, &state.cx, ui, &mut state.actions);
                modals::render(&mut state.state, &state.cx, ui.ctx(), &mut state.actions);
            },
            DesktopState {
                state,
                cx: ViewContext::desktop_for_test(),
                actions: Vec::new(),
            },
        )
}

fn desktop_harness(configure: impl FnOnce(&mut AppState)) -> Harness<'static, DesktopState> {
    desktop_harness_with(DESKTOP, 1.0, configure)
}

fn mobile_harness_with(
    size: Vec2,
    api_level: u32,
    safe_area: SafeArea,
    scale: f32,
    configure: impl FnOnce(&mut AppState),
) -> Harness<'static, MobileState> {
    let mut state = AppState::demo();
    configure(&mut state);
    Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| {
            crate::gui::theme::install(ctx, Theme::Dark);
            scale_text(ctx, scale);
        })
        .build_ui_state(
            |ui, state: &mut MobileState| {
                render_mobile(&mut state.state, &state.cx, ui, &mut state.actions);
                modals::render(&mut state.state, &state.cx, ui.ctx(), &mut state.actions);
            },
            MobileState {
                state,
                cx: ViewContext::android_for_test_with_insets(api_level, safe_area),
                actions: Vec::new(),
            },
        )
}

fn mobile_harness(configure: impl FnOnce(&mut AppState)) -> Harness<'static, MobileState> {
    mobile_harness_with(PHONE, 33, SafeArea::ZERO, 1.0, configure)
}

fn click(harness: &mut Harness<'static, impl Sized>, label: &str) {
    harness.get_by_label(label).click_accesskit();
    harness.run();
}

fn applied_dns() -> AppliedDns {
    AppliedDns {
        platform: "test".into(),
        method: "test".into(),
        interface: "wlan0".into(),
        applied: vec!["1.1.1.1".parse().unwrap()],
        previous: BTreeMap::new(),
        timestamp: 0,
    }
}

fn pending_apply() -> PendingApply {
    PendingApply {
        label: "Cloudflare (1.1.1.1)".into(),
        addresses: vec!["1.1.1.1".parse().unwrap()],
        warning: None,
    }
}

fn custom_resolver() -> CustomResolver {
    CustomResolver {
        name: "Example".into(),
        protocol: Protocol::Doh,
        endpoint: "https://dns.example.com/dns-query".into(),
        address: None,
    }
}

fn open_guide_with(state: &mut AppState, api_level: u32, mutate: impl FnOnce(&mut Row)) {
    let mut row = state
        .find_row("demo:Cloudflare")
        .expect("the demo winner exists")
        .clone();
    mutate(&mut row);
    let catalogue = state.catalog.clone();
    state.selected_row = Some(row.id.clone());
    state.guide = Some(android_guide::build(api_level, &row, &catalogue));
}

#[test]
fn button_desktop_run_actions_emit_scopes() {
    let mut harness = desktop_harness(|state| {
        let selected = state.catalog[0].id.clone();
        state.settings.selected.insert(selected);
        let favourite = state.catalog[1].id.clone();
        state.settings.favorites.insert(favourite);
    });
    harness.run();

    click(&mut harness, "Run benchmark");
    click(&mut harness, "Test selected");
    click(&mut harness, "Test all");
    click(&mut harness, "Test current");
    click(&mut harness, "Test favourites");
    click(&mut harness, "Refresh detection");

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::Run(RunScope::Selected)));
    assert!(actions.contains(&UiAction::Run(RunScope::All)));
    assert!(actions.contains(&UiAction::Run(RunScope::CurrentDns)));
    assert!(actions.contains(&UiAction::Run(RunScope::Favorites)));
    assert!(actions.contains(&UiAction::RefreshDetection));
}

#[test]
fn button_desktop_disables_run_and_profile_controls_while_running() {
    let mut harness = desktop_harness(|state| {
        state.running = true;
        state.stage = Some(crate::engine::Stage::Scan);
        state.progress = (2, 10);
    });
    harness.run();

    for label in [
        "Run benchmark",
        "Test selected",
        "Test all",
        "Test current",
        "Test favourites",
        "Thorough",
        "Quick",
    ] {
        assert!(
            harness.get_by_label(label).accesskit_node().is_disabled(),
            "{label} must be disabled while a run is active"
        );
    }
    assert!(
        !harness
            .get_by_label("Cancel")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn button_desktop_test_favourites_explains_when_empty() {
    let mut harness = desktop_harness(|_| {});
    harness.run();

    assert!(
        harness
            .get_by_label("Test favourites")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Star at least one resolver in Servers to enable Test favourites.");
}

#[test]
fn button_desktop_revert_emits_action() {
    let mut harness = desktop_harness(|state| state.applied = Some(applied_dns()));
    harness.run();

    click(&mut harness, "Restore previous DNS");
    assert!(harness.state().actions.contains(&UiAction::Revert));
}

#[test]
fn button_desktop_servers_matrix_emits_actions() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::Servers;
        state.search = "cloudflare".into();
        state.selected_row = Some("plain:1.1.1.1:53".into());
        state.settings.selected.insert("plain:1.1.1.1:53".into());
    });
    harness.run();

    for label in [
        "Clear search",
        "Security",
        "DoT",
        "IPv6",
        "Favourites",
        "Select all",
        "Select none",
        "Select Cloudflare 1.1.1.1",
        "Favourite Cloudflare 1.1.1.1",
        "Open Cloudflare 1.1.1.1",
        "Choose",
        "Copy endpoint",
        "Open website",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::SetSearch(String::new())));
    assert!(actions.contains(&UiAction::SetCategoryFilter(Some(
        crate::servers::Category::Security
    ))));
    assert!(actions.contains(&UiAction::SetKindFilter(Some(Kind::Dot))));
    assert!(actions.contains(&UiAction::SetFamilyFilter(Some(Family::V6))));
    assert!(actions.contains(&UiAction::SetFavoritesOnly(true)));
    assert!(actions.contains(&UiAction::SelectAllVisible));
    assert!(actions.contains(&UiAction::SelectNone));
    assert!(actions.contains(&UiAction::ToggleSelected("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::ToggleFavorite("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::ChooseRow("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::CopyEndpoint("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::OpenWebsite("plain:1.1.1.1:53".into())));
}

#[test]
fn button_desktop_settings_matrix_emits_actions() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::Settings;
        state.settings.custom.push(custom_resolver());
    });
    harness.run();

    for theme in Theme::ALL {
        click(&mut harness, theme.label());
    }
    for protocol in Protocol::all() {
        click(&mut harness, protocol.label());
    }
    for family in [Family::Any, Family::V4, Family::V6] {
        click(&mut harness, family.label());
    }
    for label in [
        "Save settings",
        "Reset settings",
        "Clear history",
        "About dnsrace",
        "Add custom resolver",
        "Edit Example",
        "Remove Example",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    for theme in Theme::ALL {
        assert!(actions.contains(&UiAction::SetTheme(theme)));
    }
    for protocol in Protocol::all() {
        assert!(actions.contains(&UiAction::SetProtocol(protocol)));
    }
    for family in [Family::Any, Family::V4, Family::V6] {
        assert!(actions.contains(&UiAction::SetFamily(family)));
    }
    assert!(actions.contains(&UiAction::SaveSettings));
    assert!(actions.contains(&UiAction::ResetSettings));
    assert!(actions.contains(&UiAction::RequestClearHistory));
    assert!(actions.contains(&UiAction::OpenAbout));
    assert!(actions.contains(&UiAction::OpenCustom));
    assert!(actions.contains(&UiAction::EditCustom(0)));
    assert!(actions.contains(&UiAction::RemoveCustom(0)));
}

#[test]
fn button_desktop_history_matrix_emits_actions() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::History;
        state.history_selected = Some(0);
    });
    harness.run();

    for label in [
        "Reopen run",
        "Export JSON",
        "Export CSV",
        "Export HTML",
        "Delete run",
        "Clear history",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::ReopenHistory(0)));
    for format in [ReportFormat::Json, ReportFormat::Csv, ReportFormat::Html] {
        assert!(actions.contains(&UiAction::ExportHistory(0, format)));
    }
    assert!(actions.contains(&UiAction::DeleteHistory(0)));
    assert!(actions.contains(&UiAction::RequestClearHistory));
}

#[test]
fn button_desktop_clear_history_confirm_emits_actions() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::History;
        state.confirm_clear_history = true;
    });
    harness.run();
    click(&mut harness, "Delete all runs");
    assert!(harness.state().actions.contains(&UiAction::ClearHistory));

    let mut harness = desktop_harness(|state| {
        state.page = Page::History;
        state.confirm_clear_history = true;
    });
    harness.run();
    click(&mut harness, "Keep history");
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::CancelClearHistory)
    );
}

#[test]
fn button_desktop_about_matrix_emits_actions() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::Settings;
        state.about_open = true;
    });
    harness.run();

    for label in [
        "Project website",
        "Report an issue",
        "MIT licence",
        "Close about",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Project)));
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Issues)));
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Licence)));
    assert!(actions.contains(&UiAction::CloseAbout));
}

#[test]
fn button_desktop_modal_confirm_and_custom_actions() {
    let mut harness = desktop_harness(|state| {
        state.confirm_apply = true;
        state.pending_apply = Some(pending_apply());
    });
    harness.run();
    click(&mut harness, "Apply DNS");
    assert!(harness.state().actions.contains(&UiAction::ConfirmApply));

    let mut harness = desktop_harness(|state| {
        state.confirm_apply = true;
        state.pending_apply = Some(pending_apply());
    });
    harness.run();
    click(&mut harness, "Cancel apply");
    assert!(harness.state().actions.contains(&UiAction::CancelApply));

    let mut harness = desktop_harness(|state| {
        state.confirm_revert = true;
        state.applied = Some(applied_dns());
    });
    harness.run();
    click(&mut harness, "Restore DNS");
    assert!(harness.state().actions.contains(&UiAction::ConfirmRevert));

    let mut harness = desktop_harness(|state| {
        state.confirm_revert = true;
        state.applied = Some(applied_dns());
    });
    harness.run();
    click(&mut harness, "Cancel revert");
    assert!(harness.state().actions.contains(&UiAction::CancelRevert));

    let mut harness = desktop_harness(|state| state.custom_open = true);
    harness.run();
    click(&mut harness, "Add resolver");
    click(&mut harness, "Cancel custom resolver");
    assert!(harness.state().actions.contains(&UiAction::SaveCustom));
    assert!(harness.state().actions.contains(&UiAction::CancelCustom));
}

#[test]
fn button_desktop_custom_editor_shows_edit_title() {
    let mut harness = desktop_harness(|state| {
        state.page = Page::Settings;
        state.settings.custom.push(custom_resolver());
        state.custom_open = true;
        state.custom_edit = Some(0);
        state.custom_draft.name = "Example".into();
        state.custom_draft.protocol = Protocol::Doh;
        state.custom_draft.endpoint = "https://dns.example.com/dns-query".into();
    });
    harness.run();

    harness.get_by_label("Edit custom resolver");
    harness.get_by_label("Save resolver");
}

#[test]
fn button_desktop_escape_closes_topmost_modal() {
    let mut harness = desktop_harness(|state| {
        state.custom_open = true;
        open_guide_with(state, 33, |_| {});
        state.confirm_apply = true;
        state.pending_apply = Some(pending_apply());
        state.confirm_revert = true;
        state.applied = Some(applied_dns());
        state.details_open = true;
    });
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::CancelCustom));
    assert!(!actions.contains(&UiAction::CloseGuide));
    assert!(!actions.contains(&UiAction::CancelApply));
    assert!(!actions.contains(&UiAction::CancelRevert));
    assert!(!actions.contains(&UiAction::CloseDetails));
}

#[test]
fn button_mobile_run_actions_emit_scopes() {
    let mut harness = mobile_harness(|state| {
        let selected = state.catalog[0].id.clone();
        state.settings.selected.insert(selected);
        let favourite = state.catalog[1].id.clone();
        state.settings.favorites.insert(favourite);
    });
    harness.run();

    for label in [
        "Run benchmark",
        "Test selected",
        "Test all",
        "Test current",
        "Test favourites",
        "Refresh detection",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::Run(RunScope::Selected)));
    assert!(actions.contains(&UiAction::Run(RunScope::All)));
    assert!(actions.contains(&UiAction::Run(RunScope::CurrentDns)));
    assert!(actions.contains(&UiAction::Run(RunScope::Favorites)));
    assert!(actions.contains(&UiAction::RefreshDetection));
}

#[test]
fn button_mobile_android_recommendation_opens_setup_guide() {
    let mut harness = mobile_harness(|_| {});
    harness.run();

    click(&mut harness, "Set up this resolver");
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::OpenGuide("demo:Cloudflare".into()))
    );
}

#[test]
fn button_mobile_guide_metadata_hostname_offers_dot_benchmark() {
    let mut harness = mobile_harness(|state| open_guide_with(state, 33, |_| {}));
    harness.run();

    click(&mut harness, "Benchmark DoT servers instead");

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::CloseGuide));
    assert!(actions.contains(&UiAction::SetProtocol(Protocol::Dot)));
    assert!(actions.contains(&UiAction::Run(RunScope::All)));
}

#[test]
fn button_mobile_guide_missing_hostname_offers_dot_benchmark() {
    let mut harness = mobile_harness(|state| {
        open_guide_with(state, 33, |row| {
            row.id = "demo:Mine".into();
            row.provider = "Mine".into();
            row.label = "Mine".into();
            row.dot_host = None;
            row.addr = None;
            row.transport = Protocol::Udp;
        });
    });
    harness.run();

    click(&mut harness, "Benchmark DoT servers instead");
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::Run(RunScope::All))
    );
}

#[test]
fn button_mobile_guide_missing_plain_offers_plain_benchmark() {
    let mut harness = mobile_harness(|state| {
        open_guide_with(state, 26, |row| {
            row.id = "demo:Mine".into();
            row.provider = "Mine".into();
            row.label = "Mine".into();
            row.dot_host = None;
            row.addr = None;
            row.transport = Protocol::Dot;
        });
    });
    harness.run();

    click(&mut harness, "Benchmark plain DNS instead");
    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::CloseGuide));
    assert!(actions.contains(&UiAction::SetProtocol(Protocol::Udp)));
    assert!(actions.contains(&UiAction::Run(RunScope::All)));
}

#[test]
fn button_mobile_guide_copy_open_and_close_emit_actions() {
    let mut harness = mobile_harness(|state| open_guide_with(state, 33, |_| {}));
    harness.run();

    for label in ["Copy setup value", "Open Private DNS", "Close guide"] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::CopyGuideText));
    assert!(actions.contains(&UiAction::OpenPrivateDns));
    assert!(actions.contains(&UiAction::CloseGuide));
}

#[test]
fn button_mobile_guide_api26_open_wifi_emits_action() {
    let mut harness = mobile_harness(|state| open_guide_with(state, 26, |_| {}));
    harness.run();

    click(&mut harness, "Open Wi-Fi");
    assert!(harness.state().actions.contains(&UiAction::OpenWifi));
}

#[test]
fn button_mobile_share_report_emits_action() {
    let mut harness = mobile_harness(|_| {});
    harness.run();

    click(&mut harness, "Share report");
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::ShareReport(ReportFormat::Json))
    );
}

#[test]
fn button_mobile_history_share_emits_action() {
    let mut harness = mobile_harness(|state| {
        state.page = Page::History;
        state.history_selected = Some(0);
    });
    harness.run();

    click(&mut harness, "Share report");
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::ShareHistory(0, ReportFormat::Json))
    );
}

#[test]
fn button_mobile_history_export_is_replaced_by_share() {
    let mut harness = mobile_harness(|state| {
        state.page = Page::History;
        state.history_selected = Some(0);
    });
    harness.run();

    assert!(
        harness.query_by_label("Export JSON").is_none(),
        "Android cannot write report files"
    );
    harness.get_by_label("Share report");
}

#[test]
fn button_mobile_server_detail_sheet_actions() {
    let mut harness = mobile_harness(|state| {
        state.page = Page::Servers;
        state.search = "cloudflare".into();
        state.selected_row = Some("plain:1.1.1.1:53".into());
        state.details_open = true;
    });
    harness.run();

    for label in [
        "Add to favourites",
        "Open website",
        "Set up this resolver",
        "Copy endpoint",
        "Close details",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::ToggleFavorite("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::OpenWebsite("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::OpenGuide("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::CopyEndpoint("plain:1.1.1.1:53".into())));
    assert!(actions.contains(&UiAction::CloseDetails));
}

#[test]
fn button_mobile_settings_matrix_emits_actions() {
    let mut harness = mobile_harness(|state| {
        state.page = Page::Settings;
        state.settings.custom.push(custom_resolver());
    });
    harness.run();

    for label in [
        "Save settings",
        "Reset settings",
        "About dnsrace",
        "Add custom resolver",
        "Edit Example",
        "Remove Example",
        "UDP",
        "IPv4",
        "Light",
    ] {
        click(&mut harness, label);
    }

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::SaveSettings));
    assert!(actions.contains(&UiAction::ResetSettings));
    assert!(actions.contains(&UiAction::OpenAbout));
    assert!(actions.contains(&UiAction::OpenCustom));
    assert!(actions.contains(&UiAction::EditCustom(0)));
    assert!(actions.contains(&UiAction::RemoveCustom(0)));
    assert!(actions.contains(&UiAction::SetProtocol(Protocol::Udp)));
    assert!(actions.contains(&UiAction::SetFamily(Family::V4)));
    assert!(actions.contains(&UiAction::SetTheme(Theme::Light)));
}

#[test]
fn button_mobile_about_links_emit_actions() {
    let mut harness = mobile_harness(|state| {
        state.page = Page::Settings;
        state.about_open = true;
    });
    harness.run();
    for label in [
        "Project website",
        "Report an issue",
        "MIT licence",
        "Close about",
    ] {
        click(&mut harness, label);
    }
    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Project)));
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Issues)));
    assert!(actions.contains(&UiAction::OpenAboutLink(AboutLink::Licence)));
    assert!(actions.contains(&UiAction::CloseAbout));
}

#[test]
fn button_mobile_custom_modal_is_touch_sized_and_fits() {
    let mut harness = mobile_harness_with(SMALL_PHONE, 33, SafeArea::ZERO, 1.0, |state| {
        state.custom_open = true;
    });
    harness.run();

    for label in [
        "Cancel custom resolver",
        "Add resolver",
        "UDP",
        "TCP",
        "DoT",
        "DoH",
    ] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.height() >= 44.0,
            "{label} is below the touch minimum: {rect:?}"
        );
        assert!(
            rect.left() >= -1.0 && rect.right() <= SMALL_PHONE.x + 1.0,
            "{label} leaves the phone width: {rect:?}"
        );
    }
    assert!(
        harness.get_by_label("Add resolver").rect().height() >= 48.0,
        "the primary modal action is not 48 dp"
    );
}

#[test]
fn button_mobile_guide_modal_is_touch_sized_and_fits_landscape() {
    let mut harness = mobile_harness_with(LANDSCAPE, 33, SafeArea::ZERO, 1.0, |state| {
        open_guide_with(state, 33, |_| {});
    });
    harness.run();

    for label in ["Copy setup value", "Open Private DNS", "Close guide"] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.height() >= 44.0,
            "{label} is below the touch minimum: {rect:?}"
        );
        assert!(
            rect.left() >= -1.0 && rect.right() <= LANDSCAPE.x + 1.0,
            "{label} leaves the landscape width: {rect:?}"
        );
    }
    assert!(
        harness.get_by_label("Copy setup value").rect().height() >= 48.0,
        "the primary guide action is not 48 dp"
    );
}

#[test]
fn button_mobile_escape_closes_topmost_modal() {
    let mut harness = mobile_harness(|state| {
        state.custom_open = true;
        open_guide_with(state, 33, |_| {});
        state.confirm_apply = true;
        state.pending_apply = Some(pending_apply());
        state.confirm_revert = true;
        state.applied = Some(applied_dns());
        state.details_open = true;
        state.selected_row = Some("plain:1.1.1.1:53".into());
    });
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    let actions = &harness.state().actions;
    assert!(actions.contains(&UiAction::CancelCustom));
    assert!(!actions.contains(&UiAction::CloseGuide));
    assert!(!actions.contains(&UiAction::CancelApply));
}

#[test]
fn button_mobile_every_page_is_accessible_and_touch_sized() {
    for size in [PHONE, SMALL_PHONE, LANDSCAPE] {
        for page in Page::ALL {
            let mut harness = mobile_harness_with(size, 33, SafeArea::ZERO, 1.0, move |state| {
                state.page = page;
            });
            harness.run();
            harness.check_accessibility();

            for node in harness.query_all(egui_kittest::kittest::By::new()) {
                let access = node.accesskit_node();
                if access.bounding_box().is_none() || access.is_hidden() {
                    continue;
                }
                if !is_interactive(access.role()) {
                    continue;
                }
                let rect = node.rect();
                assert!(
                    rect.width() >= 44.0 - 0.5 && rect.height() >= 44.0 - 0.5,
                    "{:?} on {page:?} at {size:?} is below 44x44: {rect:?} label={:?}",
                    access.role(),
                    access.label()
                );
            }
        }
    }
}

#[test]
fn button_mobile_large_text_and_safe_area_keep_primary_action_visible() {
    let insets = SafeArea {
        top: 24,
        right: 0,
        bottom: 48,
        left: 0,
    };
    let mut harness = mobile_harness_with(PHONE, 33, insets, 1.5, |_| {});
    harness.run();

    for label in [
        "Run benchmark",
        "Overview",
        "Servers",
        "History",
        "Settings",
    ] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.left() >= -1.0
                && rect.right() <= PHONE.x + 1.0
                && rect.top() >= -1.0
                && rect.bottom() <= PHONE.y + 1.0,
            "{label} is clipped at 1.5x text: {rect:?}"
        );
    }
}

#[test]
fn button_desktop_large_text_keeps_primary_action_visible() {
    let mut harness = desktop_harness_with(DESKTOP, 1.5, |_| {});
    harness.run();

    for label in [
        "Run benchmark",
        "Overview",
        "Servers",
        "History",
        "Settings",
    ] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.left() >= -1.0
                && rect.right() <= DESKTOP.x + 1.0
                && rect.top() >= -1.0
                && rect.bottom() <= DESKTOP.y + 1.0,
            "{label} is clipped at 1.5x text: {rect:?}"
        );
    }
}

#[test]
fn button_desktop_and_mobile_pages_pass_accessibility() {
    for size in [DESKTOP, COMPACT_DESKTOP] {
        for page in Page::ALL {
            let mut harness = desktop_harness_with(size, 1.0, move |state| {
                state.page = page;
            });
            harness.run();
            harness.check_accessibility();

            for node in harness.query_all(egui_kittest::kittest::By::new()) {
                let access = node.accesskit_node();
                if access.bounding_box().is_none() || access.is_hidden() {
                    continue;
                }
                if !is_interactive(access.role()) {
                    continue;
                }
                let rect = node.rect();
                assert!(
                    rect.left() >= -1.0 && rect.right() <= size.x + 1.0,
                    "{:?} on {page:?} at {size:?} overflows horizontally: {rect:?}",
                    access.role()
                );
            }
        }
    }
}

fn is_interactive(role: egui::Role) -> bool {
    matches!(
        role,
        egui::Role::Button
            | egui::Role::CheckBox
            | egui::Role::RadioButton
            | egui::Role::TextInput
            | egui::Role::MultilineTextInput
            | egui::Role::SpinButton
            | egui::Role::Slider
            | egui::Role::Switch
            | egui::Role::ComboBox
    )
}
