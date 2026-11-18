//! dnsrace desktop entry point.

#![cfg_attr(not(target_os = "android"), forbid(unsafe_code))]
// Release builds on Windows are GUI applications: no console window appears behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if dnsrace::cli::requested(&args) {
        return match dnsrace::cli::run(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("dnsrace: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    match dnsrace::gui::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // The window could not be opened. There is no terminal interface, but printing the
            // reason is still the only way to explain a failure when launched from a desktop.
            eprintln!("dnsrace: could not start the graphical interface: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
