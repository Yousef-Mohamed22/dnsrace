//! dnsrace - race DNS servers from your own connection and find the fastest one for your
//! location.
//!
//! Stage 1 sends one lookup to every candidate (in parallel) and drops the dead ones.
//! Stage 2 fully tests the fastest ones, several servers at a time (optionally repeated):
//!   cached   = 30 lookups of popular sites  (mostly shows how far away the server is)
//!   uncached = 5 lookups of random names    (shows how good the server's own upstream is)
//! It also records the latency spread and asks one DNSSEC-signed name for the AD bit.
//! Servers are ranked by the average of the two medians. Nothing on your system is changed.

#![forbid(unsafe_code)]

mod app;
mod bench;
mod cli;
mod dns;
mod report;
mod servers;
mod system;
mod util;

use std::io::Write;
use std::process::ExitCode;

use cli::CliError;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match cli::parse_args(&args) {
        Ok(cfg) => cfg,
        Err(CliError::Help) => {
            let _ = std::io::stdout().write_all(cli::HELP.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(CliError::Version) => {
            let _ = writeln!(std::io::stdout(), "dnsrace {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(CliError::Usage(message)) => {
            let _ = writeln!(std::io::stderr(), "dnsrace: {message}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr(),
                "dnsrace: cannot start the async runtime: {e}"
            );
            return ExitCode::from(1);
        }
    };
    ExitCode::from(runtime.block_on(app::run(cfg)))
}
