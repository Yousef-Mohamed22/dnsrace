//! End-to-end tests: run the real binary against simulated DNS servers on loopback.

mod common;

use common::{closed_udp_port, line_with, position, run_dnsrace, spawn_mock, Behavior, TempFile};
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
        text.contains(&format!("Fastest overall:        {} (Fast)", fast.addr)),
        "{text}"
    );
    assert!(
        text.contains(&format!("Fastest known provider: {} (Fast)", fast.addr)),
        "{text}"
    );

    // the steering check: both healthy servers return 127.0.0.1, and we can connect to it
    assert!(text.contains("== steer.test:"), "{text}");
    let steer = line_with(text, "returned by");
    assert!(steer.starts_with("127.0.0.1"), "{steer}");
    assert!(
        steer.contains(" ms") && !steer.contains("FAILED"),
        "{steer}"
    );
    assert!(steer.contains("returned by 2 of 2 servers"), "{steer}");
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
    assert!(results[0]["cached_ms"].as_f64().unwrap() < results[1]["cached_ms"].as_f64().unwrap());
    assert_eq!(v["fastest"], fast.addr.to_string());
    assert_eq!(v["fastest_known"], fast.addr.to_string());
    assert!(v["steering"].is_null());

    // progress text stays out of the JSON stream
    assert!(out.stderr.contains("Stage 1:"), "{}", out.stderr);
    assert!(!out.stdout.contains("Stage 1:"));
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
async fn add_and_known_options() {
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
        out.stdout.contains("full test of 2 servers"),
        "{}",
        out.stdout
    );
    assert!(line_with(&out.stdout, &a.addr.to_string()).contains(" custom "));
    // only the two fastest got the full test
    assert!(!out.stdout.contains(&c.addr.to_string()), "{}", out.stdout);
}
