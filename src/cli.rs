//! Command-line parsing.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use crate::servers::parse_addr;

pub const HELP: &str = "\
dnsrace - race DNS servers from your own connection and find the fastest one for your location

USAGE:
    dnsrace [OPTIONS] [HOSTNAME]

ARGS:
    [HOSTNAME]   a site that is slow for you (e.g. github.com): also shows which IPs the
                 fastest servers hand out and how far away they are

OPTIONS:
    -c, --countries <CC,...>   also test the open resolvers public-dns.info lists for these
                               2-letter country codes (e.g. eg,it)
    -k, --known <N>            fully test the N fastest servers from the list (default 30)
    -o, --other <N>            fully test the N fastest country resolvers (default 10)
    -s, --servers <FILE>       use this server list instead of the built-in one
    -a, --add <ADDR,...>       also test these servers (IPv4 or IPv6, optionally with :port)
    -j, --jobs <N>             how many servers to fully test at once (default 16)
        --timeout <MS>         per-lookup timeout in the full test (default 1000)
        --connect-port <PORT>  TCP port for the connect-time check of HOSTNAME (default 443)
        --no-detect            don't look up your router, your current DNS servers or your
                               Cloudflare data center
        --json                 print the results as JSON on stdout (progress goes to stderr)
        --no-color             don't use colors
    -h, --help                 show this help
    -V, --version              show the version

EXAMPLES:
    dnsrace                        race the built-in server list
    dnsrace --countries eg         also test the open resolvers listed for Egypt
    dnsrace github.com             also show where github.com gets steered
    dnsrace --json > result.json   machine-readable results

The server list is one '<address> <name>' per line (see servers.txt in the repository).
Exit status: 0 on success, 1 if no server could be tested, 2 for a usage error.
";

/// Why parsing stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    /// `--help` was given: print the help and exit successfully.
    Help,
    /// `--version` was given.
    Version,
    /// The arguments were wrong; the message says how.
    Usage(String),
}

#[derive(Debug)]
pub struct Config {
    pub countries: Vec<String>,
    pub known: usize,
    pub other: usize,
    pub servers_file: Option<PathBuf>,
    pub add: Vec<SocketAddr>,
    pub jobs: usize,
    pub timeout: Duration,
    pub connect_port: u16,
    pub detect: bool,
    pub json: bool,
    pub no_color: bool,
    pub host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            countries: Vec::new(),
            known: 30,
            other: 10,
            servers_file: None,
            add: Vec::new(),
            jobs: 16,
            timeout: Duration::from_millis(1000),
            connect_port: 443,
            detect: true,
            json: false,
            no_color: false,
            host: None,
        }
    }
}

fn usage(msg: impl Into<String>) -> CliError {
    CliError::Usage(msg.into())
}

fn take_value(
    args: &[String],
    i: &mut usize,
    inline: Option<String>,
    flag: &str,
) -> Result<String, CliError> {
    if let Some(v) = inline {
        return Ok(v);
    }
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| usage(format!("{flag} needs a value (try --help)")))
}

fn parse_num<T: FromStr>(v: &str, flag: &str) -> Result<T, CliError> {
    v.trim()
        .parse()
        .map_err(|_| usage(format!("{flag}: '{v}' is not a valid number")))
}

/// Plain ASCII host names only: no URL scheme, no spaces, no path.
fn valid_hostname(h: &str) -> bool {
    let h = h.strip_suffix('.').unwrap_or(h);
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
}

fn list(v: &str) -> impl Iterator<Item = &str> {
    v.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
}

/// Parses the arguments (without the program name).
pub fn parse_args(args: &[String]) -> Result<Config, CliError> {
    let mut cfg = Config::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        match flag.as_str() {
            "-h" | "--help" => return Err(CliError::Help),
            "-V" | "--version" => return Err(CliError::Version),
            "-c" | "--countries" => {
                let v = take_value(args, &mut i, inline, &flag)?;
                for cc in list(&v) {
                    let cc = cc.to_ascii_lowercase();
                    if cc.len() == 2 && cc.chars().all(|c| c.is_ascii_lowercase()) {
                        cfg.countries.push(cc);
                    } else {
                        return Err(usage(format!(
                            "'{cc}' is not a 2-letter country code (try eg, it, fr)"
                        )));
                    }
                }
            }
            "-k" | "--known" => {
                cfg.known = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?
            }
            "-o" | "--other" => {
                cfg.other = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?
            }
            "-j" | "--jobs" => {
                let jobs: usize = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
                cfg.jobs = jobs.clamp(1, 64);
            }
            "--timeout" => {
                let millis: u64 = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
                cfg.timeout = Duration::from_millis(millis.clamp(100, 60_000));
            }
            "--connect-port" => {
                cfg.connect_port = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
            }
            "-s" | "--servers" => {
                cfg.servers_file = Some(PathBuf::from(take_value(args, &mut i, inline, &flag)?));
            }
            "-a" | "--add" => {
                let v = take_value(args, &mut i, inline, &flag)?;
                for token in list(&v) {
                    cfg.add.push(parse_addr(token).ok_or_else(|| {
                        usage(format!("--add: '{token}' is not an IPv4 or IPv6 address"))
                    })?);
                }
            }
            "--no-detect" => cfg.detect = false,
            "--json" => cfg.json = true,
            "--no-color" => cfg.no_color = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(usage(format!("unknown option '{s}' (try --help)")));
            }
            _ => {
                if cfg.host.is_some() {
                    return Err(usage("only one HOSTNAME is allowed (try --help)"));
                }
                if !valid_hostname(&arg) {
                    return Err(usage(format!(
                        "'{arg}' doesn't look like a host name (use just the name, e.g. github.com)"
                    )));
                }
                cfg.host = Some(arg);
            }
        }
        i += 1;
    }
    cfg.countries.sort();
    cfg.countries.dedup();
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Result<Config, CliError> {
        let args: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        parse_args(&args)
    }

    fn message(v: &[&str]) -> String {
        match parse(v) {
            Err(CliError::Usage(m)) => m,
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let c = parse(&[]).unwrap();
        assert_eq!((c.known, c.other, c.jobs), (30, 10, 16));
        assert_eq!(c.timeout, Duration::from_millis(1000));
        assert_eq!(c.connect_port, 443);
        assert!(c.detect && !c.json && !c.no_color);
        assert!(c.countries.is_empty() && c.add.is_empty() && c.host.is_none());
    }

    #[test]
    fn parses_every_option() {
        let c = parse(&[
            "-c",
            "EG,it",
            "--known=5",
            "-o",
            "3",
            "-j",
            "4",
            "--timeout",
            "250",
            "--connect-port=80",
            "-s",
            "list.txt",
            "-a",
            "1.2.3.4,[::1]:5353",
            "--no-detect",
            "--json",
            "--no-color",
            "github.com",
        ])
        .unwrap();
        assert_eq!(c.countries, vec!["eg", "it"]);
        assert_eq!((c.known, c.other, c.jobs), (5, 3, 4));
        assert_eq!(c.timeout, Duration::from_millis(250));
        assert_eq!(c.connect_port, 80);
        assert_eq!(c.servers_file, Some(PathBuf::from("list.txt")));
        assert_eq!(c.add.len(), 2);
        assert_eq!(c.add[1].port(), 5353);
        assert!(!c.detect && c.json && c.no_color);
        assert_eq!(c.host.as_deref(), Some("github.com"));
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse(&["--help"]).unwrap_err(), CliError::Help);
        assert_eq!(parse(&["-h"]).unwrap_err(), CliError::Help);
        assert_eq!(parse(&["-V"]).unwrap_err(), CliError::Version);
    }

    #[test]
    fn values_are_clamped_to_sensible_ranges() {
        assert_eq!(parse(&["--jobs", "0"]).unwrap().jobs, 1);
        assert_eq!(parse(&["--jobs", "9999"]).unwrap().jobs, 64);
        assert_eq!(
            parse(&["--timeout", "1"]).unwrap().timeout,
            Duration::from_millis(100)
        );
    }

    #[test]
    fn host_names_are_checked_label_by_label() {
        for good in [
            "github.com",
            "github.com.",
            "localhost",
            "a-b.c-d.example",
            "_dmarc.example.com",
            "xn--bcher-kva.example",
        ] {
            assert!(parse(&[good]).is_ok(), "{good} should be accepted");
        }
        let long_label = "a".repeat(64);
        for bad in [
            "a..b",
            ".a.com",
            "-a.com",
            "a-.com",
            "a b.com",
            "a/b.com",
            "a.com:443",
            &long_label,
        ] {
            assert!(parse(&[bad]).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn repeated_countries_are_only_downloaded_once() {
        let c = parse(&["-c", "it,eg", "--countries=EG", "-c", "it"]).unwrap();
        assert_eq!(c.countries, vec!["eg", "it"]);
    }

    #[test]
    fn rejects_bad_input_with_a_helpful_message() {
        assert!(message(&["--bogus"]).contains("unknown option '--bogus'"));
        assert!(message(&["-c", "egypt"]).contains("2-letter country code"));
        assert!(message(&["-k"]).contains("needs a value"));
        assert!(message(&["-k", "many"]).contains("not a valid number"));
        assert!(message(&["-a", "nope"]).contains("not an IPv4 or IPv6 address"));
        assert!(message(&["a.com", "b.com"]).contains("only one HOSTNAME"));
        assert!(message(&["https://github.com/x"]).contains("doesn't look like a host name"));
        assert!(message(&["--connect-port", "70000"]).contains("not a valid number"));
    }
}
