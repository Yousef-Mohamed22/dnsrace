//! The dnsrace interface.
//!
//! The shell is split into state, actions, a backend boundary, shared components, page
//! renderers and two shells. This module owns the application lifecycle and the production
//! entry point.

use eframe::egui;

use crate::config;
use crate::platform;

pub mod actions;
pub mod app;
pub mod backend;
pub mod components;
pub mod desktop;
pub mod icons;
pub mod mobile;
pub mod modals;
pub mod pages;
pub mod state;
pub mod theme;

#[cfg(test)]
mod tests;

pub use app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_ID: &str = "io.github.yousef_mohamed22.dnsrace";

/// Starts the graphical application. The headless benchmark entry point is `cli::run`.
pub fn run() -> eframe::Result {
    #[cfg(target_os = "linux")]
    {
        // Prefer Wayland even when XWayland is also advertised. A stale Wayland environment or
        // a compositor that has gone away falls back to X11 before winit creates its one-shot
        // event loop, because winit cannot retry a backend in the same process.
        if wayland_available() {
            return eframe::run_native(
                "dnsrace",
                linux_options(LinuxBackend::Wayland),
                Box::new(create_app),
            );
        }
        if wayland_advertised() {
            eprintln!("dnsrace: Wayland display is unavailable; falling back to X11");
        }
        eframe::run_native(
            "dnsrace",
            linux_options(LinuxBackend::X11),
            Box::new(create_app),
        )
    }

    #[cfg(not(target_os = "linux"))]
    eframe::run_native("dnsrace", native_options(), Box::new(create_app))
}

fn native_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dnsrace")
        .with_app_id(APP_ID)
        .with_inner_size([1240.0, 800.0])
        .with_min_inner_size([1000.0, 620.0]);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../packaging/icon-256.png"))
    {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    }
}

fn create_app(
    cc: &eframe::CreationContext<'_>,
) -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
    let settings = config::load_settings();
    let history = config::load_history();
    let applied = config::load_applied();
    let capabilities = platform::Capabilities::current();

    // The first frame must already use the saved language, palette and bundled fonts.
    crate::i18n::set_language(settings.language);
    theme::install(&cc.egui_ctx, settings.theme);

    let backend = backend::RealBackend::new(cc.egui_ctx.clone(), settings.clone());
    let state = state::AppState::new(settings, history, applied, Vec::new(), capabilities);
    let cx = state::ViewContext {
        capabilities,
        layout: if capabilities.platform == platform::Platform::Android {
            state::LayoutMode::Mobile
        } else {
            state::LayoutMode::Desktop
        },
        version: VERSION,
        safe_area: crate::android::SafeArea::ZERO,
    };
    let mut app = app::App::new(state, cx, Box::new(backend));
    app.start_initial_tasks();
    app.start_update_check_if_enabled();
    Ok(Box::new(app))
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum LinuxBackend {
    Wayland,
    X11,
}

/// True when the session advertises a Wayland display.
#[cfg(target_os = "linux")]
fn wayland_advertised() -> bool {
    ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Check that the advertised Wayland compositor is actually reachable. A stale environment
/// would otherwise make winit pick Wayland and fail before it can try X11.
#[cfg(target_os = "linux")]
fn wayland_available() -> bool {
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    if std::env::var_os("WAYLAND_SOCKET").is_some_and(|value| !value.is_empty()) {
        return true;
    }
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty())
    else {
        return false;
    };
    let display = PathBuf::from(display);
    let socket = if display.is_absolute() {
        display
    } else {
        let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty())
        else {
            return false;
        };
        PathBuf::from(runtime).join(display)
    };
    UnixStream::connect(socket).is_ok()
}

/// Force the requested Linux backend. Forcing prevents winit from picking XWayland just because
/// `DISPLAY` is also set.
#[cfg(target_os = "linux")]
fn linux_options(backend: LinuxBackend) -> eframe::NativeOptions {
    let mut options = native_options();
    options.event_loop_builder = Some(Box::new(move |builder| match backend {
        LinuxBackend::Wayland => {
            use winit::platform::wayland::EventLoopBuilderExtWayland as _;
            builder.with_wayland();
        }
        LinuxBackend::X11 => {
            use winit::platform::x11::EventLoopBuilderExtX11 as _;
            builder.with_x11();
        }
    }));
    options
}

/// Starts the Android application. The JNI host is installed before the first frame so system
/// detection, clipboard, sharing and settings intents all use the real activity.
#[cfg(target_os = "android")]
pub fn run_android(
    app: winit::platform::android::activity::AndroidApp,
    host: Box<dyn crate::android::AndroidHost>,
) -> eframe::Result {
    let capabilities = platform::android_capabilities(host.as_ref());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dnsrace")
            .with_app_id(APP_ID),
        android_app: Some(app),
        ..Default::default()
    };

    eframe::run_native(
        "dnsrace",
        options,
        Box::new(move |cc| {
            let settings = config::load_settings();
            let history = config::load_history();
            let applied = config::load_applied();

            // The first frame must already use the saved language, palette and bundled fonts.
            crate::i18n::set_language(settings.language);
            theme::install(&cc.egui_ctx, settings.theme);

            let mut backend = backend::RealBackend::new(cc.egui_ctx.clone(), settings.clone());
            backend.set_android(host);
            let state = state::AppState::new(settings, history, applied, Vec::new(), capabilities);
            let cx = state::ViewContext {
                capabilities,
                layout: state::LayoutMode::Mobile,
                version: VERSION,
                safe_area: crate::android::SafeArea::ZERO,
            };
            let mut app = app::App::new(state, cx, Box::new(backend));
            app.start_initial_tasks();
            app.start_update_check_if_enabled();
            Ok(Box::new(app))
        }),
    )
}
