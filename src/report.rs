//! Turns results into text and JSON. Everything here returns strings, so it is easy to test.

use std::collections::HashSet;
use std::net::IpAddr;

use crate::bench::{Note, Row, SteerEntry, Steering};
use crate::servers::{fmt_addr, is_known};
use crate::util::median;

/// A time in ms, or `n/a` when it could not be measured.
pub fn fmt_ms(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:>6.1} ms"),
        None => "      n/a".to_string(),
    }
}

/// Wraps text in an ANSI color code when `on`.
pub fn paint(s: &str, code: &str, on: bool) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn is_current(row: &Row, current: &HashSet<IpAddr>) -> bool {
    row.addr.port() == 53 && current.contains(&row.addr.ip())
}

/// The ranked table, one line per server.
pub fn table(rows: &[Row], current: &HashSet<IpAddr>, color: bool) -> String {
    let addr_w = rows
        .iter()
        .map(|r| fmt_addr(r.addr).len())
        .max()
        .unwrap_or(0)
        .max(15);
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(0).max(8);
    let mut lines = vec![
        "== Results: average of the cached and uncached medians, lower is better ==".to_string(),
    ];
    for (i, r) in rows.iter().enumerate() {
        let flag = match r.note {
            Note::Ok => String::new(),
            Note::Hijacks => paint("  HIJACKS-NXDOMAIN", "31", color),
            Note::Unreliable => paint("  UNRELIABLE", "31", color),
        };
        let marker = if is_current(r, current) {
            paint("  <== current", "33", color)
        } else {
            String::new()
        };
        lines.push(format!(
            "{:>2}. {:<addr_w$} {:<name_w$} cached {}  uncached {}  timeouts {}{}{}",
            i + 1,
            fmt_addr(r.addr),
            r.name,
            fmt_ms(r.cached),
            fmt_ms(r.uncached),
            r.timeouts,
            flag,
            marker,
        ));
    }
    lines.join("\n")
}

/// A more detailed table: how steady each server's cached lookups were, and whether it
/// validates DNSSEC. Shown with `--verbose`.
pub fn details(rows: &[Row]) -> String {
    let addr_w = rows
        .iter()
        .map(|r| fmt_addr(r.addr).len())
        .max()
        .unwrap_or(0)
        .max(15);
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(0).max(8);
    let mut lines = vec![
        "== Details: cached-lookup spread, and whether the server sets the DNSSEC AD bit =="
            .to_string(),
    ];
    for (i, r) in rows.iter().enumerate() {
        let dnssec = match r.dnssec {
            Some(true) => "yes",
            Some(false) => "no",
            None => "?",
        };
        lines.push(format!(
            "{:>2}. {:<addr_w$} {:<name_w$} min {}  p95 {}  jitter {}  DNSSEC {}",
            i + 1,
            fmt_addr(r.addr),
            r.name,
            fmt_ms(r.cached_min),
            fmt_ms(r.cached_p95),
            fmt_ms(r.cached_jitter),
            dnssec,
        ));
    }
    lines.join("\n")
}

fn label(row: Option<&Row>) -> String {
    row.map(|r| format!("{} ({})", fmt_addr(r.addr), r.name))
        .unwrap_or_else(|| "-".to_string())
}

fn fastest(rows: &[Row]) -> Option<&Row> {
    rows.iter().find(|r| r.healthy())
}

fn fastest_known(rows: &[Row]) -> Option<&Row> {
    rows.iter().find(|r| r.healthy() && is_known(&r.name))
}

/// The server we suggest: the fastest healthy provider from the built-in list, or the fastest
/// healthy server of any kind when no listed provider works.
pub fn recommended(rows: &[Row]) -> Option<&Row> {
    fastest_known(rows).or_else(|| fastest(rows))
}

fn average(row: &Row) -> f64 {
    row.score
}

/// The summary: a clear recommendation, where your current DNS ranks, and how much switching
/// would actually gain you.
pub fn summary(rows: &[Row], current: &HashSet<IpAddr>, color: bool, repeat: usize) -> String {
    let rec = recommended(rows);
    let fastest = fastest(rows);
    let mut lines = Vec::new();

    match rec {
        Some(r) => lines.push(paint(
            &format!(
                "Recommended:            {} - {:.1} ms average",
                label(Some(r)),
                average(r)
            ),
            "1;32",
            color,
        )),
        None => lines.push(paint(
            "Recommended:            none (no healthy server was measured)",
            "1;31",
            color,
        )),
    }
    if let (Some(f), Some(r)) = (fastest, rec)
        && f.addr != r.addr
    {
        lines.push(format!(
            "Fastest overall:        {} - {:.1} ms average",
            label(Some(f)),
            average(f)
        ));
    }

    let mine: Vec<(usize, &Row)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| is_current(r, current))
        .collect();
    if !mine.is_empty() {
        let ranks = mine
            .iter()
            .map(|(i, r)| format!("#{} ({})", i + 1, fmt_addr(r.addr)))
            .collect::<Vec<_>>()
            .join(", ");
        let best = mine
            .iter()
            .map(|(_, r)| *r)
            .min_by(|a, b| a.score.total_cmp(&b.score));
        let detail = match (best, rec) {
            (Some(b), Some(r)) if b.addr == r.addr => {
                " - already the recommended choice".to_string()
            }
            (Some(b), Some(r)) if average(b) <= average(r) => {
                format!(
                    " - {:.1} ms faster per lookup than the recommendation",
                    average(r) - average(b)
                )
            }
            (Some(b), Some(r)) => {
                format!(
                    " - {:.1} ms slower per lookup than the recommendation",
                    average(b) - average(r)
                )
            }
            _ => String::new(),
        };
        lines.push(format!(
            "Your current DNS:       {} of {} tested{}",
            ranks,
            rows.len(),
            detail
        ));
    }

    let checked: Vec<&Row> = rows
        .iter()
        .filter(|r| r.healthy() && r.dnssec.is_some())
        .collect();
    if !checked.is_empty() {
        let yes = checked.iter().filter(|r| r.dnssec == Some(true)).count();
        lines.push(format!(
            "DNSSEC validation:      {yes} of {} healthy servers set the AD bit",
            checked.len()
        ));
    }

    let mut seen_providers: HashSet<&str> = HashSet::new();
    let known: Vec<&Row> = rows
        .iter()
        .filter(|r| r.healthy() && is_known(&r.name) && seen_providers.insert(r.name.as_str()))
        .collect();
    if known.len() >= 2 {
        let gap = known[1].score - known[0].score;
        if gap < 1.5 {
            let hint = if repeat < 3 {
                "use --repeat 3 for a steadier winner"
            } else {
                "run again at a different time to confirm"
            };
            lines.push(format!(
                "Close call:             the top two providers are within {gap:.1} ms; {hint}"
            ));
        }
    }

    let current_best = mine
        .iter()
        .map(|(_, r)| *r)
        .min_by(|a, b| a.score.total_cmp(&b.score));
    if let (Some(r), Some(b)) = (rec, current_best) {
        let gap = average(b) - average(r);
        let (text, code) = if b.addr == r.addr {
            (
                "your current DNS is already the recommended server; no change needed.".to_string(),
                "1;32",
            )
        } else if gap.abs() <= 1.0 {
            (
                "the difference is under 1 ms per lookup; either choice is fine.".to_string(),
                "1;33",
            )
        } else if gap < 0.0 {
            (
                format!(
                    "your current DNS is about {:.1} ms faster per lookup than the recommendation; no change needed for speed.",
                    -gap
                ),
                "1;33",
            )
        } else {
            (
                format!(
                    "switching to {} would save about {gap:.1} ms per DNS lookup.",
                    fmt_addr(r.addr)
                ),
                "1;32",
            )
        };
        lines.push(paint(
            &format!("Verdict:                {text}"),
            code,
            color,
        ));
    } else if let Some(r) = rec {
        lines.push(paint(
            &format!(
                "Verdict:                use {} ({}); your current DNS was not detected.",
                fmt_addr(r.addr),
                r.name
            ),
            "1;32",
            color,
        ));
    }

    lines.join("\n")
}

/// The median connect time of an entry and how many attempts failed.
fn connect_summary(e: &SteerEntry) -> (Option<f64>, usize) {
    let mut ok: Vec<f64> = e.connects.iter().flatten().copied().collect();
    let failed = e.connects.len() - ok.len();
    (median(&mut ok), failed)
}

fn connect_text(e: &SteerEntry) -> String {
    match connect_summary(e) {
        (Some(m), 0) => format!("{m:.0} ms"),
        (Some(m), failed) => format!("{m:.0} ms ({failed} of {} failed)", e.connects.len()),
        (None, _) => "FAILED".to_string(),
    }
}

/// The "where does this site go" section.
pub fn steering_text(s: &Steering) -> String {
    let mut lines = vec![format!(
        "== {}: addresses handed out by the {} fastest servers, and TCP connect time to each ==",
        s.host, s.servers
    )];
    if s.entries.is_empty() {
        lines.push(format!("no server returned an address for {}", s.host));
        return lines.join("\n");
    }
    let w = s
        .entries
        .iter()
        .map(|e| e.ip.to_string().len())
        .max()
        .unwrap_or(0)
        .max(15);
    for e in &s.entries {
        lines.push(format!(
            "{:<w$} connect {:<26} returned by {} of {} servers",
            e.ip.to_string(),
            connect_text(e),
            e.returned_by,
            s.servers
        ));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------------------

fn csv_field(s: &str) -> String {
    if s.chars().any(|c| matches!(c, ',' | '"' | '\n' | '\r')) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_num(v: Option<f64>) -> String {
    v.filter(|x| x.is_finite())
        .map(|x| format!("{x:.1}"))
        .unwrap_or_default()
}

fn flag_name(note: Note) -> &'static str {
    match note {
        Note::Ok => "",
        Note::Hijacks => "HIJACKS-NXDOMAIN",
        Note::Unreliable => "UNRELIABLE",
    }
}

/// The results as CSV, one row per server, ready for a spreadsheet.
pub fn csv(rows: &[Row], current: &HashSet<IpAddr>) -> String {
    let mut lines = vec![
        "rank,server,name,cached_ms,uncached_ms,min_ms,p95_ms,jitter_ms,timeouts,flag,dnssec,current"
            .to_string(),
    ];
    for (i, r) in rows.iter().enumerate() {
        let dnssec = match r.dnssec {
            Some(true) => "yes",
            Some(false) => "no",
            None => "",
        };
        lines.push(format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            i + 1,
            csv_field(&fmt_addr(r.addr)),
            csv_field(&r.name),
            csv_num(r.cached),
            csv_num(r.uncached),
            csv_num(r.cached_min),
            csv_num(r.cached_p95),
            csv_num(r.cached_jitter),
            r.timeouts,
            flag_name(r.note),
            dnssec,
            is_current(r, current),
        ));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------------------
// JSON (written by hand: the output is small and flat, so no extra dependency is needed)
// ---------------------------------------------------------------------------------------

fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn jnum(v: Option<f64>) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x:.1}"),
        _ => "null".to_string(),
    }
}

fn jopt(s: Option<String>) -> String {
    s.map_or_else(|| "null".to_string(), |s| jstr(&s))
}

fn jbool(v: Option<bool>) -> String {
    match v {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => "null".to_string(),
    }
}

/// The whole result as one JSON document.
pub fn json(
    version: &str,
    data_center: Option<&str>,
    rows: &[Row],
    current: &HashSet<IpAddr>,
    steering: Option<&Steering>,
) -> String {
    let results: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "    {{\"rank\": {}, \"server\": {}, \"name\": {}, \"cached_ms\": {}, \"uncached_ms\": {}, \"min_ms\": {}, \"p95_ms\": {}, \"jitter_ms\": {}, \"timeouts\": {}, \"flag\": {}, \"dnssec\": {}, \"current\": {}}}",
                i + 1,
                jstr(&fmt_addr(r.addr)),
                jstr(&r.name),
                jnum(r.cached),
                jnum(r.uncached),
                jnum(r.cached_min),
                jnum(r.cached_p95),
                jnum(r.cached_jitter),
                r.timeouts,
                jopt(match r.note {
                    Note::Ok => None,
                    Note::Hijacks => Some("HIJACKS-NXDOMAIN".to_string()),
                    Note::Unreliable => Some("UNRELIABLE".to_string()),
                }),
                jbool(r.dnssec),
                is_current(r, current),
            )
        })
        .collect();
    let steer = match steering {
        None => "null".to_string(),
        Some(s) => {
            let entries: Vec<String> = s
                .entries
                .iter()
                .map(|e| {
                    let (median_ms, failed) = connect_summary(e);
                    format!(
                        "      {{\"ip\": {}, \"returned_by\": {}, \"connect_ms\": {}, \"failed\": {}}}",
                        jstr(&e.ip.to_string()),
                        e.returned_by,
                        jnum(median_ms),
                        failed
                    )
                })
                .collect();
            format!(
                "{{\n    \"host\": {},\n    \"servers\": {},\n    \"addresses\": [\n{}\n    ]\n  }}",
                jstr(&s.host),
                s.servers,
                entries.join(",\n")
            )
        }
    };
    format!(
        "{{\n  \"dnsrace\": {},\n  \"data_center\": {},\n  \"results\": [\n{}\n  ],\n  \"recommended\": {},\n  \"fastest\": {},\n  \"fastest_known\": {},\n  \"steering\": {}\n}}",
        jstr(version),
        jopt(data_center.map(str::to_string)),
        results.join(",\n"),
        jopt(recommended(rows).map(|r| fmt_addr(r.addr))),
        jopt(fastest(rows).map(|r| fmt_addr(r.addr))),
        jopt(fastest_known(rows).map(|r| fmt_addr(r.addr))),
        steer
    )
}

// ---------------------------------------------------------------------------------------
// HTML report
// ---------------------------------------------------------------------------------------

/// The whole result as a self-contained HTML report.
pub fn html(
    version: &str,
    data_center: Option<&str>,
    rows: &[Row],
    current: &HashSet<IpAddr>,
    _steering: Option<&Steering>,
) -> String {
    let rec = recommended(rows);
    let fastest = fastest(rows);
    let now = time_stamp();

    let rows_html: String = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let flag_class = match r.note {
                Note::Ok => "ok",
                Note::Hijacks => "flag hijacks",
                Note::Unreliable => "flag unreliable",
            };
            let current_mark = if is_current(r, current) {
                " <span class=\"current\">&lt;== current</span>"
            } else {
                ""
            };
            let dnssec = match r.dnssec {
                Some(true) => "yes",
                Some(false) => "no",
                None => "?",
            };
            let doh_mark = if r.is_doh { " (DoH)" } else { "" };
            format!(
                "<tr>\n<td>{}</td>\n<td>{}{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td>{}</td>\n<td class=\"{}\">{}</td>\n</tr>",
                i + 1,
                fmt_addr(r.addr),
                doh_mark,
                current_mark,
                r.name,
                fmt_ms(r.cached),
                fmt_ms(r.cached_mean),
                fmt_ms(r.cached_min),
                fmt_ms(r.cached_max),
                fmt_ms(r.cached_p95),
                fmt_ms(r.cached_p99),
                fmt_ms(r.cached_jitter),
                r.timeouts,
                dnssec,
                flag_class,
                match r.note {
                    Note::Ok => "",
                    Note::Hijacks => "HIJACKS-NXDOMAIN",
                    Note::Unreliable => "UNRELIABLE",
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let rec_label = rec.map(|r| format!("{} ({})", fmt_addr(r.addr), r.name));
    let fastest_label = fastest.map(|r| format!("{} ({})", fmt_addr(r.addr), r.name));

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>dnsrace {version} results</title>
<style>
  body {{ font-family: system-ui, -apple-system, Segoe UI, Roboto, sans-serif; margin: 2rem; background: #f5f5f5; }}
  h1 {{ margin-top: 0; }}
  table {{ border-collapse: collapse; width: 100%; background: white; }}
  th, td {{ border: 1px solid #ccc; padding: 6px 10px; text-align: left; font-size: 14px; }}
  th {{ background: #e8e8e8; }}
  tr:nth-child(even) td {{ background: #fafafa; }}
  .flag.hijacks {{ color: #b00020; font-weight: bold; }}
  .flag.unreliable {{ color: #b00020; font-weight: bold; }}
  .current {{ color: #b8860b; font-weight: bold; }}
  .summary {{ max-width: 700px; margin: 2rem 0; font-size: 15px; line-height: 1.6; }}
  .verdict-ok {{ color: #008000; font-weight: bold; }}
  .verdict-warn {{ color: #b8860b; font-weight: bold; }}
  .verdict-stay {{ color: #b8860b; font-weight: bold; }}
  .muted {{ color: #666; }}
  .doh {{ font-size: 0.85em; color: #666; }}
</style>
</head>
<body>
<h1>dnsrace {version} results</h1>
<p class="muted">Generated {now}</p>
{potential_dc}
<h2>Results</h2>
<table>
<thead>
<tr><th>#</th><th>Server</th><th>Provider</th><th>Cached</th><th>Mean</th><th>Min</th><th>Max</th><th>P95</th><th>P99</th><th>Jitter</th><th>Timeouts</th><th>DNSSEC</th><th>Flag</th></tr>
</thead>
<tbody>
{rows_html}
</tbody>
</table>
<h2>Summary</h2>
<div class="summary">
<p><strong>Recommended:</strong> {rec}</p>
{fastest_p}
{verdict}
</div>
<p class="muted">DNS-over-HTTPS (DoH) servers are queried over HTTPS, not port 53. A DNS server sees every site you visit — only switch to one you trust.</p>
</body>
</html>"#,
        version = version,
        now = now,
        potential_dc = data_center
            .map(|dc| format!(
                "<p class=\"muted\"><strong>Cloudflare data center:</strong> {dc}</p>\n"
            ))
            .unwrap_or_default(),
        rec = rec_label.as_deref().unwrap_or("none"),
        fastest_p = fastest_label
            .as_ref()
            .map(|l| format!("<p><strong>Fastest overall:</strong> {l}</p>"))
            .unwrap_or_default(),
        verdict = verdict_html(rec, fastest, rows, current),
    )
}

/// A short verdict paragraph for the HTML report.
fn verdict_html(
    rec: Option<&Row>,
    fastest: Option<&Row>,
    rows: &[Row],
    current: &HashSet<IpAddr>,
) -> String {
    let mut lines = Vec::new();
    if let (Some(r), Some(f)) = (rec, fastest)
        && f.addr != r.addr
    {
        lines.push(format!(
            "<p><strong>Fastest overall:</strong> {} ({})</p>",
            fmt_addr(f.addr),
            f.name
        ));
    }
    let mine: Vec<&Row> = rows.iter().filter(|r| is_current(r, current)).collect();
    if let (Some(rec), Some(best_mine)) = (
        rec,
        mine.iter()
            .copied()
            .min_by(|a, b| a.score.total_cmp(&b.score)),
    ) {
        let gap = best_mine.score - rec.score;
        let verdict: String = if best_mine.addr == rec.addr {
            "Your current DNS is already the recommended server.".to_string()
        } else if gap.abs() <= 1.0 {
            "The difference is under 1 ms; either choice is fine.".to_string()
        } else if gap < 0.0 {
            format!(
                "Your current DNS is about {:.1} ms faster; no change needed for speed.",
                -gap
            )
        } else {
            format!(
                "Switching to {} would save about {:.1} ms per lookup.",
                fmt_addr(rec.addr),
                gap
            )
        };
        let class = if best_mine.addr == rec.addr || gap < 0.0 || gap.abs() <= 1.0 {
            "verdict-stay"
        } else {
            "verdict-ok"
        };
        lines.push(format!(
            "<p class=\"{}\"><strong>Verdict:</strong> {}</p>",
            class, verdict
        ));
    } else if let Some(r) = rec {
        lines.push(format!(
            "<p class=\"verdict-ok\"><strong>Verdict:</strong> Use {} ({}); your current DNS was not detected.</p>",
            fmt_addr(r.addr),
            r.name
        ));
    }
    lines.join("")
}

fn time_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            // Simple human-readable timestamp: YYYY-MM-DD HH:MM:SS UTC
            // Approximate calculation without external crates
            let mut remaining = secs;
            let sec = remaining % 60;
            remaining /= 60;
            let minute = remaining % 60;
            remaining /= 60;
            let hour = remaining % 24;
            let days_since_epoch = remaining / 24;
            // Approximate date from days since 1970-01-01
            let mut year = 1970;
            let mut days = days_since_epoch;
            loop {
                let days_in_year = if is_leap_year(year) { 366 } else { 365 };
                if days < days_in_year {
                    break;
                }
                days -= days_in_year;
                year += 1;
            }
            let month_lengths = [
                31,
                if is_leap_year(year) { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let mut month = 0;
            for &ml in &month_lengths {
                if days < ml {
                    break;
                }
                days -= ml;
                month += 1;
            }
            let day = days + 1;
            format!(
                "{year:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                month + 1,
                day,
                hour,
                minute,
                sec
            )
        })
        .unwrap_or_else(|_| "unknown time".to_string())
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        addr: &str,
        name: &str,
        cached: Option<f64>,
        uncached: Option<f64>,
        timeouts: u32,
        note: Note,
    ) -> Row {
        Row {
            addr: crate::servers::parse_addr(addr).unwrap(),
            name: name.to_string(),
            is_doh: false,
            cached,
            uncached,
            cached_min: cached.map(|x| x - 1.0),
            cached_max: cached.map(|x| x + 1.0),
            cached_p95: cached.map(|x| x + 2.0),
            cached_p99: cached.map(|x| x + 3.0),
            cached_mean: cached,
            cached_jitter: cached.map(|_| 1.5),
            dnssec: Some(true),
            timeouts,
            note,
            score: match (cached, uncached) {
                (Some(a), Some(b)) => (a + b) / 2.0,
                _ => 1.0e9,
            },
        }
    }

    fn sample() -> Vec<Row> {
        vec![
            row(
                "192.168.1.1",
                "Router/ISP-default",
                Some(5.04),
                Some(54.0),
                0,
                Note::Ok,
            ),
            row("1.1.1.1", "Cloudflare", Some(45.3), Some(66.0), 0, Note::Ok),
            row(
                "[2606:4700:4700::1111]:5353",
                "Cloudflare",
                Some(46.0),
                Some(67.0),
                1,
                Note::Ok,
            ),
            row("9.9.9.9", "Quad9", None, None, 8, Note::Unreliable),
            row("5.5.5.5", "other", Some(2.0), Some(3.0), 0, Note::Hijacks),
        ]
    }

    #[test]
    fn times_are_formatted_with_one_decimal() {
        assert_eq!(fmt_ms(Some(5.04)), "   5.0 ms");
        assert_eq!(fmt_ms(Some(123.456)), " 123.5 ms");
        assert_eq!(fmt_ms(None), "      n/a");
    }

    #[test]
    fn paint_only_adds_codes_when_asked() {
        assert_eq!(paint("x", "31", false), "x");
        assert_eq!(paint("x", "31", true), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn the_table_flags_and_marks_the_right_rows() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let text = table(&sample(), &current, false);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 6);
        assert!(lines[1].starts_with(" 1. 192.168.1.1"));
        assert!(lines[2].ends_with("<== current"));
        assert!(lines[3].contains("[2606:4700:4700::1111]:5353"));
        assert!(lines[4].contains("n/a") && lines[4].ends_with("UNRELIABLE"));
        assert!(lines[5].ends_with("HIJACKS-NXDOMAIN"));
        // the port-5353 IPv6 server is not "current" even though 1.1.1.1 is
        assert!(!lines[3].contains("current"));
    }

    #[test]
    fn details_show_spread_and_dnssec() {
        let text = details(&sample());
        assert!(text.contains("min   44.3 ms"));
        assert!(text.contains("p95   47.3 ms"));
        assert!(text.contains("jitter    1.5 ms"));
        assert!(text.contains("DNSSEC yes"));
        let mut no_dnssec = sample();
        no_dnssec[1].dnssec = Some(false);
        assert!(details(&no_dnssec).contains("DNSSEC no"));
    }

    #[test]
    fn the_summary_recommends_a_known_provider_and_shows_the_gap() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let text = summary(&sample(), &current, false, 1);
        assert!(text.contains("Recommended:            1.1.1.1 (Cloudflare) - 55.6 ms average"));
        assert!(text.contains("Fastest overall:        192.168.1.1 (Router/ISP-default)"));
        assert!(text.contains("Your current DNS:       #2 (1.1.1.1) of 5 tested"));
        assert!(text.contains("already the recommended choice"));
        assert!(text.contains("DNSSEC validation:      3 of 3 healthy servers set the AD bit"));
        assert!(text.contains("Verdict:                your current DNS is already"));
    }

    #[test]
    fn the_summary_explains_how_much_switching_gains() {
        let rows = vec![
            row("1.1.1.1", "Cloudflare", Some(20.0), Some(40.0), 0, Note::Ok),
            row("2.2.2.2", "Other", Some(50.0), Some(70.0), 0, Note::Ok),
        ];
        let current: HashSet<IpAddr> = ["2.2.2.2".parse().unwrap()].into_iter().collect();
        let text = summary(&rows, &current, false, 1);
        assert!(text.contains("30.0 ms slower per lookup"));
        assert!(text.contains("switching to 1.1.1.1 would save about 30.0 ms"));
        let none = summary(&[], &HashSet::new(), false, 1);
        assert!(none.contains("Recommended:            none"));
        assert!(!none.contains("Your current DNS"));
    }

    #[test]
    fn close_results_get_a_rerun_hint() {
        let rows = vec![
            row("1.1.1.1", "Cloudflare", Some(20.0), Some(40.0), 0, Note::Ok),
            row("8.8.8.8", "Google", Some(20.2), Some(40.2), 0, Note::Ok),
        ];
        let text = summary(&rows, &HashSet::new(), false, 1);
        assert!(text.contains("Close call:"));
        assert!(text.contains("use --repeat 3"));
        let settled = summary(&rows, &HashSet::new(), false, 3);
        assert!(settled.contains("run again at a different time"));
    }

    #[test]
    fn a_second_server_from_the_same_provider_is_not_a_close_call() {
        let rows = vec![
            row("1.1.1.1", "Cloudflare", Some(20.0), Some(40.0), 0, Note::Ok),
            row("1.0.0.1", "Cloudflare", Some(20.1), Some(40.1), 0, Note::Ok),
            row("8.8.8.8", "Google", Some(50.0), Some(70.0), 0, Note::Ok),
        ];
        assert!(!summary(&rows, &HashSet::new(), false, 1).contains("Close call:"));
    }

    fn steering_sample() -> Steering {
        Steering {
            host: "github.com".into(),
            servers: 10,
            entries: vec![
                SteerEntry {
                    ip: "185.199.108.133".parse().unwrap(),
                    returned_by: 10,
                    connects: vec![Some(172.0), Some(174.0), Some(171.0)],
                },
                SteerEntry {
                    ip: "185.199.111.133".parse().unwrap(),
                    returned_by: 9,
                    connects: vec![None, None, None],
                },
                SteerEntry {
                    ip: "2606:50c0:8000::154".parse().unwrap(),
                    returned_by: 2,
                    connects: vec![Some(20.0), None, Some(22.0)],
                },
            ],
        }
    }

    #[test]
    fn steering_shows_failures_and_intermittent_failures() {
        // columns are padded to line up, so compare with the spacing collapsed
        let text = steering_text(&steering_sample())
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("185.199.108.133 connect 172 ms"));
        assert!(text.contains("returned by 10 of 10 servers"));
        assert!(text.contains("185.199.111.133 connect FAILED"));
        assert!(text.contains("2606:50c0:8000::154 connect 21 ms (1 of 3 failed)"));
        let empty = Steering {
            host: "x.test".into(),
            servers: 3,
            entries: vec![],
        };
        assert!(steering_text(&empty).contains("no server returned an address for x.test"));
    }

    #[test]
    fn csv_has_a_header_and_quotes_awkward_names() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let mut rows = sample();
        rows[1].name = "Cloud,flare".into();
        let text = csv(&rows, &current);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(
            lines[0],
            "rank,server,name,cached_ms,uncached_ms,min_ms,p95_ms,jitter_ms,timeouts,flag,dnssec,current"
        );
        assert!(lines[2].contains("\"Cloud,flare\""));
        assert!(lines[2].contains(",yes,true"));
        assert!(lines[5].contains("HIJACKS-NXDOMAIN"));
    }

    #[test]
    fn json_escapes_strings() {
        assert_eq!(jstr("plain"), "\"plain\"");
        assert_eq!(jstr("a\"b\\c\nd\te\u{1}"), "\"a\\\"b\\\\c\\nd\\te\\u0001\"");
        assert_eq!(jnum(Some(1.26)), "1.3");
        assert_eq!(jnum(None), "null");
        assert_eq!(jnum(Some(f64::NAN)), "null");
        assert_eq!(jbool(Some(true)), "true");
        assert_eq!(jbool(None), "null");
    }

    #[test]
    fn json_is_shaped_as_documented() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let text = json(
            "0.5.0",
            Some("mrs04"),
            &sample(),
            &current,
            Some(&steering_sample()),
        );
        assert!(text.starts_with("{\n  \"dnsrace\": \"0.5.0\""));
        assert!(text.contains("\"data_center\": \"mrs04\""));
        assert!(text.contains("\"rank\": 1, \"server\": \"192.168.1.1\""));
        assert!(text.contains("\"flag\": \"UNRELIABLE\""));
        assert!(text.contains("\"dnssec\": true"));
        assert!(text.contains("\"current\": true"));
        assert!(text.contains("\"recommended\": \"1.1.1.1\""));
        assert!(text.contains("\"fastest_known\": \"1.1.1.1\""));
        assert!(text.contains("\"connect_ms\": null, \"failed\": 3"));
        let bare = json("0.5.0", None, &[], &HashSet::new(), None);
        assert!(bare.contains("\"data_center\": null"));
        assert!(bare.contains("\"steering\": null"));
        assert!(bare.contains("\"recommended\": null"));
        assert!(bare.contains("\"fastest\": null"));
    }
}
