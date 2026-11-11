//! The recommendation and insight model.
//!
//! All of this is pure: it takes a finished run and returns the rows and plain-language facts the
//! interface displays. Keeping it out of the GUI makes the comparison, close-call threshold and
//! platform-specific targets testable without a window.

use std::net::IpAddr;

use crate::bench::{Note, Row, SecurityStatus};
use crate::engine::RunResult;

/// One plain-language reason the winner was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Fastest,
    DnssecValidated,
    NoHijack,
    LowJitter,
    Encrypted,
}

impl Reason {
    pub const fn label(self) -> &'static str {
        match self {
            Reason::Fastest => "Fastest measured resolver",
            Reason::DnssecValidated => "Validates DNSSEC",
            Reason::NoHijack => "Does not hijack NXDOMAIN",
            Reason::LowJitter => "Stable latency",
            Reason::Encrypted => "Encrypted transport",
        }
    }
}

/// Everything the recommendation card needs, computed from one finished run.
#[derive(Clone, Debug, Default)]
pub struct Recommendation {
    /// The user's choice when it is recommendable, otherwise the fastest measured resolver.
    pub overall: Option<Row>,
    /// The fastest recommendable resolver that can be applied on desktop.
    pub best_plain: Option<Row>,
    /// The fastest recommendable encrypted resolver measured in this run.
    pub best_encrypted: Option<Row>,
    /// The row the user explicitly chose, even when it cannot be applied.
    pub chosen: Option<Row>,
    /// A measured row that matches the DNS currently in use.
    pub current: Option<Row>,
    /// How many milliseconds faster `overall` is than `current` (negative = slower).
    pub delta_ms: Option<f64>,
    /// The same comparison as a percentage of the current resolver's score.
    pub delta_pct: Option<f64>,
    /// The gap to the runner-up when the top two are close enough to call a tie.
    pub close_call_ms: Option<f64>,
    pub reasons: Vec<Reason>,
}

fn fastest<'a>(rows: impl Iterator<Item = &'a Row>) -> Option<&'a Row> {
    rows.min_by(|a, b| a.score.total_cmp(&b.score))
}

fn low_jitter(row: &Row) -> bool {
    match (row.cached_jitter, row.cached) {
        (Some(jitter), Some(cached)) if cached > 0.0 => jitter <= (cached * 0.25).max(1.0),
        _ => false,
    }
}

/// Build the recommendation from a finished run.
pub fn build(
    result: &RunResult,
    chosen_id: Option<&str>,
    current_dns: &[IpAddr],
) -> Recommendation {
    let recommendable: Vec<&Row> = result
        .rows
        .iter()
        .filter(|row| row.recommendable())
        .collect();
    let fastest_recommendable = fastest(recommendable.iter().copied());
    let chosen = chosen_id
        .and_then(|id| result.rows.iter().find(|row| row.id == id))
        .cloned();
    let overall = chosen
        .as_ref()
        .filter(|row| row.recommendable())
        .cloned()
        .or_else(|| fastest_recommendable.cloned());
    let best_plain = fastest(
        recommendable
            .iter()
            .copied()
            .filter(|row| row.is_plain() && row.plain_ip().is_some()),
    )
    .cloned();
    let best_encrypted =
        fastest(recommendable.iter().copied().filter(|row| !row.is_plain())).cloned();
    let current = result
        .rows
        .iter()
        .find(|row| row.plain_ip().is_some_and(|ip| current_dns.contains(&ip)))
        .cloned();

    let mut recommendation = Recommendation {
        overall,
        best_plain,
        best_encrypted,
        chosen,
        current,
        ..Recommendation::default()
    };

    if let (Some(overall), Some(current)) = (&recommendation.overall, &recommendation.current) {
        let delta = current.score - overall.score;
        recommendation.delta_ms = Some(delta);
        if current.score > 0.0 {
            recommendation.delta_pct = Some(delta / current.score * 100.0);
        }
    }

    let mut ranked = recommendable;
    ranked.sort_by(|a, b| a.score.total_cmp(&b.score));
    if let [top, second, ..] = ranked.as_slice() {
        let gap = (second.score - top.score).abs();
        if gap <= 2.0_f64.max(top.score * 0.10) {
            recommendation.close_call_ms = Some(gap);
        }
    }

    if let Some(overall) = &recommendation.overall {
        let fastest_id = fastest_recommendable.map(|row| row.id.as_str());
        if fastest_id == Some(overall.id.as_str()) {
            recommendation.reasons.push(Reason::Fastest);
        }
        if overall.security_status() == SecurityStatus::Validated {
            recommendation.reasons.push(Reason::DnssecValidated);
        }
        if overall.note == Note::Ok {
            recommendation.reasons.push(Reason::NoHijack);
        }
        if low_jitter(overall) {
            recommendation.reasons.push(Reason::LowJitter);
        }
        if !overall.is_plain() {
            recommendation.reasons.push(Reason::Encrypted);
        }
    }

    recommendation
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Note;
    use crate::engine::RunResult;
    use crate::profile::{Profile, TestDepth};
    use crate::servers::{Family, Protocol};
    use std::net::SocketAddr;

    fn row(label: &str, score: f64, transport: Protocol) -> Row {
        Row {
            id: format!("{label}:{}", transport.label()),
            provider: label.into(),
            label: label.into(),
            transport,
            addr: if transport.is_plain() {
                Some(SocketAddr::new("1.1.1.1".parse().unwrap(), 53))
            } else {
                None
            },
            endpoint: label.into(),
            category: crate::servers::Category::General,
            description: String::new(),
            website: String::new(),
            dot_host: None,
            cached: Some(score),
            uncached: Some(score + 1.0),
            cached_min: Some(score),
            cached_p95: Some(score + 2.0),
            cached_p99: Some(score + 3.0),
            cached_mean: Some(score),
            cached_max: Some(score + 4.0),
            cached_jitter: Some(0.5),
            dnssec: Some(true),
            dnssec_bogus: Some(true),
            timeouts: 0,
            note: Note::Ok,
            score,
        }
    }

    fn result(rows: Vec<Row>) -> RunResult {
        RunResult {
            protocol: Protocol::Udp,
            family: Family::Any,
            duration_secs: 1.0,
            data_center: None,
            current_dns: Vec::new(),
            rows,
            steerings: Vec::new(),
            scanned: 1,
            tested: 1,
            profile: Some(Profile::Balanced),
            depth: Some(TestDepth::BALANCED),
        }
    }

    #[test]
    fn build_picks_fastest_recommendable() {
        let result = result(vec![
            row("Slow", 30.0, Protocol::Udp),
            row("Fast", 10.0, Protocol::Udp),
        ]);
        let recommendation = build(&result, None, &[]);
        assert_eq!(recommendation.overall.unwrap().label, "Fast");
    }

    #[test]
    fn best_plain_skips_encrypted_winners() {
        let result = result(vec![
            row("Encrypted", 5.0, Protocol::Doh),
            row("Plain", 10.0, Protocol::Udp),
        ]);
        let recommendation = build(&result, None, &[]);
        assert_eq!(recommendation.overall.unwrap().label, "Encrypted");
        assert_eq!(recommendation.best_plain.unwrap().label, "Plain");
    }

    #[test]
    fn best_encrypted_requires_measured_encrypted_rows() {
        let plain_only = result(vec![row("Plain", 10.0, Protocol::Udp)]);
        assert!(build(&plain_only, None, &[]).best_encrypted.is_none());

        let mixed = result(vec![
            row("Plain", 10.0, Protocol::Udp),
            row("Encrypted", 12.0, Protocol::Dot),
        ]);
        assert_eq!(
            build(&mixed, None, &[]).best_encrypted.unwrap().label,
            "Encrypted"
        );
    }

    #[test]
    fn delta_compares_current_dns() {
        let mut current_row = row("Current", 20.0, Protocol::Udp);
        current_row.addr = Some(SocketAddr::new("192.168.1.1".parse().unwrap(), 53));
        let result = result(vec![row("Fast", 10.0, Protocol::Udp), current_row]);
        let recommendation = build(&result, None, &["192.168.1.1".parse().unwrap()]);
        assert_eq!(recommendation.current.unwrap().label, "Current");
        assert_eq!(recommendation.delta_ms, Some(10.0));
        assert_eq!(recommendation.delta_pct, Some(50.0));
    }

    #[test]
    fn close_call_uses_two_ms_or_ten_percent() {
        let close = result(vec![
            row("Top", 10.0, Protocol::Udp),
            row("Second", 11.5, Protocol::Udp),
        ]);
        assert_eq!(build(&close, None, &[]).close_call_ms, Some(1.5));

        let ten_percent = result(vec![
            row("Top", 100.0, Protocol::Udp),
            row("Second", 109.0, Protocol::Udp),
        ]);
        assert_eq!(build(&ten_percent, None, &[]).close_call_ms, Some(9.0));

        let clear = result(vec![
            row("Top", 100.0, Protocol::Udp),
            row("Second", 115.0, Protocol::Udp),
        ]);
        assert_eq!(build(&clear, None, &[]).close_call_ms, None);
    }

    #[test]
    fn chosen_encrypted_is_not_best_plain() {
        let encrypted = row("Encrypted", 5.0, Protocol::Doh);
        let chosen_id = encrypted.id.clone();
        let result = result(vec![encrypted, row("Plain", 10.0, Protocol::Udp)]);
        let recommendation = build(&result, Some(&chosen_id), &[]);
        assert_eq!(recommendation.chosen.unwrap().label, "Encrypted");
        assert_eq!(recommendation.overall.unwrap().label, "Encrypted");
        assert_eq!(recommendation.best_plain.unwrap().label, "Plain");
    }

    #[test]
    fn reasons_include_dnssec_and_no_hijack() {
        let result = result(vec![row("Fast", 10.0, Protocol::Udp)]);
        let recommendation = build(&result, None, &[]);
        assert!(recommendation.reasons.contains(&Reason::Fastest));
        assert!(recommendation.reasons.contains(&Reason::DnssecValidated));
        assert!(recommendation.reasons.contains(&Reason::NoHijack));
        assert!(!recommendation.reasons.contains(&Reason::Encrypted));
    }
}
