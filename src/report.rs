//! Report exports: JSON, CSV and a self-contained HTML page.
//!
//! The terminal table is gone in 0.6.0; these functions are what the graphical interface uses
//! for the Export buttons and for the clipboard summary. They deliberately accept only the
//! structured result, so they can be tested without any GUI code.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::bench::{Row, SecurityStatus, SteerEntry, Steering};
use crate::engine::RunResult;
use crate::util::format_time;

/// A time in ms, or an em dash when it could not be measured.
pub fn fmt_ms(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.1} ms"),
        None => "—".to_string(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn security(row: &Row) -> &'static str {
    row.security_status().label()
}

fn row_value(row: &Row, rank: usize) -> Value {
    json!({
        "rank": rank,
        "id": row.id,
        "provider": row.provider,
        "name": row.label,
        "transport": row.transport.label(),
        "server": row.endpoint,
        "addr": row.addr.map(|a| a.ip().to_string()),
        "port": row.addr.map(|a| a.port()),
        "is_doh": row.is_doh(),
        "encrypted": !row.is_plain(),
        "cached_ms": row.cached,
        "uncached_ms": row.uncached,
        "score_ms": row.score,
        "cached_min_ms": row.cached_min,
        "cached_p95_ms": row.cached_p95,
        "cached_p99_ms": row.cached_p99,
        "cached_mean_ms": row.cached_mean,
        "cached_max_ms": row.cached_max,
        "cached_jitter_ms": row.cached_jitter,
        "timeouts": row.timeouts,
        "note": row.note.label(),
        "healthy": row.healthy(),
        "recommendable": row.recommendable(),
        "dnssec_ad": row.dnssec,
        "dnssec_rejects_bogus": row.dnssec_bogus,
        "security": row.security_status().label(),
    })
}

fn steering_value(s: &Steering) -> Value {
    json!({
        "host": s.host,
        "servers": s.servers,
        "addresses": s.entries.iter().map(|e| {
            json!({
                "ip": e.ip.to_string(),
                "returned_by": e.returned_by,
                "connects_ms": e.connects,
                "median_connect_ms": median_connect(e),
                "failed": e.connects.iter().filter(|v| v.is_none()).count(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn median_connect(entry: &SteerEntry) -> Option<f64> {
    let mut values: Vec<f64> = entry.connects.iter().flatten().copied().collect();
    crate::util::median(&mut values)
}

fn recommendation(result: &RunResult) -> Option<Value> {
    let row = result.best()?;
    Some(json!({
        "provider": row.provider,
        "name": row.label,
        "transport": row.transport.label(),
        "server": row.endpoint,
        "score_ms": row.score,
        "security": row.security_status().label(),
        "applies_to_system": row.is_plain() && row.plain_ip().is_some(),
    }))
}

/// A complete machine-readable report. The existing top-level keys stay stable for scripts.
pub fn json(result: &RunResult) -> String {
    let rows: Vec<Value> = result
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| row_value(row, index + 1))
        .collect();
    let steerings: Vec<Value> = result.steerings.iter().map(steering_value).collect();
    let value = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "generated": now(),
        "generated_utc": format_time(now()),
        "protocol": result.protocol.label(),
        "family": result.family.label(),
        "duration_seconds": result.duration_secs,
        "scanned": result.scanned,
        "tested": result.tested,
        "data_center": result.data_center,
        "current_dns": result.current_dns.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "recommendation": recommendation(result),
        "steering": steerings.first().cloned(),
        "steerings": steerings,
        "results": rows,
    });
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_num(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3}")).unwrap_or_default()
}

fn csv_opt_bool(v: Option<bool>) -> String {
    match v {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => String::new(),
    }
}

/// A comma-separated report with every field the interface shows.
pub fn csv(result: &RunResult) -> String {
    let mut out = String::from(
        "rank,provider,name,transport,server,addr,port,is_doh,encrypted,\
cached_ms,uncached_ms,score_ms,cached_min_ms,cached_p95_ms,cached_p99_ms,cached_mean_ms,\
cached_max_ms,cached_jitter_ms,timeouts,note,healthy,recommendable,dnssec_ad,\
dnssec_rejects_bogus,security\n",
    );
    for (index, row) in result.rows.iter().enumerate() {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            index + 1,
            csv_field(&row.provider),
            csv_field(&row.label),
            row.transport.label(),
            csv_field(&row.endpoint),
            row.addr.map(|a| a.ip().to_string()).unwrap_or_default(),
            row.addr.map(|a| a.port().to_string()).unwrap_or_default(),
            row.is_doh(),
            !row.is_plain(),
            csv_num(row.cached),
            csv_num(row.uncached),
            csv_num(Some(row.score)),
            csv_num(row.cached_min),
            csv_num(row.cached_p95),
            csv_num(row.cached_p99),
            csv_num(row.cached_mean),
            csv_num(row.cached_max),
            csv_num(row.cached_jitter),
            row.timeouts,
            csv_field(row.note.label()),
            row.healthy(),
            row.recommendable(),
            csv_opt_bool(row.dnssec),
            csv_opt_bool(row.dnssec_bogus),
            csv_field(row.security_status().label()),
        ));
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn security_class(row: &Row) -> &'static str {
    match row.security_status() {
        SecurityStatus::Validated => "ok",
        SecurityStatus::Insecure | SecurityStatus::Partial | SecurityStatus::Unknown => "warn",
        SecurityStatus::BogusAccepted => "bad",
    }
}

fn steering_html(s: &Steering) -> String {
    if s.entries.is_empty() {
        return format!(
            "<section class=\"card\"><h3>{}</h3><p class=\"muted\">No addresses were returned by the {} tested servers.</p></section>",
            escape(&s.host),
            s.servers
        );
    }
    let mut rows = String::new();
    for entry in &s.entries {
        let connects: Vec<String> = entry
            .connects
            .iter()
            .map(|v| {
                v.map(|x| format!("{x:.1} ms"))
                    .unwrap_or_else(|| "failed".into())
            })
            .collect();
        rows.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{} of {}</td><td>{}</td><td>{}</td></tr>",
            escape(&entry.ip.to_string()),
            entry.returned_by,
            s.servers,
            connects.join(" · "),
            fmt_ms(median_connect(entry))
        ));
    }
    format!(
        "<section class=\"card\"><h3>{}</h3><p class=\"muted\">Addresses handed out by the {} fastest plain-DNS servers, and TCP connect time to each.</p><table><thead><tr><th>Address</th><th>Returned by</th><th>Connect attempts</th><th>Median</th></tr></thead><tbody>{}</tbody></table></section>",
        escape(&s.host),
        s.servers,
        rows
    )
}

/// A standalone HTML report with the same information as the desktop results table.
pub fn html(result: &RunResult) -> String {
    let generated = format_time(now());
    let recommendation = result
        .best()
        .map(|row| {
            format!(
                "<div class=\"winner\"><div class=\"eyebrow\">Recommended</div><div class=\"winner-name\">{} <span class=\"muted\">{}</span></div><div class=\"winner-time\">{}</div><div class=\"badge {}\">{}</div></div>",
                escape(&row.label),
                escape(&row.endpoint),
                fmt_ms(Some(row.score)),
                security_class(row),
                escape(security(row)),
            )
        })
        .unwrap_or_else(|| "<div class=\"winner\"><div class=\"winner-name\">No recommendation</div><p class=\"muted\">No resolver completed enough measurements.</p></div>".to_string());

    let mut rows = String::new();
    for (index, row) in result.rows.iter().enumerate() {
        let status = if row.healthy() { "ok" } else { "bad" };
        rows.push_str(&format!(
            "<tr><td>{}</td><td><strong>{}</strong><br><span class=\"muted\">{}</span></td><td><code>{}</code></td><td>{}</td><td>{}</td><td class=\"score\">{}</td><td>{}</td><td><span class=\"badge {}\">{}</span></td><td><span class=\"badge {}\">{}</span></td></tr>",
            index + 1,
            escape(&row.label),
            escape(&row.provider),
            escape(&row.endpoint),
            fmt_ms(row.cached),
            fmt_ms(row.uncached),
            fmt_ms(Some(row.score)),
            escape(row.note.label()),
            security_class(row),
            escape(security(row)),
            status,
            if row.healthy() { "OK" } else { "Check" },
        ));
    }
    let steering = result
        .steerings
        .iter()
        .map(steering_html)
        .collect::<Vec<_>>()
        .join("\n");
    let current = if result.current_dns.is_empty() {
        "not detected".to_string()
    } else {
        result
            .current_dns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>dnsrace report</title>
<style>
:root{{color-scheme:dark;--bg:#111419;--panel:#191e26;--line:#2a313d;--text:#e8edf5;--muted:#8c98a9;--accent:#69a7ff;--ok:#4ed6a1;--warn:#f1c56b;--bad:#ff7b7b}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--text);font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}}
main{{max-width:1180px;margin:0 auto;padding:40px 24px 80px}}h1{{font-size:34px;margin:0 0 4px}}h2{{font-size:20px;margin:32px 0 12px}}h3{{margin:0 0 8px}}p{{margin:8px 0}}
.muted{{color:var(--muted)}}.eyebrow{{text-transform:uppercase;letter-spacing:.12em;font-size:11px;color:var(--muted)}}
.sub{{color:var(--muted);margin-bottom:24px}}.cards{{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:12px;margin:24px 0}}
.card{{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:18px}}.card strong{{display:block;font-size:18px}}
.winner{{background:linear-gradient(135deg,#1c2a42,#17251f);border:1px solid #33537a;border-radius:16px;padding:24px;margin:18px 0;display:grid;grid-template-columns:1fr auto;gap:8px}}
.winner-name{{font-size:24px;font-weight:700}}.winner-time{{font-size:32px;font-weight:700;grid-column:2;grid-row:1/4;align-self:center;color:var(--ok)}}
table{{width:100%;border-collapse:collapse;background:var(--panel);border:1px solid var(--line);border-radius:12px;overflow:hidden}}
th,td{{padding:10px 12px;text-align:left;border-bottom:1px solid var(--line);vertical-align:top}}th{{font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--muted)}}
tr:last-child td{{border-bottom:0}}.score{{font-weight:700;color:var(--accent)}}code{{font-size:12px;color:#cbd6e7}}
.badge{{display:inline-block;border-radius:999px;padding:2px 8px;font-size:11px;font-weight:600;border:1px solid}} .badge.ok{{color:var(--ok);border-color:#2e7259;background:#132a22}} .badge.warn{{color:var(--warn);border-color:#725d2e;background:#2a2413}} .badge.bad{{color:var(--bad);border-color:#723232;background:#2a1313}}
section.card{{margin:12px 0 18px}}footer{{margin-top:36px;color:var(--muted);font-size:12px}}
</style>
</head>
<body><main>
<div class="eyebrow">dnsrace {version} · {generated}</div>
<h1>DNS benchmark report</h1>
<p class="sub">{protocol} · {family} · {tested} tested in {duration:.1} s</p>
<div class="cards">
<div class="card"><span class="eyebrow">Current DNS</span><strong>{current}</strong></div>
<div class="card"><span class="eyebrow">Cloudflare edge</span><strong>{dc}</strong></div>
<div class="card"><span class="eyebrow">Candidates scanned</span><strong>{scanned}</strong></div>
<div class="card"><span class="eyebrow">Full tests</span><strong>{tested}</strong></div>
</div>
{recommendation}
<h2>All results</h2>
<table><thead><tr><th>#</th><th>Provider</th><th>Server</th><th>Cached</th><th>Uncached</th><th>Score</th><th>Status</th><th>DNSSEC</th><th>Health</th></tr></thead><tbody>{rows}</tbody></table>
<h2>Steering</h2>
{steering}
<footer>Generated locally by dnsrace. Latency depends on your connection and the time of day.</footer>
</main></body></html>"#,
        version = env!("CARGO_PKG_VERSION"),
        generated = escape(&generated),
        protocol = result.protocol.label(),
        family = result.family.label(),
        tested = result.tested,
        duration = result.duration_secs,
        current = escape(&current),
        dc = escape(result.data_center.as_deref().unwrap_or("not detected")),
        scanned = result.scanned,
        recommendation = recommendation,
        rows = rows,
        steering = steering,
    )
}

/// A short plain-text summary for the clipboard or a notification.
pub fn summary_text(result: &RunResult) -> String {
    let mut out = format!(
        "dnsrace {} — {} — {}\n",
        env!("CARGO_PKG_VERSION"),
        result.protocol.label(),
        format_time(now())
    );
    if let Some(row) = result.best() {
        out.push_str(&format!(
            "Fastest: {} ({}) — {}\n",
            row.label,
            row.endpoint,
            fmt_ms(Some(row.score))
        ));
        out.push_str(&format!("Security: {}\n", row.security_status().label()));
        if let Some(ip) = row.plain_ip() {
            out.push_str(&format!("Address: {}\n", ip));
        }
    } else {
        out.push_str("No recommendable resolver completed the run.\n");
    }
    out.push_str(&format!(
        "Protocol: {} · Tested: {} · Duration: {:.1} s\n",
        result.protocol.label(),
        result.tested,
        result.duration_secs
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Note;
    use crate::servers::{Family, Protocol};

    fn sample_result() -> RunResult {
        let row = Row {
            id: "plain:1.1.1.1:53".into(),
            provider: "Cloudflare".into(),
            label: "Cloudflare".into(),
            transport: Protocol::Udp,
            addr: Some("1.1.1.1:53".parse().unwrap()),
            endpoint: "1.1.1.1".into(),
            cached: Some(12.34),
            uncached: Some(23.45),
            cached_min: Some(10.0),
            cached_p95: Some(20.0),
            cached_p99: Some(22.0),
            cached_mean: Some(14.0),
            cached_max: Some(30.0),
            cached_jitter: Some(2.5),
            dnssec: Some(true),
            dnssec_bogus: Some(true),
            timeouts: 0,
            note: Note::Ok,
            score: 17.895,
        };
        RunResult {
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 12.5,
            data_center: Some("MRS".into()),
            current_dns: vec!["192.168.1.1".parse().unwrap()],
            rows: vec![row],
            steerings: vec![Steering {
                host: "example.com".into(),
                servers: 1,
                entries: vec![SteerEntry {
                    ip: "93.184.216.34".parse().unwrap(),
                    returned_by: 1,
                    connects: vec![Some(12.0), None, Some(14.0)],
                }],
            }],
            scanned: 1,
            tested: 1,
        }
    }

    #[test]
    fn json_has_rank_recommendation_and_security() {
        let text = json(&sample_result());
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["results"][0]["rank"], 1);
        assert_eq!(value["results"][0]["security"], "DNSSEC validated");
        assert_eq!(value["recommendation"]["name"], "Cloudflare");
        assert_eq!(value["steering"]["addresses"][0]["returned_by"], 1);
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        let mut result = sample_result();
        result.rows[0].label = "A, \"quoted\" resolver".into();
        let text = csv(&result);
        assert!(text.starts_with("rank,provider,name"));
        assert!(text.contains("\"A, \"\"quoted\"\" resolver\""));
        assert!(text.contains("17.895"));
    }

    #[test]
    fn html_escapes_every_user_visible_value() {
        let mut result = sample_result();
        result.rows[0].label = "<script>alert(1)</script>".into();
        result.rows[0].endpoint = "1.1.1.1&x".into();
        let text = html(&result);
        assert!(!text.contains("<script>alert(1)</script>"));
        assert!(text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(text.contains("1.1.1.1&amp;x"));
        assert!(text.contains("DNSSEC validated"));
    }

    #[test]
    fn summary_mentions_the_winner_and_security() {
        let text = summary_text(&sample_result());
        assert!(text.contains("Fastest: Cloudflare"));
        assert!(text.contains("DNSSEC validated"));
        assert!(text.contains("Address: 1.1.1.1"));
    }

    #[test]
    fn empty_results_are_handled_without_panicking() {
        let mut result = sample_result();
        result.rows.clear();
        assert!(json(&result).contains("\"recommendation\": null"));
        assert!(csv(&result).lines().count() == 1);
        assert!(html(&result).contains("No recommendation"));
        assert!(summary_text(&result).contains("No recommendable"));
    }

    #[test]
    fn ms_formatting() {
        assert_eq!(fmt_ms(Some(12.34)), "12.3 ms");
        assert_eq!(fmt_ms(None), "—");
    }
}
