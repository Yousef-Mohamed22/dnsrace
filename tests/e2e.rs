//! Integration tests for the engine. They exercise the same pipeline as the desktop app against
//! simulated DNS servers on loopback, without opening a window or launching a terminal binary.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{
    Behavior, closed_udp_port, mock_candidate, run_engine, run_engine_cancelled, sample_row,
    spawn_mock, spawn_tcp_mock, tcp_stub,
};
use dnsrace::bench::{Note, SecurityStatus};
use dnsrace::engine::{self, RunEvent, RunRequest, Stage};
use dnsrace::report;
use dnsrace::servers::{Candidate, Family, Protocol};

fn request(candidates: Vec<Candidate>, protocol: Protocol, wait_ms: u64) -> RunRequest {
    RunRequest {
        candidates,
        protocol,
        family: Family::Any,
        wait: Duration::from_millis(wait_ms),
        jobs: 4,
        repeat: 1,
        hostnames: Vec::new(),
        connect_port: 443,
        include_system: false,
        include_nearby: false,
    }
}

fn find<'a>(result: &'a dnsrace::engine::RunResult, label: &str) -> &'a dnsrace::bench::Row {
    result
        .rows
        .iter()
        .find(|row| row.label == label)
        .unwrap_or_else(|| panic!("no row for {label}: {:#?}", result.rows))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ranks_flags_and_steers() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 40).validated())
        .await
        .expect("bind fast");
    let mid = spawn_mock("127.0.0.1:0", Behavior::new(30, 60))
        .await
        .expect("bind mid");
    let hijacker = spawn_mock(
        "127.0.0.1:0",
        Behavior {
            hijack: true,
            ..Behavior::new(10, 12)
        },
    )
    .await
    .expect("bind hijacker");
    let silent = spawn_mock(
        "127.0.0.1:0",
        Behavior {
            only_google: true,
            ..Behavior::new(5, 8)
        },
    )
    .await
    .expect("bind silent");
    let dead_port = closed_udp_port().await;
    let tcp_port = tcp_stub().await;

    let candidates = vec![
        mock_candidate(&fast, "Fast", "Fast"),
        mock_candidate(&mid, "Mid", "Mid"),
        mock_candidate(&hijacker, "Hijacker", "Hijacker"),
        mock_candidate(&silent, "Silent", "Silent"),
        Candidate::plain(
            format!("127.0.0.1:{dead_port}").parse().expect("addr"),
            "Dead",
            "Dead",
        ),
    ];
    let mut request = request(candidates, Protocol::Udp, 200);
    request.hostnames = vec!["steer.test".into(), "steer2.test".into()];
    request.connect_port = tcp_port;
    let (result, events) = run_engine(request);
    let result = result.expect("the run should finish");

    assert_eq!(result.scanned, 5);
    assert_eq!(result.tested, 4);
    assert_eq!(result.rows.len(), 4);
    assert!(!result.rows.iter().any(|row| row.label == "Dead"));

    let fast_row = find(&result, "Fast");
    let mid_row = find(&result, "Mid");
    let hijacker_row = find(&result, "Hijacker");
    let silent_row = find(&result, "Silent");

    assert!(fast_row.score < mid_row.score);
    assert!(mid_row.score < hijacker_row.score);
    assert!(hijacker_row.score < silent_row.score);
    assert_eq!(fast_row.security_status(), SecurityStatus::Validated);
    assert_eq!(hijacker_row.note, Note::Hijacks);
    assert_eq!(silent_row.note, Note::Unreliable);
    assert_eq!(result.best().map(|row| row.label.as_str()), Some("Fast"));

    // The live event stream should include one row per server and end with Finished.
    let row_events = events
        .iter()
        .filter(|event| matches!(event, RunEvent::Row(_)))
        .count();
    assert_eq!(row_events, 4);
    assert!(events.iter().any(|event| matches!(
        event,
        RunEvent::Stage {
            stage: Stage::Test,
            total: 4,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RunEvent::Stage {
            stage: Stage::Done,
            ..
        }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunEvent::Finished(_)))
    );

    // Only the healthy plain rows are used for steering. Both return 127.0.0.1 and the TCP
    // stub accepts the connection on the chosen port.
    assert_eq!(result.steerings.len(), 2);
    for steering in &result.steerings {
        assert_eq!(steering.servers, 2);
        assert!(steering.host == "steer.test" || steering.host == "steer2.test");
        let entry = steering
            .entries
            .iter()
            .find(|entry| entry.ip.to_string() == "127.0.0.1")
            .expect("localhost steering address");
        assert_eq!(entry.returned_by, 2);
        assert_eq!(entry.connects.len(), 3);
        assert!(entry.connects.iter().all(Option::is_some));
    }
    assert_eq!(result.steerings[0].host, "steer.test");
    assert_eq!(result.steerings[1].host, "steer2.test");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bogus_dnssec_answer_is_never_recommended() {
    let bogus = spawn_mock(
        "127.0.0.1:0",
        Behavior::new(1, 2).validated().accepts_bogus(),
    )
    .await
    .expect("bind bogus");
    let good = spawn_mock("127.0.0.1:0", Behavior::new(30, 40).validated())
        .await
        .expect("bind good");

    let request = request(
        vec![
            mock_candidate(&bogus, "Bogus", "Bogus"),
            mock_candidate(&good, "Good", "Good"),
        ],
        Protocol::Udp,
        200,
    );
    let (result, _) = run_engine(request);
    let result = result.expect("the run should finish");
    let bogus_row = find(&result, "Bogus");
    let good_row = find(&result, "Good");
    assert_eq!(bogus_row.security_status(), SecurityStatus::BogusAccepted);
    assert!(!bogus_row.recommendable());
    assert!(good_row.recommendable());
    assert!(bogus_row.score < good_row.score);
    assert_eq!(result.best().map(|row| row.label.as_str()), Some("Good"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exports_include_results_and_steering() {
    let mock = spawn_mock("127.0.0.1:0", Behavior::new(2, 20).validated())
        .await
        .expect("bind");
    let tcp_port = tcp_stub().await;
    let mut request = request(
        vec![mock_candidate(&mock, "Fast", "Fast")],
        Protocol::Udp,
        200,
    );
    request.hostnames = vec!["steer.test".into()];
    request.connect_port = tcp_port;
    let (result, _) = run_engine(request);
    let result = result.expect("the run should finish");

    let json: serde_json::Value =
        serde_json::from_str(&report::json(&result)).expect("valid JSON report");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["protocol"], "UDP");
    assert_eq!(json["tested"], 1);
    assert_eq!(json["results"][0]["rank"], 1);
    assert_eq!(json["results"][0]["name"], "Fast");
    assert_eq!(json["recommendation"]["name"], "Fast");
    assert_eq!(json["steerings"][0]["host"], "steer.test");
    assert_eq!(json["results"][0]["security"], "DNSSEC validated");

    let csv = report::csv(&result);
    assert!(csv.starts_with("rank,provider,name,transport,server,"));
    assert_eq!(csv.lines().count(), 2);
    assert!(csv.contains("Fast"));

    let html = report::html(&result);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("<h2>Steering</h2>"));
    assert!(html.contains("steer.test"));
    assert!(html.contains("DNSSEC validated"));

    let summary = report::summary_text(&result);
    assert!(summary.contains("Fastest: Fast"));
    assert!(summary.contains("Security: DNSSEC validated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_transport_works() {
    let mock = spawn_tcp_mock("127.0.0.1:0", Behavior::new(2, 20).validated())
        .await
        .expect("bind tcp dns");
    let request = request(
        vec![mock_candidate(&mock, "Fast", "Fast")],
        Protocol::Tcp,
        250,
    );
    let (result, _) = run_engine(request);
    let result = result.expect("the TCP run should finish");
    assert_eq!(result.tested, 1);
    let row = find(&result, "Fast");
    assert_eq!(row.transport, Protocol::Tcp);
    assert_eq!(row.security_status(), SecurityStatus::Validated);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ipv6_servers_work_where_the_machine_supports_ipv6() {
    let Some(v6) = spawn_mock("[::1]:0", Behavior::new(3, 20).validated()).await else {
        eprintln!("skipping: no IPv6 loopback on this machine");
        return;
    };
    let v4 = spawn_mock("127.0.0.1:0", Behavior::new(50, 80))
        .await
        .expect("bind v4");
    let request = request(
        vec![
            mock_candidate(&v6, "V6", "V6"),
            mock_candidate(&v4, "V4", "V4"),
        ],
        Protocol::Udp,
        250,
    );
    let (result, _) = run_engine(request);
    let result = result.expect("the IPv6 run should finish");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0].label, "V6");
    assert!(result.rows[0].score < result.rows[1].score);
}

#[tokio::test]
async fn a_cancelled_run_returns_partial_state() {
    let mock = spawn_mock("127.0.0.1:0", Behavior::new(10, 20))
        .await
        .expect("bind");
    let request = request(
        vec![mock_candidate(&mock, "Fast", "Fast")],
        Protocol::Udp,
        100,
    );
    let cancel = Arc::new(AtomicBool::new(true));
    let (result, events) = run_engine_cancelled(request, cancel);
    let result = result.expect("cancellation is not an error");
    assert_eq!(result.scanned, 0);
    assert_eq!(result.tested, 0);
    assert!(result.rows.is_empty());
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunEvent::Cancelled(_)))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RunEvent::Finished(_)))
    );
}

#[tokio::test]
async fn an_empty_candidate_list_is_an_error() {
    let request = request(Vec::new(), Protocol::Udp, 100);
    let (result, _) = run_engine(request);
    let error = result.expect_err("an empty run should be an error");
    assert!(error.contains("no servers"), "{error}");
}

#[test]
fn apply_addresses_follow_the_provider_variant_only() {
    let malware = "Cloudflare (malware)";
    let candidates = vec![
        Candidate::plain("1.1.1.2:53".parse().unwrap(), "Cloudflare", malware),
        Candidate::plain("1.0.0.2:53".parse().unwrap(), "Cloudflare", malware),
        Candidate::plain("1.1.1.1:53".parse().unwrap(), "Cloudflare", "Cloudflare"),
    ];
    let row = sample_row("plain:1.1.1.2:53", malware, "1.1.1.2", 10.0);
    let addresses = engine::apply_addresses(&row, &candidates);
    let expected: Vec<std::net::IpAddr> =
        vec!["1.1.1.2".parse().unwrap(), "1.0.0.2".parse().unwrap()];
    assert_eq!(addresses, expected);
}

#[test]
fn a_custom_port_is_not_applyable() {
    let mut row = sample_row("plain:9.9.9.9:5353", "Custom", "9.9.9.9", 10.0);
    row.addr = Some("9.9.9.9:5353".parse().unwrap());
    assert!(engine::apply_addresses(&row, &[]).is_empty());
}
