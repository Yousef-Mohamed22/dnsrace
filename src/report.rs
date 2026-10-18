//! Turns results into text and JSON. Everything here returns strings, so it is easy to test.

use std::collections::HashSet;
use std::fmt::Write;
use std::net::IpAddr;

use crate::bench::{Note, Row, SteerEntry, Steering};
use crate::servers::{fmt_addr, is_known};
use crate::util::median;

/// Makes untrusted values safe to embed in terminal text, before adding layout or colors.
pub fn terminal_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// The data-center announcement, also used on stderr with `--json`.
pub fn data_center_text(code: &str) -> String {
    format!(
        "== Cloudflare data center you reach (first 3 letters = nearest city's airport code, e.g. cai = Cairo, mrs = Marseille, fra = Frankfurt) ==\n{}\n",
        terminal_text(code)
    )
}

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

/// Bounds both name processing and padding, including any expanded control escapes.
fn table_name(name: &str) -> String {
    const MAX_NAME_WIDTH: usize = 64;
    let prefix: String = name.chars().take(MAX_NAME_WIDTH + 1).collect();
    let text = terminal_text(&prefix);
    if text.chars().count() > MAX_NAME_WIDTH {
        let prefix: String = text.chars().take(MAX_NAME_WIDTH - 3).collect();
        format!("{prefix}...")
    } else {
        text
    }
}

/// The ranked table, one line per server.
pub fn table(rows: &[Row], current: &HashSet<IpAddr>, color: bool) -> String {
    let addr_w = rows
        .iter()
        .map(|r| fmt_addr(r.addr).len())
        .max()
        .unwrap_or(0)
        .max(15);
    let names: Vec<String> = rows.iter().map(|r| table_name(&r.name)).collect();
    let name_w = names
        .iter()
        .map(|n| n.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    let mut text =
        "== Results: average of the cached and uncached medians, lower is better ==".to_string();
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
        write!(
            text,
            "\n{:>2}. {:<addr_w$} {:<name_w$} cached {}  uncached {}  timeouts {}{}{}",
            i + 1,
            fmt_addr(r.addr),
            names[i],
            fmt_ms(r.cached),
            fmt_ms(r.uncached),
            r.timeouts,
            flag,
            marker,
        )
        .expect("writing to a string cannot fail");
    }
    text
}

fn label(row: Option<&Row>) -> String {
    row.map(|r| format!("{} ({})", fmt_addr(r.addr), terminal_text(&r.name)))
        .unwrap_or_else(|| "-".to_string())
}

fn fastest(rows: &[Row]) -> Option<&Row> {
    rows.iter().find(|r| r.healthy())
}

fn fastest_known(rows: &[Row]) -> Option<&Row> {
    rows.iter().find(|r| r.healthy() && is_known(&r.name))
}

/// The two winners, and where your current DNS ranks.
pub fn summary(rows: &[Row], current: &HashSet<IpAddr>, color: bool) -> String {
    let mut lines = vec![
        paint(
            &format!("Fastest overall:        {}", label(fastest(rows))),
            "1;32",
            color,
        ),
        paint(
            &format!("Fastest known provider: {}", label(fastest_known(rows))),
            "1;32",
            color,
        ),
    ];
    let mine: Vec<String> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| is_current(r, current))
        .map(|(i, r)| format!("#{} ({})", i + 1, fmt_addr(r.addr)))
        .collect();
    if !mine.is_empty() {
        lines.push(format!(
            "Your current DNS:       {} of {} tested",
            mine.join(", "),
            rows.len()
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
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
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
            let flag = match r.note {
                Note::Ok => None,
                Note::Hijacks => Some("HIJACKS-NXDOMAIN".to_string()),
                Note::Unreliable => Some("UNRELIABLE".to_string()),
            };
            format!(
                "    {{\"rank\": {}, \"server\": {}, \"name\": {}, \"cached_ms\": {}, \"uncached_ms\": {}, \"timeouts\": {}, \"flag\": {}, \"current\": {}}}",
                i + 1,
                jstr(&fmt_addr(r.addr)),
                jstr(&r.name),
                jnum(r.cached),
                jnum(r.uncached),
                r.timeouts,
                jopt(flag),
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
        "{{\n  \"dnsrace\": {},\n  \"data_center\": {},\n  \"results\": [\n{}\n  ],\n  \"fastest\": {},\n  \"fastest_known\": {},\n  \"steering\": {}\n}}",
        jstr(version),
        jopt(data_center.map(str::to_string)),
        results.join(",\n"),
        jopt(fastest(rows).map(|r| fmt_addr(r.addr))),
        jopt(fastest_known(rows).map(|r| fmt_addr(r.addr))),
        steer
    )
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
            cached,
            uncached,
            timeouts,
            note,
            score: 0.0,
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
    fn terminal_values_escape_controls_and_preserve_printable_text() {
        let printable = r#"DNS café \ "quoted""#;
        assert_eq!(terminal_text(printable), printable);
        assert_eq!(
            terminal_text("\x1b]52;c;Y2xpcA==\x07\r\n\t\x7f\u{9b}2J"),
            "\\u{1b}]52;c;Y2xpcA==\\u{7}\\r\\n\\t\\u{7f}\\u{9b}2J"
        );
        for c in ('\0'..='\u{1f}').chain('\u{7f}'..='\u{9f}') {
            let text = terminal_text(&c.to_string());
            assert!(text.starts_with('\\'));
            assert!(!text.chars().any(char::is_control));
        }
    }

    #[test]
    fn data_center_announcement_escapes_txt_controls() {
        let text = data_center_text("mrs04\r\n\x1b]52;c;Y2xpcA==\x07");
        assert!(text.ends_with("\nmrs04\\r\\n\\u{1b}]52;c;Y2xpcA==\\u{7}\n"));
        assert_eq!(text.lines().count(), 2);
        assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
    }

    #[test]
    fn reports_escape_names_before_padding_and_adding_colors() {
        let rows = vec![
            row(
                "1.1.1.1",
                "Bad\x1b[2J\x07",
                Some(1.0),
                Some(2.0),
                0,
                Note::Ok,
            ),
            row("9.9.9.9", "Plain", None, None, 8, Note::Unreliable),
        ];
        let current = HashSet::new();
        let plain = table(&rows, &current, false);
        assert!(plain.contains("Bad\\u{1b}[2J\\u{7}"));
        // Padding uses the visible escaped name, so the following columns still line up.
        let columns: Vec<usize> = plain
            .lines()
            .skip(1)
            .map(|l| l.find("cached").unwrap())
            .collect();
        assert_eq!(columns[0], columns[1]);
        assert_eq!(
            table(&rows, &current, true)
                .replace("\x1b[31m", "")
                .replace("\x1b[0m", ""),
            plain
        );
        let plain = summary(&rows, &current, false);
        assert_eq!(plain.matches("Bad\\u{1b}[2J\\u{7}").count(), 2);
        assert_eq!(
            summary(&rows, &current, true)
                .replace("\x1b[1;32m", "")
                .replace("\x1b[0m", ""),
            plain
        );
        assert!(!plain.chars().any(|c| c.is_control() && c != '\n'));
    }

    #[test]
    fn table_bounds_padding_from_one_long_server_name() {
        let mut rows = vec![row("1.1.1.1", "Short", None, None, 0, Note::Ok); 128];
        rows[0].name = "x".repeat(32_768);
        let text = table(&rows, &HashSet::new(), false);
        assert!(text.len() < rows.len() * 256);
        assert!(text.contains(&format!("{}... cached", "x".repeat(61))));
        assert_eq!(text.lines().count(), rows.len() + 1);
    }

    #[test]
    fn table_bounds_escaped_names_at_character_boundaries() {
        for name in [
            "x".repeat(63),
            "x".repeat(64),
            "x".repeat(65),
            "é".repeat(65),
            "\x1b".repeat(65),
        ] {
            let rows = vec![
                row("1.1.1.1", &name, None, None, 0, Note::Ok),
                row("9.9.9.9", "Plain", None, None, 0, Note::Ok),
            ];
            let text = table(&rows, &HashSet::new(), false);
            let lines: Vec<&str> = text.lines().skip(1).collect();
            let columns: Vec<usize> = lines
                .iter()
                .map(|l| l[..l.find("cached").unwrap()].chars().count())
                .collect();
            assert!(columns[0] <= 85);
            assert_eq!(columns[0], columns[1]);
            assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
            if name.chars().count() <= 64 {
                assert!(lines[0].contains(&format!("{name} ")));
            } else {
                assert!(lines[0].contains("... cached"));
            }
            // Display truncation must leave the original value available in JSON.
            let decoded: serde_json::Value =
                serde_json::from_str(&json("0.3.0", None, &rows, &HashSet::new(), None)).unwrap();
            assert_eq!(decoded["results"][0]["name"], name);
        }
    }

    #[test]
    fn json_preserves_untrusted_values_without_literal_controls() {
        let value = "DNS\x1b]52;c;Y2xpcA==\x07\r\n\t\x7f\u{9b}\u{9d}\\\"";
        let rows = vec![row("1.1.1.1", value, Some(1.0), Some(2.0), 0, Note::Ok)];
        let text = json("0.3.0", Some(value), &rows, &HashSet::new(), None);
        assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
        let decoded: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(decoded["data_center"], value);
        assert_eq!(decoded["results"][0]["name"], value);
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
    fn the_summary_names_the_winners_and_your_rank() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let text = summary(&sample(), &current, false);
        assert!(text.contains("Fastest overall:        192.168.1.1 (Router/ISP-default)"));
        assert!(text.contains("Fastest known provider: 1.1.1.1 (Cloudflare)"));
        assert!(text.contains("Your current DNS:       #2 (1.1.1.1) of 5 tested"));
        let none = summary(&[], &HashSet::new(), false);
        assert!(none.contains("Fastest overall:        -"));
        assert!(!none.contains("Your current DNS"));
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
    fn json_escapes_strings() {
        assert_eq!(jstr("plain"), "\"plain\"");
        assert_eq!(jstr("a\"b\\c\nd\te\u{1}"), "\"a\\\"b\\\\c\\nd\\te\\u0001\"");
        assert_eq!(jnum(Some(1.26)), "1.3");
        assert_eq!(jnum(None), "null");
        assert_eq!(jnum(Some(f64::NAN)), "null");
    }

    #[test]
    fn json_is_shaped_as_documented() {
        let current: HashSet<IpAddr> = ["1.1.1.1".parse().unwrap()].into_iter().collect();
        let text = json(
            "0.3.0",
            Some("mrs04"),
            &sample(),
            &current,
            Some(&steering_sample()),
        );
        assert!(text.starts_with("{\n  \"dnsrace\": \"0.3.0\""));
        assert!(text.contains("\"data_center\": \"mrs04\""));
        assert!(text.contains("\"rank\": 1, \"server\": \"192.168.1.1\""));
        assert!(text.contains("\"flag\": \"UNRELIABLE\""));
        assert!(text.contains("\"current\": true"));
        assert!(text.contains("\"fastest_known\": \"1.1.1.1\""));
        assert!(text.contains("\"connect_ms\": null, \"failed\": 3"));
        let bare = json("0.3.0", None, &[], &HashSet::new(), None);
        assert!(bare.contains("\"data_center\": null"));
        assert!(bare.contains("\"steering\": null"));
        assert!(bare.contains("\"fastest\": null"));
    }
}
