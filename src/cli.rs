//! Command-line parsing.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use crate::servers::parse_addr;

pub const HELP: &str = "\
dnsrace - race DNS servers from your own connection and find the fastest one for your location

USAGE:
    dnsrace [OPTIONS] [HOSTNAME...]

ARGS:
    [HOSTNAME]...   a site that is slow for you (e.g. github.com): also shows which IPs the
                    fastest servers hand out and how far away they are

WHAT TO TEST:
    -c, --countries <CC,...>   also test the open resolvers public-dns.info lists for these
                               2-letter country codes (e.g. eg,it)
        --nearby               also test the open resolvers listed for your country (detected
                               through Cloudflare; best effort)
    -k, --known <N>            fully test the N fastest servers from the list (default 30)
    -o, --other <N>            fully test the N fastest country resolvers (default 10)
    -s, --servers <FILE>       use this server list instead of the built-in one
    -a, --add <ADDR,...>       also test these servers (IPv4 or IPv6, optionally with :port)
    -4, --ipv4                 only test IPv4 servers
    -6, --ipv6                 only test IPv6 servers
        --tcp                  query over TCP instead of UDP
        --doh                  test DNS-over-HTTPS (Cloudflare and Google) as servers too
    -r, --repeat <N>           repeat the full test N times and pool the measurements
                               (1-5, default 1); slower but steadier on a noisy connection

OUTPUT:
    -q, --quiet                print only the recommended server address, nothing else
        --csv                   print the results as CSV on stdout (progress goes to stderr)
        --json                  print the results as JSON on stdout (progress goes to stderr)
    -v, --verbose              add latency spread, DNSSEC details and mean/max/p99 to the table
        --top <N>              show only the N fastest rows in the table (default: all)
        --no-color             don't use colors
        --gui                   open the graphical interface (needs the 'gui' feature)
        --export-json <FILE>    write the JSON results to this file
        --export-csv <FILE>     write the CSV results to this file
        --export-html <FILE>    write an HTML report to this file

PERFORMANCE:
    -j, --jobs <N>             how many servers to fully test at once (default 16)
        --timeout <MS>         per-lookup timeout (default 1000; the quick scan uses at most
                               3000)
        --connect-port <PORT>  TCP port for the connect-time check of HOSTNAME (default 443)
        --no-detect            don't look up your router, your current DNS servers or your
                               Cloudflare data center

    -h, --help                 show this help
    -V, --version              show the version

EXAMPLES:
    dnsrace                        race the built-in server list
    dnsrace --nearby               also test the open resolvers near you
    dnsrace --countries eg         also test the open resolvers listed for Egypt
    dnsrace github.com             also show where github.com gets steered
    dnsrace -v --repeat 2          a steadier run with more detail
    dnsrace -q                     print just the recommended address (for scripts)
    dnsrace --json > result.json   machine-readable results
    dnsrace --csv > result.csv     open the results in a spreadsheet
    dnsrace --gui                  open the graphical interface
    dnsrace --export-html report.html  save a visual HTML report

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

/// Which servers to test.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Family {
    /// IPv4 and IPv6.
    #[default]
    Any,
    /// IPv4 only.
    V4,
    /// IPv6 only.
    V6,
}

/// How to print the results.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFormat {
    /// The normal ranked table.
    #[default]
    Table,
    /// A JSON document.
    Json,
    /// Comma-separated values.
    Csv,
    /// Only the recommended server address.
    Quiet,
}

#[derive(Debug)]
pub struct Config {
    pub countries: Vec<String>,
    pub nearby: bool,
    pub known: usize,
    pub other: usize,
    pub servers_file: Option<PathBuf>,
    pub add: Vec<SocketAddr>,
    pub family: Family,
    pub repeat: usize,
    pub jobs: usize,
    pub timeout: Duration,
    pub connect_port: u16,
    pub detect: bool,
    pub format: OutputFormat,
    pub verbose: bool,
    pub top: usize,
    pub no_color: bool,
    pub host: Vec<String>,
    pub tcp: bool,
    pub doh: bool,
    pub gui: bool,
    pub export_json: Option<PathBuf>,
    pub export_csv: Option<PathBuf>,
    pub export_html: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            countries: Vec::new(),
            nearby: false,
            known: 30,
            other: 10,
            servers_file: None,
            add: Vec::new(),
            family: Family::Any,
            repeat: 1,
            jobs: 16,
            timeout: Duration::from_millis(1000),
            connect_port: 443,
            detect: true,
            format: OutputFormat::Table,
            verbose: false,
            top: 0,
            no_color: false,
            host: Vec::new(),
            tcp: false,
            doh: false,
            gui: false,
            export_json: None,
            export_csv: None,
            export_html: None,
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

fn set_family(cfg: &mut Config, family: Family, flag: &str) -> Result<(), CliError> {
    if cfg.family != Family::Any && cfg.family != family {
        return Err(usage(format!(
            "{flag} cannot be combined with the other address-family option"
        )));
    }
    cfg.family = family;
    Ok(())
}

fn set_format(cfg: &mut Config, format: OutputFormat, flag: &str) -> Result<(), CliError> {
    if cfg.format != OutputFormat::Table && cfg.format != format {
        return Err(usage(format!(
            "{flag} cannot be combined with another output format (--json, --csv, --quiet)"
        )));
    }
    cfg.format = format;
    Ok(())
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
            "--nearby" => cfg.nearby = true,
            "-k" | "--known" => {
                cfg.known = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?
            }
            "-o" | "--other" => {
                cfg.other = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?
            }
            "-r" | "--repeat" => {
                let repeat: usize = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
                cfg.repeat = repeat.clamp(1, 5);
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
                let port: u16 = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
                if port == 0 {
                    return Err(usage("--connect-port must be between 1 and 65535"));
                }
                cfg.connect_port = port;
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
            "-4" | "--ipv4" => set_family(&mut cfg, Family::V4, &flag)?,
            "-6" | "--ipv6" => set_family(&mut cfg, Family::V6, &flag)?,
            "-q" | "--quiet" => set_format(&mut cfg, OutputFormat::Quiet, &flag)?,
            "--csv" => set_format(&mut cfg, OutputFormat::Csv, &flag)?,
            "--json" => set_format(&mut cfg, OutputFormat::Json, &flag)?,
            "-v" | "--verbose" => cfg.verbose = true,
            "--top" => {
                cfg.top = parse_num(&take_value(args, &mut i, inline, &flag)?, &flag)?;
            }
            "--no-detect" => cfg.detect = false,
            "--no-color" => cfg.no_color = true,
            "--tcp" => cfg.tcp = true,
            "--doh" => cfg.doh = true,
            "--gui" => cfg.gui = true,
            "--export-json" => {
                cfg.export_json = Some(PathBuf::from(take_value(args, &mut i, inline, &flag)?));
            }
            "--export-csv" => {
                cfg.export_csv = Some(PathBuf::from(take_value(args, &mut i, inline, &flag)?));
            }
            "--export-html" => {
                cfg.export_html = Some(PathBuf::from(take_value(args, &mut i, inline, &flag)?));
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(usage(format!("unknown option '{s}' (try --help)")));
            }
            _ => {
                if !valid_hostname(&arg) {
                    return Err(usage(format!(
                        "'{arg}' doesn't look like a host name (use just the name, e.g. github.com)"
                    )));
                }
                cfg.host.push(arg);
            }
        }
        i += 1;
    }
    cfg.countries.sort();
    cfg.countries.dedup();
    cfg.host.sort();
    cfg.host.dedup();
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
        assert_eq!((c.known, c.other, c.jobs, c.repeat), (30, 10, 16, 1));
        assert_eq!(c.timeout, Duration::from_millis(1000));
        assert_eq!(c.connect_port, 443);
        assert_eq!(c.family, Family::Any);
        assert_eq!(c.format, OutputFormat::Table);
        assert!(c.detect && !c.verbose && !c.nearby && !c.no_color);
        assert!(c.countries.is_empty() && c.add.is_empty() && c.host.is_empty());
        assert_eq!(c.top, 0);
        assert!(!c.tcp && !c.doh && !c.gui);
        assert!(c.export_json.is_none() && c.export_csv.is_none() && c.export_html.is_none());
    }

    #[test]
    fn parses_every_option() {
        let c = parse(&[
            "-c",
            "EG,it",
            "--nearby",
            "--known=5",
            "-o",
            "3",
            "-j",
            "4",
            "-r",
            "3",
            "--timeout",
            "250",
            "--connect-port=80",
            "-s",
            "list.txt",
            "-a",
            "1.2.3.4,[::1]:5353",
            "-4",
            "-q",
            "-v",
            "--top",
            "10",
            "--no-detect",
            "--no-color",
            "--tcp",
            "--doh",
            "--gui",
            "--export-json",
            "out.json",
            "--export-csv",
            "out.csv",
            "--export-html",
            "report.html",
            "github.com",
            "example.org",
        ])
        .unwrap();
        assert_eq!(c.countries, vec!["eg", "it"]);
        assert_eq!((c.known, c.other, c.jobs, c.repeat), (5, 3, 4, 3));
        assert_eq!(c.timeout, Duration::from_millis(250));
        assert_eq!(c.connect_port, 80);
        assert_eq!(c.servers_file, Some(PathBuf::from("list.txt")));
        assert_eq!(c.add.len(), 2);
        assert_eq!(c.add[1].port(), 5353);
        assert_eq!(c.family, Family::V4);
        assert_eq!(c.format, OutputFormat::Quiet);
        assert!(c.verbose && c.nearby && !c.detect && c.no_color);
        assert_eq!(c.top, 10);
        assert_eq!(
            c.host,
            vec!["example.org".to_string(), "github.com".to_string()]
        );
        assert!(c.tcp && c.doh && c.gui);
        assert_eq!(c.export_json, Some(PathBuf::from("out.json")));
        assert_eq!(c.export_csv, Some(PathBuf::from("out.csv")));
        assert_eq!(c.export_html, Some(PathBuf::from("report.html")));
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
        assert_eq!(parse(&["--repeat", "0"]).unwrap().repeat, 1);
        assert_eq!(parse(&["--repeat", "99"]).unwrap().repeat, 5);
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
    fn output_formats_are_exclusive() {
        assert_eq!(
            parse(&["--json", "--csv"]).unwrap_err(),
            CliError::Usage(
                "--csv cannot be combined with another output format (--json, --csv, --quiet)"
                    .into()
            )
        );
        assert!(parse(&["-q", "--json"]).is_err());
        assert_eq!(
            parse(&["--json", "--json"]).unwrap().format,
            OutputFormat::Json
        );
        assert_eq!(parse(&["--csv"]).unwrap().format, OutputFormat::Csv);
    }

    #[test]
    fn address_families_are_exclusive() {
        assert!(parse(&["-4", "--ipv6"]).is_err());
        assert_eq!(parse(&["-6"]).unwrap().family, Family::V6);
        assert_eq!(parse(&["--ipv4", "-4"]).unwrap().family, Family::V4);
    }

    #[test]
    fn rejects_bad_input_with_a_helpful_message() {
        assert!(message(&["--bogus"]).contains("unknown option '--bogus'"));
        assert!(message(&["-c", "egypt"]).contains("2-letter country code"));
        assert!(message(&["-k"]).contains("needs a value"));
        assert!(message(&["-k", "many"]).contains("not a valid number"));
        assert!(message(&["-a", "nope"]).contains("not an IPv4 or IPv6 address"));
        assert!(message(&["https://github.com/x"]).contains("doesn't look like a host name"));
        assert!(message(&["--connect-port", "0"]).contains("between 1 and 65535"));
        assert!(message(&["--connect-port", "70000"]).contains("not a valid number"));
        assert!(message(&["--top", "-1"]).contains("not a valid number"));
    }

    #[test]
    fn multiple_hostnames_are_accepted_and_deduped() {
        let c = parse(&["example.com", "Example.com", "test.org"]).unwrap();
        assert_eq!(
            c.host,
            vec![
                "Example.com".to_string(),
                "example.com".to_string(),
                "test.org".to_string()
            ]
        );
    }

    #[test]
    fn new_options_parse() {
        let c = parse(&[
            "--tcp",
            "--doh",
            "--gui",
            "--export-json",
            "r.json",
            "--export-csv",
            "r.csv",
            "--export-html",
            "r.html",
        ])
        .unwrap();
        assert!(c.tcp && c.doh && c.gui);
        assert_eq!(c.export_json, Some(PathBuf::from("r.json")));
        assert_eq!(c.export_csv, Some(PathBuf::from("r.csv")));
        assert_eq!(c.export_html, Some(PathBuf::from("r.html")));
    }
}
