//! The dnsrace interface.
//!
//! The 0.8.0 shell is split into state, actions, a backend boundary, shared components, page
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

/// Starts the graphical application. There is no terminal/CLI mode.
pub fn run() -> eframe::Result {
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
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "dnsrace",
        options,
        Box::new(|cc| {
            let settings = config::load_settings();
            let history = config::load_history();
            let applied = config::load_applied();
            let capabilities = platform::Capabilities::current();

            // The first frame must already use the saved palette and bundled fonts.
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
            Ok(Box::new(app))
        }),
    )
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

            // The first frame must already use the saved palette and bundled fonts.
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
            Ok(Box::new(app))
        }),
    )
}
