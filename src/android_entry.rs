//! The Android application entry point.
//!
//! Android loads the crate as a shared library and calls `android_main`. The entry point owns the
//! two things that must happen before the first frame: pointing the config paths at the
//! application's private data directory and building the JNI platform host.

use std::path::PathBuf;

use crate::config;

/// Point the config files at the private directory Android hands the application. This must run
/// before the GUI reads settings or opens a window; the fallback `"."` is not writable on a phone.
pub fn init_android_paths(data_dir: PathBuf) {
    config::init_paths(data_dir, None);
}

#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;

    use winit::platform::android::activity::AndroidApp;

    use crate::android::{AndroidHost, RealAndroidHost};
    use crate::gui;

    /// Called by the Android loader on the application thread.
    #[unsafe(no_mangle)]
    pub fn android_main(app: AndroidApp) {
        let data_dir = app
            .internal_data_path()
            .unwrap_or_else(|| PathBuf::from("."));
        super::init_android_paths(data_dir);

        let host: Box<dyn AndroidHost> = Box::new(RealAndroidHost::new(app.clone()));
        // Android can finish the activity (Back) while the winit loop keeps running. End the
        // process at that point so reopening the app does not inherit a dead event loop.
        crate::android::exit_when_finished(app.clone());
        if let Err(error) = gui::run_android(app, host) {
            eprintln!("dnsrace: could not start the Android interface: {error}");
        }
        // The event loop has ended; an Android process with no loop cannot be reused.
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_paths_are_initialised_before_gui() {
        let _guard = config::path_test_lock();
        let dir =
            std::env::temp_dir().join(format!("dnsrace-android-paths-{}", std::process::id()));
        init_android_paths(dir.clone());
        assert_eq!(config::config_dir(), dir);
        assert_eq!(config::settings_path(), dir.join("settings.json"));
        assert!(dir.is_dir(), "the data directory is created eagerly");
        config::reset_paths_for_test();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
