//! Mobile shell tests. They use the same page state and actions as desktop, but with an Android
//! view context, touch-sized controls and a bottom navigation bar.
//!
//! Every assertion runs in egui_kittest at a phone or landscape viewport, so a control that is
//! clipped or too small fails the test the same way it would fail a person.

use eframe::egui::{Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};

use crate::android::SafeArea;
use crate::android_guide;
use crate::gui::actions::{Page, UiAction};
use crate::gui::mobile::render_mobile;
use crate::gui::modals;
use crate::gui::state::{AppState, ViewContext};

const PHONE: Vec2 = Vec2::new(390.0, 844.0);
const SMALL_PHONE: Vec2 = Vec2::new(360.0, 800.0);
const LANDSCAPE: Vec2 = Vec2::new(844.0, 390.0);

struct MobileState {
    state: AppState,
    cx: ViewContext,
    actions: Vec<UiAction>,
}

fn mobile_harness(
    size: Vec2,
    api_level: u32,
    configure: impl FnOnce(&mut AppState),
) -> Harness<'static, MobileState> {
    mobile_harness_with_insets(size, api_level, SafeArea::ZERO, configure)
}

fn mobile_harness_with_insets(
    size: Vec2,
    api_level: u32,
    safe_area: SafeArea,
    configure: impl FnOnce(&mut AppState),
) -> Harness<'static, MobileState> {
    let mut state = AppState::demo();
    configure(&mut state);
    Harness::builder()
        .with_size(size)
        .with_context_setup(|ctx| {
            crate::gui::theme::install(ctx, crate::config::Theme::Dark);
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

fn phone(configure: impl FnOnce(&mut AppState)) -> Harness<'static, MobileState> {
    mobile_harness(PHONE, 33, configure)
}

fn open_guide(state: &mut AppState, api_level: u32) {
    let row = state
        .find_row("demo:Cloudflare")
        .expect("the demo winner exists")
        .clone();
    let catalogue = state.catalog.clone();
    state.selected_row = Some(row.id.clone());
    state.guide = Some(android_guide::build(api_level, &row, &catalogue));
}

#[test]
fn mobile_has_bottom_navigation() {
    let mut harness = phone(|_| {});
    harness.run();

    for label in ["Overview", "Servers", "History", "Settings"] {
        let node = harness.get_by_label(label);
        assert!(
            node.rect().height() >= 44.0,
            "{label} tab is below the touch minimum"
        );
    }
    let overview = harness.get_by_label("Overview").rect();
    assert!(
        overview.top() > PHONE.y / 2.0,
        "navigation sits at the bottom of the window"
    );
    assert!(
        overview.bottom() <= PHONE.y + 1.0,
        "the bottom bar is not clipped by the window edge: {overview:?}"
    );
}

#[test]
fn mobile_360_shell_does_not_expand_after_overview() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |_| {});
    harness.run();

    for label in ["Overview", "Servers", "History", "Settings"] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.right() <= SMALL_PHONE.x + 0.5,
            "{label} tab leaves the 360 dp viewport: {rect:?}"
        );
    }

    // Every destination must fit after the Overview has laid out once.
    for page in Page::ALL {
        harness.state_mut().state.page = page;
        harness.run();
        for label in ["Overview", "Servers", "History", "Settings"] {
            let rect = harness.get_by_label(label).rect();
            assert!(
                rect.right() <= SMALL_PHONE.x + 0.5,
                "{label} tab leaves the viewport on {page:?}: {rect:?}"
            );
        }
    }
    harness.state_mut().state.page = Page::Servers;
    harness.run();

    let first = harness
        .state()
        .state
        .catalog
        .first()
        .expect("the catalogue has resolvers")
        .clone();
    let display = if first.label.is_empty() {
        first.provider.clone()
    } else {
        first.label.clone()
    };
    let star = harness
        .get_by_label(&format!("Favourite {display} {}", first.endpoint()))
        .rect();
    assert!(
        star.right() <= SMALL_PHONE.x + 0.5,
        "the server card runs past the 360 dp viewport: {star:?}"
    );
}

#[test]
fn mobile_profile_choices_form_a_2x2_grid() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |_| {});
    harness.run();

    let rects: Vec<_> = ["Quick", "Balanced", "Thorough", "Custom"]
        .iter()
        .map(|label| (label, harness.get_by_label(label).rect()))
        .collect();
    let width = rects[0].1.width();
    for (label, rect) in &rects {
        assert!(
            (rect.width() - width).abs() <= 1.0,
            "{label} is not the same width as the other profile choices: {rect:?}"
        );
        assert!(
            rect.height() >= 44.0,
            "{label} is below the touch minimum: {rect:?}"
        );
        assert!(
            rect.right() <= SMALL_PHONE.x + 0.5,
            "{label} leaves the 360 dp viewport: {rect:?}"
        );
    }
    // Two equal columns: the first and third choices share a row, the second and fourth
    // share the next one.
    assert!((rects[0].1.top() - rects[1].1.top()).abs() <= 1.0);
    assert!((rects[2].1.top() - rects[3].1.top()).abs() <= 1.0);
    assert!(rects[2].1.top() > rects[0].1.bottom());
}

#[test]
fn mobile_benchmark_primary_button_is_48() {
    let mut harness = phone(|_| {});
    harness.run();

    let button = harness.get_by_label("Run benchmark");
    assert!(
        button.rect().height() >= 48.0,
        "the primary benchmark action is not touch-sized"
    );
}

#[test]
fn mobile_server_card_opens_details() {
    let mut harness = phone(|state| {
        state.page = Page::Servers;
        state.search = "cloudflare".into();
    });
    harness.run();

    harness.get_by_label("Open Cloudflare 1.1.1.1").click();
    harness.run();

    assert!(
        harness
            .state()
            .actions
            .iter()
            .any(|action| matches!(action, UiAction::ChooseRow(id) if id == "plain:1.1.1.1:53")),
        "tapping a resolver card opens its detail sheet"
    );
}

#[test]
fn mobile_guide_sheet_has_copy_and_open() {
    let mut harness = phone(|state| open_guide(state, 33));
    harness.run();

    harness.get_by_label("Copy setup value");
    harness.get_by_label("Open Private DNS");
}

#[test]
fn mobile_api_26_hides_private_dns() {
    let mut harness = phone(|state| open_guide(state, 26));
    harness.run();

    harness.get_by_label("Open Wi-Fi");
    assert!(
        harness.query_by_label("Open Private DNS").is_none(),
        "Android 8 has no Private DNS screen"
    );
}

#[test]
fn mobile_modals_fit_small_screens() {
    const NARROW_PHONE: Vec2 = Vec2::new(320.0, 640.0);

    fn assert_modal_fits(size: Vec2, name: &str, configure: impl FnOnce(&mut AppState)) {
        let mut harness = mobile_harness(size, 33, configure);
        harness.run();

        for node in harness.query_all(By::new()) {
            if node.accesskit_node().bounding_box().is_none() {
                continue;
            }
            let rect = node.rect();
            assert!(
                rect.left() >= -1.0,
                "the {name} modal starts outside the {size:?} screen: {rect:?} role={:?} label={:?} value={:?}",
                node.accesskit_node().role(),
                node.accesskit_node().label(),
                node.accesskit_node().value()
            );
            assert!(
                rect.right() <= size.x + 1.0,
                "the {name} modal overflows the {size:?} screen: {rect:?} role={:?} label={:?} value={:?}",
                node.accesskit_node().role(),
                node.accesskit_node().label(),
                node.accesskit_node().value()
            );
        }
    }

    for size in [SMALL_PHONE, NARROW_PHONE] {
        assert_modal_fits(size, "guide", |state| open_guide(state, 33));
        assert_modal_fits(size, "custom editor", |state| state.custom_open = true);
        assert_modal_fits(size, "row details", |state| {
            let row = state
                .find_row("demo:Cloudflare")
                .expect("the demo winner exists")
                .clone();
            state.selected_row = Some(row.id);
            state.details_open = true;
        });
    }
}

#[test]
fn mobile_detail_sheet_title_stays_left_of_close() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        let id = state
            .find_row("demo:Cloudflare")
            .expect("the demo winner exists")
            .id
            .clone();
        if let Some(row) = state
            .result
            .as_mut()
            .and_then(|result| result.rows.iter_mut().find(|row| row.id == id))
        {
            row.label = "Current DNS (163.121.128.135)".into();
        }
        state.selected_row = Some(id);
        state.details_open = true;
    });
    harness.run();

    let mut title = None;
    let mut close = None;
    for node in harness.query_all(By::new()) {
        let ax = node.accesskit_node();
        if ax.label().as_deref() == Some("Close details") {
            close = Some(node.rect());
        }
        if ax
            .value()
            .is_some_and(|value| value.starts_with("Current DNS ("))
        {
            title = Some(node.rect());
        }
    }
    let title = title.expect("the sheet title is rendered");
    let close = close.expect("the close button is rendered");
    assert!(
        title.right() <= close.left() + 1.0,
        "the detail title runs under the close button: title {title:?}, close {close:?}"
    );
}

#[test]
fn mobile_stale_selection_does_not_enable_selected_run() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        state.settings.selected.insert(state.catalog[0].id.clone());
        state.settings.protocol = crate::servers::Protocol::Dot;
        state.catalog =
            crate::providers::catalog(crate::servers::Protocol::Dot, crate::servers::Family::Any);
    });
    harness.run();

    assert!(
        harness
            .get_by_label("Test selected")
            .accesskit_node()
            .is_disabled(),
        "a selection from another protocol must not enable Test selected"
    );
    harness.get_by_label("Run benchmark").click();
    harness.run();
    assert!(
        harness
            .state()
            .actions
            .contains(&UiAction::Run(crate::gui::actions::RunScope::All)),
        "Run benchmark falls back to Test all when no selection matches the protocol"
    );
}

#[test]
fn mobile_test_favourites_disables_for_a_protocol_without_them() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        state.settings.favorites.insert(state.catalog[0].id.clone());
        state.settings.protocol = crate::servers::Protocol::Dot;
        state.catalog =
            crate::providers::catalog(crate::servers::Protocol::Dot, crate::servers::Family::Any);
    });
    harness.run();

    assert!(
        harness
            .get_by_label("Test favourites")
            .accesskit_node()
            .is_disabled(),
        "a favourite from another protocol must not enable Test favourites"
    );
    harness.get_by_label("No favourite is available for the current protocol.");
}

#[test]
fn mobile_opening_about_scrolls_it_into_view() {
    let mut harness = phone(|state| {
        state.page = Page::Settings;
    });
    harness.run();

    // Scroll deep into Settings first: the About card is above the form and must be brought
    // back into view when it opens.
    harness
        .input_mut()
        .events
        .push(eframe::egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -20_000.0),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
    harness.run_steps(10);

    harness.state_mut().state.about_open = true;
    harness.state_mut().state.about_scroll_pending = true;
    harness.run_steps(3);

    let close = harness.get_by_label("Close about").rect();
    assert!(
        close.top() >= 0.0 && close.bottom() <= SMALL_PHONE.y,
        "About opens off-screen instead of scrolling into view: {close:?}"
    );
}

#[test]
fn mobile_custom_resolver_actions_do_not_overlap_the_name() {
    let mut harness = phone(|state| {
        state.page = Page::Settings;
        state.settings.custom = vec![crate::config::CustomResolver {
            name: "Example".to_string(),
            protocol: crate::servers::Protocol::Udp,
            endpoint: "1.2.3.4".to_string(),
            address: None,
        }];
    });
    harness.run();

    harness
        .input_mut()
        .events
        .push(eframe::egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -20_000.0),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
    harness.run_steps(10);

    let name = harness.get_by_label("Example").rect();
    let edit = harness.get_by_label("Edit Example").rect();
    let remove = harness.get_by_label("Remove Example").rect();
    assert!(
        !name.intersects(edit),
        "Edit Example paints over the resolver name: name={name:?} edit={edit:?}"
    );
    assert!(
        !name.intersects(remove),
        "Remove Example paints over the resolver name: name={name:?} remove={remove:?}"
    );
    assert!(
        !edit.intersects(remove),
        "Edit and Remove paint over each other: edit={edit:?} remove={remove:?}"
    );
}

#[test]
fn mobile_layout_has_no_horizontal_overflow() {
    for (name, size) in [("phone", PHONE), ("small", SMALL_PHONE)] {
        for page in Page::ALL {
            let mut harness = mobile_harness(size, 33, move |state| {
                state.page = page;
            });
            harness.run();

            for node in harness.query_all(By::new()) {
                if node.accesskit_node().bounding_box().is_none() {
                    continue;
                }
                let rect = node.rect();
                assert!(
                    rect.right() <= size.x + 1.0,
                    "a widget overflows the {name} width on {page:?}: {rect:?} role={:?} label={:?} value={:?}",
                    node.accesskit_node().role(),
                    node.accesskit_node().label(),
                    node.accesskit_node().value()
                );
                assert!(
                    rect.left() >= -1.0,
                    "a widget starts outside the {name} width on {page:?}: {rect:?}"
                );
            }
        }
    }
}

#[test]
fn mobile_long_run_content_does_not_expand_the_shell() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        // What a real "Test all" run produces on the Redmi: ISP and gateway rows with long
        // labels plus a one-line completion notice.
        state.notify(
            "Finished: Current DNS (163.121.128.134) is fastest at 26.1 ms. Selected nothing.",
            false,
        );
        state.system = Some(crate::system::SystemInfo {
            gateway: Some("192.168.1.1".parse().unwrap()),
            dns: vec![
                "163.121.128.134".parse().unwrap(),
                "163.121.128.135".parse().unwrap(),
                "192.168.1.1".parse().unwrap(),
            ],
            interface: Some("wlan0".into()),
            source: Some("Android ConnectivityManager".into()),
        });
        if let Some(result) = &mut state.result {
            result.rows[0].label = "Current DNS (163.121.128.134)".into();
            result.rows[0].provider = "Current DNS".into();
        }
    });
    harness.run();

    for node in harness.query_all(By::new()) {
        if node.accesskit_node().bounding_box().is_none() {
            continue;
        }
        let rect = node.rect();
        assert!(
            rect.right() <= SMALL_PHONE.x + 1.0,
            "a widget overflows the 360 dp phone: {rect:?} role={:?} label={:?} value={:?}",
            node.accesskit_node().role(),
            node.accesskit_node().label(),
            node.accesskit_node().value()
        );
    }

    let run = harness.get_by_label("Run benchmark").rect();
    assert!(
        run.right() <= SMALL_PHONE.x - 15.0,
        "the app bar action keeps the 16 dp page padding: {run:?}"
    );
}

#[test]
fn mobile_320_adapts_to_a_narrow_screen() {
    const NARROW: Vec2 = Vec2::new(320.0, 640.0);
    let mut harness = mobile_harness(NARROW, 33, |state| {
        state.notify(
            "Finished: Current DNS (163.121.128.134) is fastest at 26.1 ms. Selected nothing.",
            false,
        );
        state.system = Some(crate::system::SystemInfo {
            gateway: Some("192.168.1.1".parse().unwrap()),
            dns: vec![
                "163.121.128.134".parse().unwrap(),
                "163.121.128.135".parse().unwrap(),
                "192.168.1.1".parse().unwrap(),
            ],
            interface: Some("wlan0".into()),
            source: Some("Android ConnectivityManager".into()),
        });
        if let Some(result) = &mut state.result {
            result.rows[0].label = "Current DNS (163.121.128.134)".into();
            result.rows[0].provider = "Current DNS".into();
        }
    });
    harness.run();

    for node in harness.query_all(By::new()) {
        if node.accesskit_node().bounding_box().is_none() {
            continue;
        }
        let rect = node.rect();
        assert!(
            rect.right() <= NARROW.x + 1.0,
            "a widget overflows the 320 dp phone: {rect:?} role={:?} label={:?} value={:?}",
            node.accesskit_node().role(),
            node.accesskit_node().label(),
            node.accesskit_node().value()
        );
    }
    let run = harness.get_by_label("Run benchmark").rect();
    assert!(
        run.right() <= NARROW.x - 15.0 && run.left() >= 15.0,
        "the app bar keeps its padding on a 320 dp screen: {run:?}"
    );
}

#[test]
fn mobile_landscape_keeps_navigation() {
    let mut harness = mobile_harness(LANDSCAPE, 33, |_| {});
    harness.run();

    for label in ["Overview", "Servers", "History", "Settings"] {
        harness.get_by_label(label);
    }
    harness.get_by_label("Run benchmark");
}

#[test]
fn mobile_portrait_respects_system_bars() {
    let insets = SafeArea {
        top: 24,
        right: 0,
        bottom: 48,
        left: 0,
    };
    let mut harness = mobile_harness_with_insets(PHONE, 33, insets, |_| {});
    harness.run();

    let run = harness.get_by_label("Run benchmark").rect();
    assert!(
        run.top() >= 24.0,
        "the app bar starts under the status bar: {run:?}"
    );

    let overview = harness.get_by_label("Overview").rect();
    assert!(
        overview.bottom() <= PHONE.y - 48.0 + 1.0,
        "the bottom navigation is under the system buttons: {overview:?}"
    );
    assert!(
        overview.top() > PHONE.y / 2.0,
        "navigation still sits at the bottom of the safe area"
    );
}

#[test]
fn mobile_landscape_respects_side_insets() {
    let insets = SafeArea {
        top: 24,
        right: 48,
        bottom: 24,
        left: 48,
    };
    let mut harness = mobile_harness_with_insets(LANDSCAPE, 33, insets, |_| {});
    harness.run();

    for node in harness.query_all(By::new()) {
        if node.accesskit_node().bounding_box().is_none() {
            continue;
        }
        let rect = node.rect();
        assert!(
            rect.left() >= 48.0 - 1.0,
            "a widget starts under the left cutout: {rect:?}"
        );
        assert!(
            rect.right() <= LANDSCAPE.x - 48.0 + 1.0,
            "a widget runs under the right cutout: {rect:?} role={:?} label={:?} value={:?}",
            node.accesskit_node().role(),
            node.accesskit_node().label(),
            node.accesskit_node().value()
        );
    }

    // Scrolled page content may extend past its viewport, so only the shell chrome is
    // checked vertically: the app bar clears the status bar and the tabs clear the buttons.
    let run = harness.get_by_label("Run benchmark").rect();
    assert!(
        run.top() >= 24.0 - 1.0,
        "the app bar starts under the status bar: {run:?}"
    );
    let overview = harness.get_by_label("Overview").rect();
    assert!(
        overview.bottom() <= LANDSCAPE.y - 24.0 + 1.0,
        "the bottom navigation is under the system buttons: {overview:?}"
    );
}

#[test]
fn app_refreshes_safe_area_from_the_android_host() {
    let insets = SafeArea {
        top: 24,
        right: 0,
        bottom: 48,
        left: 0,
    };
    let mut backend = crate::gui::backend::TestBackend::new();
    backend.android = Some(crate::android::FakeAndroidHost::new(33).with_safe_area(insets));
    let backend = std::rc::Rc::new(backend);
    let app = crate::gui::app::App::new(
        AppState::demo(),
        ViewContext::android_for_test(33),
        Box::new(backend),
    );

    let mut harness = Harness::builder()
        .with_size(PHONE)
        .with_context_setup(|ctx| {
            crate::gui::theme::install(ctx, crate::config::Theme::Dark);
        })
        .build_ui_state(|ui, app: &mut crate::gui::app::App| app.render(ui), app);
    harness.run();

    assert_eq!(harness.state().cx.safe_area, insets);
}

#[test]
fn mobile_server_list_only_builds_visible_cards() {
    let mut harness = phone(|state| {
        state.page = Page::Servers;
    });
    harness.run();

    let open_cards = harness
        .query_all(By::new())
        .filter(|node| {
            node.accesskit_node()
                .label()
                .is_some_and(|label| label.starts_with("Open "))
        })
        .count();
    assert!(
        open_cards < 20,
        "the Servers tab built {open_cards} cards; it must only build the visible ones"
    );
    assert!(open_cards >= 1, "the visible cards are still rendered");
}

#[test]
fn mobile_360_keeps_server_pills_on_one_line() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        state.page = Page::Servers;
    });
    harness.run();

    let mut checked = 0;
    for node in harness.query_all(By::new()) {
        let node_label = node.accesskit_node();
        let text = node_label.label().or_else(|| node_label.value());
        if text.as_deref() == Some("Not measured") {
            assert!(
                node.rect().height() < 30.0,
                "a status pill wrapped onto two lines: {:?}",
                node.rect()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "the mobile card status pills are rendered");

    for node in harness.query_all(By::new()) {
        if node.accesskit_node().bounding_box().is_none() {
            continue;
        }
        let rect = node.rect();
        assert!(
            rect.right() <= SMALL_PHONE.x + 1.0,
            "a widget overflows the 360 dp phone: {rect:?}"
        );
    }
}

#[test]
fn mobile_settings_advanced_labels_stay_horizontal() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |state| {
        state.page = Page::Settings;
    });
    harness.run();

    let mut checked = 0;
    for node in harness.query_all(By::new()) {
        // Match the Label nodes, not the SpinButton nodes that reference them through
        // `labelled_by`; the latter are 44 dp tall by design.
        let text = node.accesskit_node().value();
        if matches!(
            text.as_deref(),
            Some("Timeout") | Some("Cached rounds") | Some("Random lookups")
        ) {
            let rect = node.rect();
            assert!(
                rect.height() < 20.0,
                "{text:?} stacked vertically on a phone: {rect:?}"
            );
            if matches!(
                text.as_deref(),
                Some("Cached rounds") | Some("Random lookups")
            ) {
                assert!(
                    rect.width() >= 70.0,
                    "{text:?} was squeezed by the wrapping row: {rect:?}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 3, "the advanced benchmark labels are rendered");
}

#[test]
fn mobile_benchmark_counts_sit_below_the_buttons() {
    let mut harness = mobile_harness(SMALL_PHONE, 33, |_| {});
    harness.run();

    let counts = format!(
        "{} selected · {} available",
        harness.state().state.selected_in_catalog(),
        harness.state().state.visible_catalog().len()
    );
    let test_all = harness.get_by_label("Test all").rect();
    let label = harness
        .query_all(By::new())
        .find(|node| node.accesskit_node().value().as_deref() == Some(counts.as_str()))
        .expect("the selection counts are rendered")
        .rect();
    assert!(
        label.top() >= test_all.bottom() - 1.0,
        "the counts crowd the touch buttons: label {label:?}, Test all {test_all:?}"
    );
}

#[test]
fn mobile_server_list_reaches_the_last_card_after_scrolling() {
    let mut harness = phone(|state| {
        state.page = Page::Servers;
    });
    harness.run();

    let last = harness
        .state()
        .state
        .catalog
        .last()
        .expect("the built-in catalogue is not empty")
        .clone();
    let display = if last.label.is_empty() {
        last.provider.clone()
    } else {
        last.label.clone()
    };
    let label = format!("Open {display} {}", last.endpoint());

    harness
        .input_mut()
        .events
        .push(eframe::egui::Event::PointerMoved(eframe::egui::Pos2::new(
            SMALL_PHONE.x / 2.0,
            400.0,
        )));
    harness
        .input_mut()
        .events
        .push(eframe::egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -20_000.0),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
    harness.run_steps(10);

    harness.get_by_label(&label);
}
