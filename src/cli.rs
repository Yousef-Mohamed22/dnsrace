//! The headless benchmark: `dnsrace --cli`.
//!
//! It reuses the same engine, protocols and report writers as the graphical application, so a
//! terminal result and a window result are produced by the same measurements. Progress goes to
//! stderr and the ranked table goes to stdout, which keeps piping and redirection clean.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::engine::{self, RunEvent, RunRequest};
use crate::profile::{Profile, TestDepth};
use crate::providers;
use crate::report;
use crate::servers::{Family, Protocol};

const HELP: &str = "\
dnsrace - find the fastest DNS resolver for your connection

Usage:
  dnsrace                    open the graphical application
  dnsrace --cli [options]    run a benchmark in the terminal

Options:
  --protocol udp|tcp|dot|doh  transport to measure (default: udp)
  --family any|v4|v6          address family (default: any)
  --profile quick|balanced|thorough
  --timeout MS                per-query timeout in milliseconds
  --jobs N                    concurrent queries
  --repeat N                  repeat the measurement
  --all                       test the whole catalogue, not the default selection
  --no-system                 do not include the current system DNS
  --top N                     rows in the terminal table (default: 20)
  --json PATH                 also write a JSON report
  --csv PATH                  also write a CSV report
  --list                      list the resolvers that would be tested
  --version                   print the version
  --help                      print this help
";

struct Options {
    protocol: Protocol,
    family: Family,
    profile: Profile,
    values: ProfileValues,
    all: bool,
    include_system: bool,
    top: usize,
    json: Option<PathBuf>,
    csv: Option<PathBuf>,
    list: bool,
}

#[derive(Clone, Copy)]
struct ProfileValues {
    timeout_ms: u64,
    jobs: usize,
    repeat: usize,
    depth: TestDepth,
}

impl ProfileValues {
    fn of(profile: Profile) -> Self {
        let values = profile.values();
        Self {
            timeout_ms: values.map_or(1000, |values| values.timeout_ms),
            jobs: values.map_or(16, |values| values.jobs),
            repeat: values.map_or(1, |values| values.repeat),
            depth: values.map_or(TestDepth::BALANCED, |values| TestDepth {
                cached_rounds: values.cached_rounds,
                random_lookups: values.random_lookups,
            }),
        }
    }
}

/// True when the process was asked for the terminal interface or its help.
pub fn requested(args: &[String]) -> bool {
    matches!(
        args.get(1).map(String::as_str),
        Some("--cli" | "--help" | "-h" | "--version" | "-V")
    )
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut options = Options {
        protocol: Protocol::Udp,
        family: Family::Any,
        profile: Profile::Balanced,
        values: ProfileValues::of(Profile::Balanced),
        all: false,
        include_system: true,
        top: 20,
        json: None,
        csv: None,
        list: false,
    };
    let mut overridden = false;
    let mut index = 1;
    while index < args.len() {
        let argument = args[index].clone();
        match argument.as_str() {
            "--cli" => {}
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("dnsrace {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--protocol" => {
                options.protocol = match next(args, &mut index, &argument)? {
                    "udp" => Protocol::Udp,
                    "tcp" => Protocol::Tcp,
                    "dot" => Protocol::Dot,
                    "doh" => Protocol::Doh,
                    other => return Err(format!("unknown protocol: {other}")),
                };
            }
            "--family" => {
                options.family = match next(args, &mut index, &argument)? {
                    "any" => Family::Any,
                    "v4" | "ipv4" => Family::V4,
                    "v6" | "ipv6" => Family::V6,
                    other => return Err(format!("unknown address family: {other}")),
                };
            }
            "--profile" => {
                options.profile = match next(args, &mut index, &argument)? {
                    "quick" => Profile::Quick,
                    "balanced" => Profile::Balanced,
                    "thorough" => Profile::Thorough,
                    other => return Err(format!("unknown profile: {other}")),
                };
                options.values = ProfileValues::of(options.profile);
            }
            "--timeout" => {
                options.values.timeout_ms =
                    parse::<u64>(next(args, &mut index, &argument)?, "--timeout")?
                        .clamp(100, 10_000);
                overridden = true;
            }
            "--jobs" => {
                options.values.jobs =
                    parse::<usize>(next(args, &mut index, &argument)?, "--jobs")?.clamp(1, 64);
                overridden = true;
            }
            "--repeat" => {
                options.values.repeat =
                    parse::<usize>(next(args, &mut index, &argument)?, "--repeat")?.clamp(1, 5);
                overridden = true;
            }
            "--top" => options.top = parse::<usize>(next(args, &mut index, &argument)?, "--top")?,
            "--json" => options.json = Some(PathBuf::from(next(args, &mut index, &argument)?)),
            "--csv" => options.csv = Some(PathBuf::from(next(args, &mut index, &argument)?)),
            "--list" => options.list = true,
            "--all" => options.all = true,
            "--no-system" => options.include_system = false,
            other => return Err(format!("unknown option: {other}")),
        }
        index += 1;
    }
    if overridden {
        options.profile = Profile::Custom;
    }

    let selected = providers::default_selection();
    let mut candidates = providers::catalog(options.protocol, options.family);
    if !options.all {
        let filtered: Vec<_> = candidates
            .iter()
            .filter(|candidate| selected.contains(&candidate.id))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            candidates = filtered;
        }
    }
    if candidates.is_empty() {
        return Err("no resolver matches this protocol and address family".to_owned());
    }
    if options.list {
        for candidate in &candidates {
            println!(
                "{}\t{}\t{}",
                candidate.id,
                candidate.label,
                candidate.endpoint()
            );
        }
        return Ok(());
    }

    let request = RunRequest {
        candidates,
        protocol: options.protocol,
        family: options.family,
        wait: Duration::from_millis(options.values.timeout_ms),
        jobs: options.values.jobs,
        repeat: options.values.repeat,
        hostnames: Vec::new(),
        connect_port: 443,
        include_system: options.include_system,
        include_nearby: false,
        environment: None,
        profile: options.profile,
        depth: options.values.depth,
    };

    eprintln!(
        "dnsrace {} · {} · {} · {} · {} jobs · repeat {}",
        env!("CARGO_PKG_VERSION"),
        options.protocol.label(),
        options.family.label(),
        options.profile.label(),
        options.values.jobs,
        options.values.repeat
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let handle = std::thread::spawn(move || engine::run_blocking(request, sender, cancel));
    while let Ok(event) = receiver.recv() {
        match event {
            RunEvent::Stage { stage, done, total } => {
                if total == 0 {
                    eprintln!("{}", stage.label());
                } else {
                    eprintln!("{}: {done}/{total}", stage.label());
                }
            }
            RunEvent::Row(row) => {
                eprintln!("  {:<24} {}", row.label, report::fmt_ms(Some(row.score)));
            }
            RunEvent::Detected {
                current_dns,
                data_center,
                ..
            } => {
                let dns = current_dns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!("current DNS: {}", if dns.is_empty() { "-" } else { &dns });
                if let Some(data_center) = data_center {
                    eprintln!("data center: {data_center}");
                }
            }
            RunEvent::Error(error) => return Err(error),
            _ => {}
        }
    }
    let result = handle
        .join()
        .map_err(|_| "the benchmark thread panicked".to_owned())??;

    let ranked = result
        .rows
        .iter()
        .filter(|row| row.recommendable())
        .take(options.top)
        .collect::<Vec<_>>();
    println!(
        "{:<4} {:<28} {:<32} {:>10}  Security",
        "#", "Resolver", "Endpoint", "Latency"
    );
    for (index, row) in ranked.iter().enumerate() {
        println!(
            "{:<4} {:<28} {:<32} {:>10}  {}",
            index + 1,
            row.label,
            row.endpoint,
            report::fmt_ms(Some(row.score)),
            row.security_status().short_en()
        );
    }
    if ranked.is_empty() {
        println!("no resolver completed enough measurements");
    }
    if let Some(path) = &options.json {
        fs::write(path, report::json(&result)).map_err(|error| error.to_string())?;
        eprintln!("wrote {}", path.display());
    }
    if let Some(path) = &options.csv {
        fs::write(path, report::csv(&result)).map_err(|error| error.to_string())?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

fn next<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, String> {
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .ok_or_else(|| format!("{option} needs a value"))
}

fn parse<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{option} needs a number, not {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        std::iter::once("dnsrace")
            .chain(items.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn cli_is_requested_after_the_program_name() {
        assert!(requested(&args(&["--cli"])));
        assert!(requested(&args(&["--help"])));
        assert!(requested(&args(&["--version"])));
        assert!(!requested(&args(&[])));
        assert!(!requested(&["--cli".to_owned()]));
    }
}
