//! The Android platform boundary.
//!
//! The 0.8.0 Android app never changes DNS itself. It copies a DoT hostname, opens the matching
//! settings screen and shares a text report. Those operations need a thin platform host; keeping
//! them behind a trait means the backend, guide and buttons can be tested on a desktop.

use std::cell::RefCell;

use crate::system::SystemInfo;

/// Space reserved for system bars and display cutouts, in density-independent points.
///
/// Android 15+ draws every app edge-to-edge by default, so the phone shell must pad its app bar
/// and bottom navigation by these insets. Whole points keep `ViewContext` comparable in tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SafeArea {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl SafeArea {
    pub const ZERO: SafeArea = SafeArea {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
    };

    pub fn is_zero(self) -> bool {
        self == SafeArea::ZERO
    }
}

/// One recorded Android platform call, used by the fake host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AndroidCall {
    Copy(String),
    OpenPrivateDns,
    OpenWifi,
    ShareText { subject: String, text: String },
}

/// Platform operations that only exist on a phone. Every method returns a `Result` so a missing
/// activity or denied intent surfaces as a notice instead of a panic.
pub trait AndroidHost {
    /// The device API level, for example 33 for Android 13.
    fn sdk_int(&self) -> u32;
    /// System bar and cutout insets reported by the activity.
    fn safe_area(&self) -> SafeArea;
    /// The system DNS values the platform can see. Android never runs Linux detection commands.
    fn current_dns(&self) -> Option<SystemInfo>;
    fn copy_text(&self, text: &str) -> Result<(), String>;
    /// Best-effort intent to Settings → Network & internet → Private DNS (API 28+).
    fn open_private_dns_settings(&self) -> Result<(), String>;
    /// Best-effort intent to Wi-Fi settings (API 26–27 flow).
    fn open_wifi_settings(&self) -> Result<(), String>;
    fn share_text(&self, subject: &str, text: &str) -> Result<(), String>;
}

/// A host used by GUI tests. It records every call and can be told to fail the next operation.
#[derive(Clone, Debug, Default)]
pub struct FakeAndroidHost {
    pub sdk_int: u32,
    pub safe_area: SafeArea,
    pub calls: RefCell<Vec<AndroidCall>>,
    pub fail: RefCell<Option<String>>,
    pub dns: Option<SystemInfo>,
}

impl FakeAndroidHost {
    pub fn new(sdk_int: u32) -> Self {
        FakeAndroidHost {
            sdk_int,
            ..FakeAndroidHost::default()
        }
    }

    pub fn failing(sdk_int: u32, message: &str) -> Self {
        let host = FakeAndroidHost::new(sdk_int);
        *host.fail.borrow_mut() = Some(message.into());
        host
    }

    pub fn with_dns(mut self, dns: SystemInfo) -> Self {
        self.dns = Some(dns);
        self
    }

    pub fn with_safe_area(mut self, safe_area: SafeArea) -> Self {
        self.safe_area = safe_area;
        self
    }

    pub fn take_calls(&self) -> Vec<AndroidCall> {
        std::mem::take(&mut *self.calls.borrow_mut())
    }

    fn finish(&self, call: AndroidCall) -> Result<(), String> {
        self.calls.borrow_mut().push(call);
        match self.fail.borrow().clone() {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }
}

impl AndroidHost for FakeAndroidHost {
    fn sdk_int(&self) -> u32 {
        self.sdk_int
    }

    fn safe_area(&self) -> SafeArea {
        self.safe_area
    }

    fn current_dns(&self) -> Option<SystemInfo> {
        self.dns.clone()
    }

    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.finish(AndroidCall::Copy(text.into()))
    }

    fn open_private_dns_settings(&self) -> Result<(), String> {
        self.finish(AndroidCall::OpenPrivateDns)
    }

    fn open_wifi_settings(&self) -> Result<(), String> {
        self.finish(AndroidCall::OpenWifi)
    }

    fn share_text(&self, subject: &str, text: &str) -> Result<(), String> {
        self.finish(AndroidCall::ShareText {
            subject: subject.into(),
            text: text.into(),
        })
    }
}

/// The real host. It keeps the `AndroidApp` handle and talks to the activity through JNI.
///
/// Unsafe code is confined to wrapping the activity's unowned global reference; every Java call
/// maps an error to a `String` and never panics.
#[cfg(target_os = "android")]
pub struct RealAndroidHost {
    app: winit::platform::android::activity::AndroidApp,
}

#[cfg(target_os = "android")]
impl RealAndroidHost {
    pub fn new(app: winit::platform::android::activity::AndroidApp) -> Self {
        RealAndroidHost { app }
    }

    /// Whether the user finished the activity (for example with the Back button).
    fn is_finishing(&self) -> bool {
        use jni::{jni_sig, jni_str};

        self.with_env(|env, activity| {
            env.call_method(activity, jni_str!("isFinishing"), jni_sig!("()Z"), &[])
                .and_then(|value| value.z())
        })
        .unwrap_or(false)
    }

    /// Attach the calling thread and run `call` against the activity.
    fn with_env<T>(
        &self,
        call: impl FnOnce(&mut jni::Env, &jni::objects::JObject) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        let vm = unsafe { jni::JavaVM::from_raw(self.app.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| {
            // `activity_as_ptr` is an unowned global reference owned by android-activity. Do not
            // delete it; wrapping it in `JObject` for the duration of the call is enough.
            let activity =
                unsafe { jni::objects::JObject::from_raw(env, self.app.activity_as_ptr().cast()) };
            call(env, &activity)
        })
        .map_err(|error| error.to_string())
    }

    fn open_settings(&self, action: &str) -> Result<(), String> {
        self.with_env(|env, activity| {
            use jni::{JValue, jni_sig, jni_str};

            let action = env.new_string(action)?;
            let intent = env.new_object(
                jni_str!("android/content/Intent"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&action)],
            )?;
            env.call_method(
                activity,
                jni_str!("startActivity"),
                jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        })
    }

    /// Try each settings action in order. Vendor ROMs such as HyperOS remove the direct
    /// Private DNS activity, so a failed intent must fall back to its parent screen rather
    /// than surfacing an ActivityNotFoundException.
    fn open_first_settings(&self, actions: &[&str]) -> Result<(), String> {
        for action in actions {
            if self.open_settings(action).is_ok() {
                return Ok(());
            }
        }
        Err(
            "no matching Settings screen was found on this device; open Settings manually."
                .to_string(),
        )
    }
}

#[cfg(target_os = "android")]
impl AndroidHost for RealAndroidHost {
    fn sdk_int(&self) -> u32 {
        self.with_env(|env, _activity| {
            use jni::{jni_sig, jni_str};

            let sdk = env.get_static_field(
                jni_str!("android/os/Build$VERSION"),
                jni_str!("SDK_INT"),
                jni_sig!("I"),
            )?;
            Ok(sdk.i()? as u32)
        })
        .unwrap_or(0)
    }

    fn safe_area(&self) -> SafeArea {
        self.with_env(|env, activity| {
            use jni::{JValue, jni_sig, jni_str};

            let window = env
                .call_method(
                    activity,
                    jni_str!("getWindow"),
                    jni_sig!("()Landroid/view/Window;"),
                    &[],
                )?
                .l()?;
            let decor = env
                .call_method(
                    &window,
                    jni_str!("getDecorView"),
                    jni_sig!("()Landroid/view/View;"),
                    &[],
                )?
                .l()?;
            let raw = env.call_method(
                &decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?;
            if raw.is_null() {
                return Ok(SafeArea::ZERO);
            }
            let insets = raw.l()?;

            let resources = env
                .call_method(
                    activity,
                    jni_str!("getResources"),
                    jni_sig!("()Landroid/content/res/Resources;"),
                    &[],
                )?
                .l()?;
            let metrics = env
                .call_method(
                    &resources,
                    jni_str!("getDisplayMetrics"),
                    jni_sig!("()Landroid/util/DisplayMetrics;"),
                    &[],
                )?
                .l()?;
            let density = env
                .get_field(&metrics, jni_str!("density"), jni_sig!("F"))?
                .f()?
                .max(1.0);

            let sdk = env
                .get_static_field(
                    jni_str!("android/os/Build$VERSION"),
                    jni_str!("SDK_INT"),
                    jni_sig!("I"),
                )?
                .i()?;

            let (left, top, right, bottom) = if sdk >= 30 {
                let bars = env
                    .call_static_method(
                        jni_str!("android/view/WindowInsets$Type"),
                        jni_str!("systemBars"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?;
                let cutout = env
                    .call_static_method(
                        jni_str!("android/view/WindowInsets$Type"),
                        jni_str!("displayCutout"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?;
                let safe = env
                    .call_method(
                        &insets,
                        jni_str!("getInsets"),
                        jni_sig!("(I)Landroid/graphics/Insets;"),
                        &[JValue::Int(bars | cutout)],
                    )?
                    .l()?;
                (
                    env.get_field(&safe, jni_str!("left"), jni_sig!("I"))?.i()? as f32,
                    env.get_field(&safe, jni_str!("top"), jni_sig!("I"))?.i()? as f32,
                    env.get_field(&safe, jni_str!("right"), jni_sig!("I"))?
                        .i()? as f32,
                    env.get_field(&safe, jni_str!("bottom"), jni_sig!("I"))?
                        .i()? as f32,
                )
            } else {
                (
                    env.call_method(
                        &insets,
                        jni_str!("getSystemWindowInsetLeft"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()? as f32,
                    env.call_method(
                        &insets,
                        jni_str!("getSystemWindowInsetTop"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()? as f32,
                    env.call_method(
                        &insets,
                        jni_str!("getSystemWindowInsetRight"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()? as f32,
                    env.call_method(
                        &insets,
                        jni_str!("getSystemWindowInsetBottom"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()? as f32,
                )
            };

            Ok(SafeArea {
                top: (top / density).round() as u32,
                right: (right / density).round() as u32,
                bottom: (bottom / density).round() as u32,
                left: (left / density).round() as u32,
            })
        })
        .unwrap_or(SafeArea::ZERO)
    }

    fn current_dns(&self) -> Option<SystemInfo> {
        self.with_env(|env, activity| {
            use jni::objects::JString;
            use jni::{JValue, jni_sig, jni_str};

            let service = env.new_string("connectivity")?;
            let manager = env.call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&service)],
            )?;
            if manager.is_null() {
                return Ok(None);
            }
            let manager = manager.l()?;

            let network = env.call_method(
                &manager,
                jni_str!("getActiveNetwork"),
                jni_sig!("()Landroid/net/Network;"),
                &[],
            )?;
            if network.is_null() {
                return Ok(None);
            }
            let network = network.l()?;

            let link = env.call_method(
                &manager,
                jni_str!("getLinkProperties"),
                jni_sig!("(Landroid/net/Network;)Landroid/net/LinkProperties;"),
                &[JValue::Object(&network)],
            )?;
            if link.is_null() {
                return Ok(None);
            }
            let link = link.l()?;

            let servers = env.call_method(
                &link,
                jni_str!("getDnsServers"),
                jni_sig!("()Ljava/util/List;"),
                &[],
            )?;
            if servers.is_null() {
                return Ok(None);
            }
            let servers = servers.l()?;
            let count = env
                .call_method(&servers, jni_str!("size"), jni_sig!("()I"), &[])?
                .i()?;

            let mut dns = Vec::new();
            for index in 0..count {
                let address = env.call_method(
                    &servers,
                    jni_str!("get"),
                    jni_sig!("(I)Ljava/lang/Object;"),
                    &[JValue::Int(index)],
                )?;
                if address.is_null() {
                    continue;
                }
                let address = address.l()?;
                let host = env
                    .call_method(
                        &address,
                        jni_str!("getHostAddress"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                let host = env.cast_local::<JString>(host)?;
                if let Ok(text) = host.try_to_string(env)
                    && let Ok(ip) = text.parse()
                {
                    dns.push(ip);
                }
            }

            let name = env.call_method(
                &link,
                jni_str!("getInterfaceName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?;
            let interface = if name.is_null() {
                None
            } else {
                let name = name.l()?;
                let name = env.cast_local::<JString>(name)?;
                name.try_to_string(env).ok()
            };

            Ok(Some(SystemInfo {
                gateway: None,
                dns,
                interface,
                source: Some("Android ConnectivityManager".into()),
            }))
        })
        .ok()
        .flatten()
    }

    fn copy_text(&self, text: &str) -> Result<(), String> {
        self.with_env(|env, activity| {
            use jni::{JValue, jni_sig, jni_str};

            // Android 8.0's ClipboardManager creates a Handler bound to the calling thread's
            // Looper. The winit native-activity thread has no Java Looper, so `getSystemService`
            // throws there. Prepare one first; our copy never registers for clipboard updates,
            // so the queue stays empty and the render thread never pumps it.
            let looper = env.call_static_method(
                jni_str!("android/os/Looper"),
                jni_str!("myLooper"),
                jni_sig!("()Landroid/os/Looper;"),
                &[],
            )?;
            if looper.is_null() {
                env.call_static_method(
                    jni_str!("android/os/Looper"),
                    jni_str!("prepare"),
                    jni_sig!("()V"),
                    &[],
                )?;
            }

            let service = env.new_string("clipboard")?;
            let manager = env.call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&service)],
            )?;
            if manager.is_null() {
                return Err(jni::errors::Error::NullPtr("ClipboardManager"));
            }
            let manager = manager.l()?;

            let label = env.new_string("dnsrace")?;
            let value = env.new_string(text)?;
            let clip = env
                .call_static_method(
                    jni_str!("android/content/ClipData"),
                    jni_str!("newPlainText"),
                    jni_sig!(
                        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                    ),
                    &[JValue::Object(&label), JValue::Object(&value)],
                )?
                .l()?;
            env.call_method(
                &manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(())
        })
    }

    fn open_private_dns_settings(&self) -> Result<(), String> {
        self.open_first_settings(&[
            // The documented screen (API 29+). HyperOS and some vendor ROMs do not export it.
            "android.settings.PRIVATE_DNS_SETTINGS",
            // Network & internet (where Private DNS lives on most phones).
            "android.settings.WIRELESS_SETTINGS",
            "android.settings.SETTINGS",
        ])
    }

    fn open_wifi_settings(&self) -> Result<(), String> {
        self.open_first_settings(&[
            "android.settings.WIFI_SETTINGS",
            "android.settings.WIRELESS_SETTINGS",
            "android.settings.SETTINGS",
        ])
    }

    fn share_text(&self, subject: &str, text: &str) -> Result<(), String> {
        self.with_env(|env, activity| {
            use jni::{JValue, jni_sig, jni_str};

            let action = env.new_string("android.intent.action.SEND")?;
            let send = env.new_object(
                jni_str!("android/content/Intent"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&action)],
            )?;

            let kind = env.new_string("text/plain")?;
            env.call_method(
                &send,
                jni_str!("setType"),
                jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&kind)],
            )?;

            let subject_key = env.new_string("android.intent.extra.SUBJECT")?;
            let subject_value = env.new_string(subject)?;
            env.call_method(
                &send,
                jni_str!("putExtra"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;)Landroid/content/Intent;"),
                &[JValue::Object(&subject_key), JValue::Object(&subject_value)],
            )?;

            let text_key = env.new_string("android.intent.extra.TEXT")?;
            let text_value = env.new_string(text)?;
            env.call_method(
                &send,
                jni_str!("putExtra"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;)Landroid/content/Intent;"),
                &[JValue::Object(&text_key), JValue::Object(&text_value)],
            )?;

            let title = env.new_string("Share dnsrace report")?;
            let chooser = env
                .call_static_method(
                    jni_str!("android/content/Intent"),
                    jni_str!("createChooser"),
                    jni_sig!(
                        "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;"
                    ),
                    &[JValue::Object(&send), JValue::Object(&title)],
                )?
                .l()?;
            env.call_method(
                activity,
                jni_str!("startActivity"),
                jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::Object(&chooser)],
            )?;
            Ok(())
        })
    }
}

/// End the process when the activity is finished. Winit ignores Android's `Destroy`
/// notification, so its event loop otherwise survives without an activity and a later launch can
/// hang before the first frame. Exiting here makes the next launch start a fresh process.
#[cfg(target_os = "android")]
pub fn exit_when_finished(app: winit::platform::android::activity::AndroidApp) {
    std::thread::spawn(move || {
        let host = RealAndroidHost::new(app);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if host.is_finishing() {
                std::process::exit(0);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_android_host_records_copy() {
        let host = FakeAndroidHost::new(33);
        assert_eq!(host.sdk_int(), 33);
        host.copy_text("dns.example.com").unwrap();
        host.open_private_dns_settings().unwrap();
        host.open_wifi_settings().unwrap();
        host.share_text("dnsrace", "report").unwrap();
        assert_eq!(
            host.take_calls(),
            vec![
                AndroidCall::Copy("dns.example.com".into()),
                AndroidCall::OpenPrivateDns,
                AndroidCall::OpenWifi,
                AndroidCall::ShareText {
                    subject: "dnsrace".into(),
                    text: "report".into(),
                },
            ]
        );
    }

    #[test]
    fn fake_android_host_can_fail_intents() {
        let host = FakeAndroidHost::failing(33, "no activity");
        assert_eq!(
            host.open_private_dns_settings(),
            Err("no activity".to_string())
        );
        assert_eq!(host.open_wifi_settings(), Err("no activity".to_string()));
        assert_eq!(
            host.share_text("dnsrace", "report"),
            Err("no activity".to_string())
        );
        // A failed intent is still recorded, so tests can see the attempt.
        assert_eq!(host.take_calls().len(), 3);
    }

    #[test]
    fn fake_android_host_reports_safe_area() {
        let area = SafeArea {
            top: 24,
            right: 0,
            bottom: 48,
            left: 12,
        };
        let host = FakeAndroidHost::new(33).with_safe_area(area);
        assert_eq!(host.safe_area(), area);
        assert!(!area.is_zero());
        assert!(SafeArea::ZERO.is_zero());
    }

    #[test]
    fn android_capabilities_supply_sdk_level() {
        let host = FakeAndroidHost::new(26);
        let capabilities = crate::platform::android_capabilities(&host);
        assert_eq!(capabilities.platform, crate::platform::Platform::Android);
        assert_eq!(capabilities.android_sdk, Some(26));
        assert!(!capabilities.can_apply_dns);
        assert!(!capabilities.can_export_files);
    }
}
