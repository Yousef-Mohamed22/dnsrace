//! End-to-end tests: run the real binary against simulated DNS servers on loopback.

mod common;

use common::{Behavior, TempFile, closed_udp_port, line_with, position, run_dnsrace, spawn_mock};
use tokio::net::TcpListener;

/// A TCP listener that accepts and drops connections, standing in for a web server.
async fn tcp_stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind tcp");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        loop {
            let _ = listener.accept().await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ranks_flags_and_steers() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 40))
        .await
        .expect("bind");
    let mid = spawn_mock("127.0.0.1:0", Behavior::new(120, 160))
        .await
        .expect("bind");
    let hijacker = spawn_mock(
        "127.0.0.1:0",
        Behavior {
            hijack: true,
            ..Behavior::new(10, 12)
        },
    )
    .await
    .expect("bind");
    let silent = spawn_mock(
        "127.0.0.1:0",
        Behavior {
            only_google: true,
            ..Behavior::new(5, 8)
        },
    )
    .await
    .expect("bind");
    let dead_port = closed_udp_port().await;
    let tcp_port = tcp_stub().await.to_string();
    let list = TempFile::with(&format!(
        "{} Fast\n{} Mid\n{} Hijacker\n{} Silent\n127.0.0.1:{dead_port} Dead\n",
        fast.addr, mid.addr, hijacker.addr, silent.addr
    ));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--timeout",
        "600",
        "--connect-port",
        &tcp_port,
        "steer.test",
        "steer2.test",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let text = &out.stdout;

    assert!(
        text.contains("Stage 1: quick scan of 5 candidates..."),
        "{text}"
    );
    assert!(text.contains("4 answered."), "{text}"); // the dead port never answers
    assert!(!text.contains("Dead"), "{text}");

    // ranking: fast, then mid, then the flagged servers
    let fast_at = position(text, " Fast ");
    let mid_at = position(text, " Mid ");
    let hijacker_at = position(text, " Hijacker ");
    let silent_at = position(text, " Silent ");
    assert!(
        fast_at < mid_at && mid_at < hijacker_at && hijacker_at < silent_at,
        "unexpected order:\n{text}"
    );
    assert!(line_with(text, " Hijacker ").ends_with("HIJACKS-NXDOMAIN"));
    assert!(line_with(text, " Silent ").ends_with("UNRELIABLE"));
    assert!(!line_with(text, " Fast ").contains("HIJACKS"));

    // the summary only recommends healthy servers
    assert!(
        text.contains(&format!("Recommended:            {} (Fast)", fast.addr)),
        "{text}"
    );
    assert!(!text.contains("Hijacker)"), "{text}");
    assert!(!text.contains("Silent)"), "{text}");

    // the steering check: both healthy servers return 127.0.0.1, and we can connect to it
    assert!(text.contains("== steer.test:"), "{text}");
    let steer = line_with(text, "returned by");
    assert!(steer.starts_with("127.0.0.1"), "{steer}");
    assert!(
        steer.contains(" ms") && !steer.contains("FAILED"),
        "{steer}"
    );
    assert!(steer.contains("returned by 2 of 2 servers"), "{steer}");
    assert!(text.contains("== steer2.test:"), "{text}");
    assert!(
        position(text, "== steer.test:") < position(text, "== steer2.test:"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn json_output_is_valid_and_stdout_is_only_json() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 40))
        .await
        .expect("bind");
    let mid = spawn_mock("127.0.0.1:0", Behavior::new(120, 160))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n{} Mid\n", fast.addr, mid.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--json",
        "--timeout",
        "500",
        "steer.test",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", out.stdout));

    assert_eq!(v["dnsrace"], env!("CARGO_PKG_VERSION"));
    assert!(v["data_center"].is_null());
    let results = v["results"].as_array().expect("a results array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["rank"], 1);
    assert_eq!(results[0]["name"], "Fast");
    assert_eq!(results[0]["server"], fast.addr.to_string());
    assert!(results[0]["flag"].is_null());
    assert_eq!(results[0]["current"], false);
    assert_eq!(results[0]["timeouts"], 0);
    assert_eq!(results[0]["dnssec"], false);
    assert!(results[0]["min_ms"].as_f64().is_some());
    assert!(results[0]["p95_ms"].as_f64().is_some());
    assert!(results[0]["jitter_ms"].as_f64().is_some());
    assert!(results[0]["cached_ms"].as_f64().unwrap() < results[1]["cached_ms"].as_f64().unwrap());
    assert_eq!(v["recommended"], fast.addr.to_string());
    assert_eq!(v["fastest"], fast.addr.to_string());
    assert_eq!(v["fastest_known"], fast.addr.to_string());
    assert_eq!(v["steering"]["host"], "steer.test");
    assert_eq!(v["steerings"].as_array().unwrap().len(), 1);
    assert_eq!(v["steerings"][0]["host"], "steer.test");

    // progress text stays out of the JSON stream
    assert!(out.stderr.contains("Stage 1:"), "{}", out.stderr);
    assert!(!out.stdout.contains("Stage 1:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exports_write_json_csv_and_html_reports() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 40))
        .await
        .expect("bind");
    let mid = spawn_mock("127.0.0.1:0", Behavior::new(120, 160))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n{} Mid\n", fast.addr, mid.addr));
    let json_path = TempFile::with("");
    let csv_path = TempFile::with("");
    let html_path = TempFile::with("");

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--timeout",
        "500",
        "--export-json",
        &json_path.arg(),
        "--export-csv",
        &csv_path.arg(),
        "--export-html",
        &html_path.arg(),
        "steer.test",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let json_text = std::fs::read_to_string(json_path.path()).expect("read json export");
    let json: serde_json::Value = serde_json::from_str(&json_text).expect("valid exported JSON");
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert_eq!(json["steerings"][0]["host"], "steer.test");

    let csv_text = std::fs::read_to_string(csv_path.path()).expect("read csv export");
    assert!(csv_text.starts_with("rank,server,name,"));
    assert_eq!(csv_text.lines().count(), 3);

    let html_text = std::fs::read_to_string(html_path.path()).expect("read html export");
    assert!(html_text.contains("<!DOCTYPE html>"));
    assert!(html_text.contains("<h2>Steering</h2>"));
    assert!(html_text.contains("steer.test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ipv6_servers_work_where_the_machine_supports_ipv6() {
    let Some(v6) = spawn_mock("[::1]:0", Behavior::new(3, 40)).await else {
        eprintln!("skipping: no IPv6 loopback on this machine");
        return;
    };
    let v4 = spawn_mock("127.0.0.1:0", Behavior::new(120, 160))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} V6\n{} V4\n", v6.addr, v4.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("2 answered."), "{}", out.stdout);
    let line = line_with(&out.stdout, " V6 ");
    assert!(line.contains(&v6.addr.to_string()), "{line}");
    assert!(!line.contains("UNRELIABLE"), "{line}");
    assert!(position(&out.stdout, " V6 ") < position(&out.stdout, " V4 "));
}

#[tokio::test]
async fn help_version_and_usage_errors() {
    let help = run_dnsrace(&["--help"]).await;
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("USAGE:") && help.stdout.contains("--countries"));

    let version = run_dnsrace(&["--version"]).await;
    assert_eq!(version.code, 0);
    assert_eq!(
        version.stdout.trim(),
        format!("dnsrace {}", env!("CARGO_PKG_VERSION"))
    );

    for args in [
        &["--bogus"][..],
        &["-c", "egypt"],
        &["-k"],
        &["https://example.com/x"],
    ] {
        let out = run_dnsrace(args).await;
        assert_eq!(out.code, 2, "args {args:?}: {}", out.stderr);
        assert!(out.stderr.starts_with("dnsrace: "), "{}", out.stderr);
        assert!(out.stdout.is_empty(), "{}", out.stdout);
    }

    let missing = run_dnsrace(&["--servers", "/definitely/not/here.txt"]).await;
    assert_eq!(missing.code, 1);
    assert!(
        missing.stderr.contains("cannot read the server list"),
        "{}",
        missing.stderr
    );
}

#[tokio::test]
async fn exits_with_an_error_when_nothing_answers() {
    let port = closed_udp_port().await;
    let list = TempFile::with(&format!("127.0.0.1:{port} Nobody\n"));
    let out = run_dnsrace(&["--servers", &list.arg(), "--no-detect"]).await;
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("no server answered"), "{}", out.stderr);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quiet_prints_only_the_recommended_address() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 20))
        .await
        .expect("bind");
    let slow = spawn_mock("127.0.0.1:0", Behavior::new(80, 100))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n{} Slow\n", fast.addr, slow.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--quiet",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.stdout.trim(), fast.addr.to_string());
    assert_eq!(out.stdout.lines().count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn csv_has_a_header_and_one_row_per_server() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 20))
        .await
        .expect("bind");
    let slow = spawn_mock("127.0.0.1:0", Behavior::new(80, 100))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n{} Slow\n", fast.addr, slow.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--csv",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[0],
        "rank,server,name,cached_ms,uncached_ms,min_ms,p95_ms,jitter_ms,timeouts,flag,dnssec,current"
    );
    assert!(lines[1].contains(&fast.addr.to_string()), "{}", lines[1]);
    assert!(lines[1].contains(",Fast,"), "{}", lines[1]);
    assert!(out.stderr.contains("Stage 1:"), "{}", out.stderr);
    assert!(!out.stdout.contains("Stage 1:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn address_family_filters_only_scan_that_family() {
    let Some(v6) = spawn_mock("[::1]:0", Behavior::new(3, 20)).await else {
        eprintln!("skipping: no IPv6 loopback on this machine");
        return;
    };
    let v4 = spawn_mock("127.0.0.1:0", Behavior::new(2, 20))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} V6\n{} V4\n", v6.addr, v4.addr));

    let out4 = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--ipv4",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out4.code, 0, "stderr: {}", out4.stderr);
    assert!(out4.stdout.contains("quick scan of 1 candidate"));
    assert!(out4.stdout.contains("1 answered."));
    assert!(!out4.stdout.contains(&v6.addr.to_string()));

    let out6 = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--ipv6",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out6.code, 0, "stderr: {}", out6.stderr);
    assert!(out6.stdout.contains("quick scan of 1 candidate"));
    assert!(out6.stdout.contains(&v6.addr.to_string()));
    assert!(!out6.stdout.contains(&v4.addr.to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verbose_shows_spread_and_dnssec() {
    let fast = spawn_mock(
        "127.0.0.1:0",
        Behavior {
            dnssec: true,
            ..Behavior::new(2, 20)
        },
    )
    .await
    .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n", fast.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--verbose",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("== Details:"), "{}", out.stdout);
    assert!(out.stdout.contains("jitter"), "{}", out.stdout);
    assert!(out.stdout.contains("DNSSEC yes"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("DNSSEC validation:      1 of 1 healthy servers set the AD bit"),
        "{}",
        out.stdout
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeat_runs_more_rounds_and_still_ranks() {
    let fast = spawn_mock("127.0.0.1:0", Behavior::new(2, 20))
        .await
        .expect("bind");
    let slow = spawn_mock("127.0.0.1:0", Behavior::new(80, 100))
        .await
        .expect("bind");
    let list = TempFile::with(&format!("{} Fast\n{} Slow\n", fast.addr, slow.addr));

    let out = run_dnsrace(&[
        "--servers",
        &list.arg(),
        "--no-detect",
        "--no-color",
        "--repeat",
        "2",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("2 rounds each"), "{}", out.stdout);
    assert!(position(&out.stdout, " Fast ") < position(&out.stdout, " Slow "));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn added_servers_are_fully_tested_despite_known_limit() {
    let a = spawn_mock("127.0.0.1:0", Behavior::new(2, 30))
        .await
        .expect("bind");
    let b = spawn_mock("127.0.0.1:0", Behavior::new(50, 60))
        .await
        .expect("bind");
    let c = spawn_mock("127.0.0.1:0", Behavior::new(200, 210))
        .await
        .expect("bind");
    let empty = TempFile::with("# no servers in this list\n");
    let added = format!("{},{},{}", a.addr, b.addr, c.addr);

    let out = run_dnsrace(&[
        "--servers",
        &empty.arg(),
        "--no-detect",
        "--no-color",
        "--add",
        &added,
        "--known",
        "2",
        "--timeout",
        "500",
    ])
    .await;
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("3 answered."), "{}", out.stdout);
    assert!(
        out.stdout.contains("full test of 3 servers"),
        "{}",
        out.stdout
    );
    assert!(line_with(&out.stdout, &a.addr.to_string()).contains(" custom "));
    // --add servers are always fully tested; --known does not take their slots
    assert!(out.stdout.contains(&b.addr.to_string()), "{}", out.stdout);
    assert!(out.stdout.contains(&c.addr.to_string()), "{}", out.stdout);
    assert!(
        position(&out.stdout, &a.addr.to_string()) < position(&out.stdout, &b.addr.to_string()),
        "{}",
        out.stdout
    );
}
