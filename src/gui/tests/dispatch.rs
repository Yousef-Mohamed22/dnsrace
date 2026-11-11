//! Dispatch tests: every user action must reach exactly one backend effect and leave the
//! interface in a state a person can understand.
//!
//! These tests use `TestBackend`, which records calls and returns canned channels, so no network
//! query, clipboard, file or administrator prompt is touched.

use std::rc::Rc;

use crate::config::Settings;
use crate::engine::{RunEvent, Stage};
use crate::gui::actions::{ReportFormat, RunScope, UiAction};
use crate::gui::app::App;
use crate::gui::backend::{BackendCall, BackendReply, TestBackend};
use crate::gui::state::{AppState, ViewContext};
use crate::platform::AppliedDns;
use crate::profile::Profile;
use crate::servers::Protocol;

fn test_app() -> (App, Rc<TestBackend>) {
    let backend = Rc::new(TestBackend::new());
    let app = App::new(
        AppState::demo(),
        ViewContext::desktop_for_test(),
        Box::new(backend.clone()),
    );
    (app, backend)
}

fn applied_dns() -> AppliedDns {
    AppliedDns {
        platform: "test".into(),
        method: "test".into(),
        interface: "test0".into(),
        applied: vec!["1.1.1.1".parse().unwrap()],
        previous: Default::default(),
        timestamp: 0,
    }
}

#[test]
fn dispatch_run_starts_one_backend_run() {
    let (mut app, backend) = test_app();
    let id = app.state.catalog[0].id.clone();
    app.state.settings.selected.insert(id);

    backend.push_reply(BackendReply::RunEvent(Box::new(RunEvent::Stage {
        stage: Stage::Scan,
        done: 1,
        total: 5,
    })));
    app.dispatch(UiAction::Run(RunScope::Selected));

    assert!(app.state.running);
    assert_eq!(
        backend
            .take_calls()
            .into_iter()
            .filter(|call| matches!(call, BackendCall::Run { .. }))
            .count(),
        1
    );

    // A second request while a run is active must not start another worker.
    app.dispatch(UiAction::Run(RunScope::All));
    assert_eq!(
        backend
            .take_calls()
            .into_iter()
            .filter(|call| matches!(call, BackendCall::Run { .. }))
            .count(),
        0
    );
}

#[test]
fn dispatch_apply_confirms_and_calls_backend() {
    let (mut app, backend) = test_app();
    let id = "demo:Cloudflare".to_string();
    app.state.selected_row = Some(id.clone());

    app.dispatch(UiAction::Apply(id));
    assert!(app.state.confirm_apply);
    let pending = app
        .state
        .pending_apply
        .as_ref()
        .expect("apply details are pending");
    assert!(
        pending
            .addresses
            .contains(&"1.1.1.1".parse::<std::net::IpAddr>().unwrap()),
        "the pending addresses include the row's main address"
    );

    backend.push_reply(BackendReply::Apply(Err("permission denied".into())));
    app.dispatch(UiAction::ConfirmApply);

    assert!(!app.state.confirm_apply);
    let calls = backend.take_calls();
    let applied = calls
        .iter()
        .find_map(|call| match call {
            BackendCall::Apply(addresses) => Some(addresses.clone()),
            _ => None,
        })
        .expect("the backend received an apply call");
    assert!(
        applied.contains(&"1.1.1.1".parse::<std::net::IpAddr>().unwrap()),
        "the apply call includes the row's main address"
    );
    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("permission denied"));
}

#[test]
fn dispatch_revert_calls_backend() {
    let (mut app, backend) = test_app();
    app.state.applied = Some(applied_dns());

    backend.push_reply(BackendReply::Revert(Err("permission denied".into())));
    app.dispatch(UiAction::Revert);
    assert!(app.state.confirm_revert);
    app.dispatch(UiAction::ConfirmRevert);

    assert!(!app.state.confirm_revert);
    assert!(backend.take_calls().contains(&BackendCall::Revert));
    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("permission denied"));
}

#[test]
fn dispatch_export_uses_run_result() {
    let (mut app, backend) = test_app();
    app.dispatch(UiAction::Export(ReportFormat::Json));

    assert!(
        backend
            .take_calls()
            .contains(&BackendCall::Export(ReportFormat::Json))
    );
    let exported = backend.exported_result.borrow();
    let result = exported.as_ref().expect("the backend received a result");
    assert_eq!(
        result.best().map(|row| row.label.as_str()),
        Some("Cloudflare")
    );
    let notice = app.state.notices.last().expect("success notice");
    assert!(!notice.error);
    assert!(notice.text.contains("Saved"));
}

#[test]
fn dispatch_copy_failure_shows_notice() {
    let (mut app, backend) = test_app();
    backend.push_reply(BackendReply::Copy(Err("clipboard denied".into())));
    app.dispatch(UiAction::CopyCurrentDns);

    assert!(
        backend
            .take_calls()
            .contains(&BackendCall::Copy("9.9.9.9".to_string()))
    );
    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("clipboard denied"));
}

#[test]
fn dispatch_open_link_failure_shows_notice() {
    let (mut app, backend) = test_app();
    backend.push_reply(BackendReply::OpenLink(Err("no browser".into())));
    app.dispatch(UiAction::OpenWebsite("demo:Cloudflare".into()));

    assert!(backend.take_calls().contains(&BackendCall::OpenLink(
        "https://example.com/Cloudflare".to_string()
    )));
    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("no browser"));
}

#[test]
fn dispatch_custom_save_validates_and_persists() {
    let (mut app, backend) = test_app();
    app.dispatch(UiAction::OpenCustom);
    app.state.custom_draft.name = "Example".into();
    app.state.custom_draft.endpoint = "not a URL".into();
    app.dispatch(UiAction::SaveCustom);

    assert!(app.state.custom_open, "an invalid draft stays open");
    assert!(app.state.custom_draft.error.is_some());
    assert!(app.state.settings.custom.is_empty());
    assert!(!backend.take_calls().contains(&BackendCall::SaveSettings));

    app.state.custom_draft.endpoint = "https://dns.example.com/dns-query".into();
    app.dispatch(UiAction::SaveCustom);

    assert!(!app.state.custom_open);
    assert_eq!(app.state.settings.custom.len(), 1);
    assert_eq!(
        app.state.settings.custom[0].endpoint,
        "https://dns.example.com/dns-query"
    );
    assert!(backend.take_calls().contains(&BackendCall::SaveSettings));
    assert!(
        app.state
            .settings
            .selected
            .contains("doh:https://dns.example.com/dns-query"),
        "a saved custom resolver is selected for testing"
    );
}

#[test]
fn dispatch_reset_settings_loads_defaults() {
    let (mut app, backend) = test_app();
    app.state.settings.protocol = Protocol::Doh;
    app.state.settings.profile = Profile::Thorough;
    app.state
        .settings
        .custom
        .push(crate::config::CustomResolver {
            name: "Example".into(),
            protocol: Protocol::Doh,
            endpoint: "https://dns.example.com/dns-query".into(),
            address: None,
        });

    app.dispatch(UiAction::ResetSettings);

    let defaults = Settings::default();
    assert_eq!(app.state.settings.protocol, defaults.protocol);
    assert_eq!(app.state.settings.profile, Profile::Balanced);
    assert!(app.state.settings.custom.is_empty());
    assert_eq!(app.state.settings.timeout_ms, defaults.timeout_ms);
    assert!(backend.take_calls().contains(&BackendCall::SaveSettings));
}

#[test]
fn dispatch_edit_custom_updates_in_place_and_moves_selection() {
    let (mut app, backend) = test_app();
    let old_endpoint = "https://old.example.com/dns-query";
    let new_endpoint = "https://new.example.com/dns-query";
    app.state
        .settings
        .custom
        .push(crate::config::CustomResolver {
            name: "Example".into(),
            protocol: Protocol::Doh,
            endpoint: old_endpoint.into(),
            address: None,
        });
    let old_id = crate::providers::custom_doh("Example", old_endpoint)
        .expect("a valid DoH endpoint")
        .id;
    app.state.settings.selected.insert(old_id.clone());

    app.dispatch(UiAction::EditCustom(0));

    assert!(app.state.custom_open);
    assert_eq!(app.state.custom_edit, Some(0));
    assert_eq!(app.state.custom_draft.name, "Example");
    assert_eq!(app.state.custom_draft.protocol, Protocol::Doh);
    assert_eq!(app.state.custom_draft.endpoint, old_endpoint);

    app.state.custom_draft.endpoint = new_endpoint.into();
    app.dispatch(UiAction::SaveCustom);

    assert!(!app.state.custom_open);
    assert_eq!(app.state.custom_edit, None);
    assert_eq!(app.state.settings.custom.len(), 1);
    assert_eq!(app.state.settings.custom[0].endpoint, new_endpoint);
    assert!(!app.state.settings.selected.contains(&old_id));
    assert!(
        app.state
            .settings
            .selected
            .contains("doh:https://new.example.com/dns-query")
    );
    assert!(backend.take_calls().contains(&BackendCall::SaveSettings));
}

#[test]
fn dispatch_edit_custom_cancel_keeps_the_original() {
    let (mut app, _backend) = test_app();
    app.state
        .settings
        .custom
        .push(crate::config::CustomResolver {
            name: "Example".into(),
            protocol: Protocol::Doh,
            endpoint: "https://old.example.com/dns-query".into(),
            address: None,
        });

    app.dispatch(UiAction::EditCustom(0));
    app.state.custom_draft.name = "Changed".into();
    app.dispatch(UiAction::CancelCustom);

    assert_eq!(app.state.settings.custom[0].name, "Example");
    assert_eq!(app.state.custom_edit, None);
    assert!(!app.state.custom_open);
}

#[test]
fn dispatch_share_history_calls_share_text() {
    let (mut app, backend) = test_app();
    app.dispatch(UiAction::ShareHistory(0, ReportFormat::Json));

    let calls = backend.take_calls();
    let shared = calls
        .iter()
        .find_map(|call| match call {
            BackendCall::ShareText { subject, text } => Some((subject.clone(), text.clone())),
            _ => None,
        })
        .expect("the history report was shared");
    assert!(shared.0.contains("history"));
    assert!(shared.1.contains("Cloudflare"));
    assert!(!app.state.notices.last().expect("success notice").error);
}

#[test]
fn dispatch_share_history_failure_shows_notice() {
    let (mut app, backend) = test_app();
    backend.push_reply(BackendReply::ShareText(Err("no share sheet".into())));
    app.dispatch(UiAction::ShareHistory(0, ReportFormat::Json));

    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("no share sheet"));
}

#[test]
fn dispatch_share_report_failure_shows_notice() {
    let (mut app, backend) = test_app();
    backend.push_reply(BackendReply::ShareText(Err("no share sheet".into())));
    app.dispatch(UiAction::ShareReport(ReportFormat::Json));

    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("no share sheet"));
}

#[test]
fn dispatch_about_links_open_the_project_urls() {
    let (mut app, backend) = test_app();
    for (link, expected) in [
        (
            crate::gui::actions::AboutLink::Project,
            "https://github.com/Yousef-Mohamed22/dnsrace",
        ),
        (
            crate::gui::actions::AboutLink::Issues,
            "https://github.com/Yousef-Mohamed22/dnsrace/issues",
        ),
        (
            crate::gui::actions::AboutLink::Licence,
            "https://github.com/Yousef-Mohamed22/dnsrace/blob/main/LICENSE",
        ),
    ] {
        app.dispatch(UiAction::OpenAboutLink(link));
        assert!(
            backend
                .take_calls()
                .contains(&BackendCall::OpenLink(expected.to_string()))
        );
        assert!(!app.state.notices.last().expect("success notice").error);
    }
}

#[test]
fn dispatch_guide_copy_failure_shows_notice() {
    let (mut app, backend) = test_app();
    let row = app
        .state
        .find_row("demo:Cloudflare")
        .expect("the demo winner exists")
        .clone();
    let catalogue = app.state.catalog.clone();
    app.state.guide = Some(crate::android_guide::build(33, &row, &catalogue));
    backend.push_reply(BackendReply::Copy(Err("clipboard denied".into())));

    app.dispatch(UiAction::CopyGuideText);

    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("clipboard denied"));
}

#[test]
fn dispatch_guide_open_failure_shows_notice() {
    let (mut app, _backend) = test_app();
    app.dispatch(UiAction::OpenPrivateDns);

    let notice = app.state.notices.last().expect("failure notice");
    assert!(notice.error);
    assert!(notice.text.contains("Android"));
}

#[test]
fn dispatch_advanced_value_switches_profile_to_custom() {
    let (mut app, _backend) = test_app();
    for action in [
        UiAction::SetTimeout(1200),
        UiAction::SetJobs(4),
        UiAction::SetRepeat(2),
        UiAction::SetCachedRounds(2),
        UiAction::SetRandomLookups(3),
    ] {
        app.state.settings.profile = Profile::Balanced;
        app.dispatch(action);
        assert_eq!(app.state.settings.profile, Profile::Custom);
    }
}
