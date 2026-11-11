//! The effect boundary between the interface and the outside world.
//!
//! Pages never touch threads, files or the clipboard. The app calls a `Backend`, which either
//! performs a short operation and returns a result or starts a worker and returns a channel. The
//! test backend records every call so every button has a verifiable effect.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use eframe::egui;

use crate::android::{AndroidHost, FakeAndroidHost};
use crate::config::{self, Settings};
use crate::engine::{self, RunEvent, RunRequest, RunResult};
use crate::platform::{self, AppliedDns};
use crate::report;
use crate::system::SystemInfo;

use super::actions::ReportFormat;

/// One observable backend call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendCall {
    Detect,
    Run {
        protocol: crate::servers::Protocol,
        profile: crate::profile::Profile,
        jobs: usize,
        repeat: usize,
    },
    Apply(Vec<IpAddr>),
    Revert,
    Export(ReportFormat),
    Copy(String),
    OpenLink(String),
    ShareText {
        subject: String,
        text: String,
    },
    Cancel,
    SaveSettings,
}

/// A canned response for the test backend.
#[derive(Clone, Debug)]
pub enum BackendReply {
    Copy(Result<(), String>),
    OpenLink(Result<(), String>),
    ShareText(Result<(), String>),
    Export(Result<PathBuf, String>),
    Apply(Result<AppliedDns, String>),
    Revert(Result<(), String>),
    Detect(SystemInfo),
    RunEvent(Box<RunEvent>),
    SaveSettings(Result<(), String>),
}

/// Everything the app needs from the platform. Short operations return a `Result`; long ones
/// return a channel polled by the app on later frames.
pub trait Backend {
    fn start_detection(&self) -> mpsc::Receiver<SystemInfo>;
    fn start_run(&self, request: RunRequest) -> mpsc::Receiver<RunEvent>;
    fn apply(&self, addresses: Vec<IpAddr>) -> mpsc::Receiver<Result<AppliedDns, String>>;
    fn revert(&self, applied: AppliedDns) -> mpsc::Receiver<Result<(), String>>;
    fn export(
        &self,
        format: ReportFormat,
        result: &crate::engine::RunResult,
    ) -> Result<PathBuf, String>;
    fn copy(&self, text: &str) -> Result<(), String>;
    fn open_link(&self, url: &str) -> Result<(), String>;
    fn share_text(&self, subject: &str, text: &str) -> Result<(), String>;
    fn cancel_run(&self);
    fn save_settings(&self, settings: &Settings) -> Result<(), String>;
    fn android(&self) -> Option<&dyn AndroidHost>;
}

fn one_shot<T>(value: T) -> mpsc::Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    let _ = sender.send(value);
    receiver
}

/// A backend that records calls and returns queued replies. Cloning the handle shares the same
/// call log, so a test can keep one clone while the app owns another.
#[derive(Clone, Default)]
pub struct TestBackend {
    pub calls: RefCell<Vec<BackendCall>>,
    pub next: RefCell<VecDeque<BackendReply>>,
    pub android: Option<FakeAndroidHost>,
    /// The last result handed to `export`, so a test can prove the right run was exported.
    pub exported_result: RefCell<Option<RunResult>>,
    /// The last settings snapshot handed to `save_settings`.
    pub saved_settings: RefCell<Option<Settings>>,
    /// Senders kept alive so a test run's channel stays connected like a real worker's.
    run_senders: RefCell<Vec<mpsc::Sender<RunEvent>>>,
}

impl TestBackend {
    pub fn new() -> Self {
        TestBackend::default()
    }

    pub fn with_android(api_level: u32) -> Self {
        TestBackend {
            android: Some(FakeAndroidHost::new(api_level)),
            ..TestBackend::default()
        }
    }

    pub fn push_reply(&self, reply: BackendReply) {
        self.next.borrow_mut().push_back(reply);
    }

    pub fn take_calls(&self) -> Vec<BackendCall> {
        std::mem::take(&mut *self.calls.borrow_mut())
    }

    fn pop(&self) -> Option<BackendReply> {
        self.next.borrow_mut().pop_front()
    }
}

impl Backend for TestBackend {
    fn start_detection(&self) -> mpsc::Receiver<SystemInfo> {
        self.calls.borrow_mut().push(BackendCall::Detect);
        let info = match self.pop() {
            Some(BackendReply::Detect(info)) => info,
            _ => SystemInfo::default(),
        };
        one_shot(info)
    }

    fn start_run(&self, request: RunRequest) -> mpsc::Receiver<RunEvent> {
        self.calls.borrow_mut().push(BackendCall::Run {
            protocol: request.protocol,
            profile: request.profile,
            jobs: request.jobs,
            repeat: request.repeat,
        });
        let event = match self.pop() {
            Some(BackendReply::RunEvent(event)) => *event,
            _ => RunEvent::Error("the test backend does not run benchmarks".into()),
        };
        let (sender, receiver) = mpsc::channel();
        let _ = sender.send(event);
        // Keep the sender alive so a non-terminal event does not look like a disconnected
        // worker. Tests that send `Finished` still stop because the app drops its receiver.
        self.run_senders.borrow_mut().push(sender);
        receiver
    }

    fn apply(&self, addresses: Vec<IpAddr>) -> mpsc::Receiver<Result<AppliedDns, String>> {
        self.calls
            .borrow_mut()
            .push(BackendCall::Apply(addresses.clone()));
        let reply = match self.pop() {
            Some(BackendReply::Apply(result)) => result,
            _ => Ok(AppliedDns {
                platform: "test".into(),
                method: "test".into(),
                interface: "test".into(),
                applied: addresses,
                previous: Default::default(),
                timestamp: 0,
            }),
        };
        one_shot(reply)
    }

    fn revert(&self, _applied: AppliedDns) -> mpsc::Receiver<Result<(), String>> {
        self.calls.borrow_mut().push(BackendCall::Revert);
        let reply = match self.pop() {
            Some(BackendReply::Revert(result)) => result,
            _ => Ok(()),
        };
        one_shot(reply)
    }

    fn export(
        &self,
        format: ReportFormat,
        result: &crate::engine::RunResult,
    ) -> Result<PathBuf, String> {
        self.calls.borrow_mut().push(BackendCall::Export(format));
        *self.exported_result.borrow_mut() = Some(result.clone());
        match self.pop() {
            Some(BackendReply::Export(result)) => result,
            _ => Ok(PathBuf::from(format!(
                "dnsrace-test.{}",
                format.extension()
            ))),
        }
    }

    fn copy(&self, text: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(BackendCall::Copy(text.to_string()));
        match self.pop() {
            Some(BackendReply::Copy(result)) => result,
            _ => Ok(()),
        }
    }

    fn open_link(&self, url: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(BackendCall::OpenLink(url.to_string()));
        match self.pop() {
            Some(BackendReply::OpenLink(result)) => result,
            _ => Ok(()),
        }
    }

    fn share_text(&self, subject: &str, text: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(BackendCall::ShareText {
            subject: subject.to_string(),
            text: text.to_string(),
        });
        match self.pop() {
            Some(BackendReply::ShareText(result)) => result,
            _ => Ok(()),
        }
    }

    fn cancel_run(&self) {
        self.calls.borrow_mut().push(BackendCall::Cancel);
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        self.calls.borrow_mut().push(BackendCall::SaveSettings);
        *self.saved_settings.borrow_mut() = Some(settings.clone());
        match self.pop() {
            Some(BackendReply::SaveSettings(result)) => result,
            _ => Ok(()),
        }
    }

    fn android(&self) -> Option<&dyn AndroidHost> {
        self.android.as_ref().map(|host| host as &dyn AndroidHost)
    }
}

/// The production backend. It owns the worker threads and the platform integrations; the rest of
/// the application only sees receivers and `Result`s.
pub struct RealBackend {
    ctx: egui::Context,
    settings: RefCell<Settings>,
    cancel: Mutex<Arc<AtomicBool>>,
    android: Option<Box<dyn AndroidHost>>,
}

impl RealBackend {
    pub fn new(ctx: egui::Context, settings: Settings) -> Self {
        RealBackend {
            ctx,
            settings: RefCell::new(settings),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            android: None,
        }
    }

    /// Install the Android platform host once the JNI layer is available.
    pub fn set_android(&mut self, host: Box<dyn AndroidHost>) {
        self.android = Some(host);
    }

    fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Backend for RealBackend {
    fn start_detection(&self) -> mpsc::Receiver<SystemInfo> {
        if let Some(host) = &self.android {
            return one_shot(host.current_dns().unwrap_or_default());
        }
        let (sender, receiver) = mpsc::channel();
        let fallback = sender.clone();
        let spawned = thread::Builder::new()
            .name("dnsrace-detect".into())
            .spawn(move || {
                let _ = sender.send(engine::detect_system());
            });
        if spawned.is_err() {
            let _ = fallback.send(SystemInfo::default());
        }
        receiver
    }

    fn start_run(&self, request: RunRequest) -> mpsc::Receiver<RunEvent> {
        let mut request = request;
        if request.environment.is_none()
            && request.include_system
            && let Some(host) = &self.android
        {
            request.environment = Some(host.current_dns().unwrap_or_default());
        }

        let cancel = Arc::new(AtomicBool::new(false));
        *self
            .cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cancel.clone();

        let (sender, receiver) = mpsc::channel();
        let fallback = sender.clone();
        let spawned = thread::Builder::new()
            .name("dnsrace-benchmark".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(engine::run(request, sender, cancel)),
                    Err(error) => {
                        let _ = sender.send(RunEvent::Error(format!(
                            "Could not start the benchmark runtime: {error}"
                        )));
                    }
                }
            });
        if let Err(error) = spawned {
            let _ = fallback.send(RunEvent::Error(format!(
                "Could not start the benchmark thread: {error}"
            )));
        }
        receiver
    }

    fn apply(&self, addresses: Vec<IpAddr>) -> mpsc::Receiver<Result<AppliedDns, String>> {
        let (sender, receiver) = mpsc::channel();
        let fallback = sender.clone();
        let spawned = thread::Builder::new()
            .name("dnsrace-apply".into())
            .spawn(move || {
                let _ = sender.send(platform::apply(&addresses));
            });
        if let Err(error) = spawned {
            let _ = fallback.send(Err(format!("Could not start the apply operation: {error}")));
        }
        receiver
    }

    fn revert(&self, applied: AppliedDns) -> mpsc::Receiver<Result<(), String>> {
        let (sender, receiver) = mpsc::channel();
        let fallback = sender.clone();
        let spawned = thread::Builder::new()
            .name("dnsrace-revert".into())
            .spawn(move || {
                let _ = sender.send(platform::revert(&applied));
            });
        if let Err(error) = spawned {
            let _ = fallback.send(Err(format!(
                "Could not start the revert operation: {error}"
            )));
        }
        receiver
    }

    fn export(&self, format: ReportFormat, result: &RunResult) -> Result<PathBuf, String> {
        let text = match format {
            ReportFormat::Json => report::json(result),
            ReportFormat::Csv => report::csv(result),
            ReportFormat::Html => report::html(result),
        };
        let settings = self.settings.borrow();
        config::write_export(format.extension(), &settings, text.as_bytes())
            .map_err(|error| format!("Could not save the report: {error}"))
    }

    fn copy(&self, text: &str) -> Result<(), String> {
        if let Some(host) = &self.android {
            return host.copy_text(text);
        }
        self.ctx.copy_text(text.to_string());
        Ok(())
    }

    fn open_link(&self, url: &str) -> Result<(), String> {
        self.ctx.open_url(egui::OpenUrl::new_tab(url));
        Ok(())
    }

    fn share_text(&self, subject: &str, text: &str) -> Result<(), String> {
        if let Some(host) = &self.android {
            return host.share_text(subject, text);
        }
        // Desktop systems have no standard text-share intent; copying is the useful equivalent.
        self.ctx.copy_text(text.to_string());
        Ok(())
    }

    fn cancel_run(&self) {
        self.cancellation().store(true, Ordering::Relaxed);
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        config::save_settings(settings)
            .map_err(|error| format!("Could not save settings: {error}"))?;
        *self.settings.borrow_mut() = settings.clone();
        Ok(())
    }

    fn android(&self) -> Option<&dyn AndroidHost> {
        self.android.as_deref()
    }
}

/// Delegate through an `Rc` so tests can hold a handle to the same recording backend the app
/// owns.
impl<T: Backend + ?Sized> Backend for Rc<T> {
    fn start_detection(&self) -> mpsc::Receiver<SystemInfo> {
        (**self).start_detection()
    }

    fn start_run(&self, request: RunRequest) -> mpsc::Receiver<RunEvent> {
        (**self).start_run(request)
    }

    fn apply(&self, addresses: Vec<IpAddr>) -> mpsc::Receiver<Result<AppliedDns, String>> {
        (**self).apply(addresses)
    }

    fn revert(&self, applied: AppliedDns) -> mpsc::Receiver<Result<(), String>> {
        (**self).revert(applied)
    }

    fn export(
        &self,
        format: ReportFormat,
        result: &crate::engine::RunResult,
    ) -> Result<PathBuf, String> {
        (**self).export(format, result)
    }

    fn copy(&self, text: &str) -> Result<(), String> {
        (**self).copy(text)
    }

    fn open_link(&self, url: &str) -> Result<(), String> {
        (**self).open_link(url)
    }

    fn share_text(&self, subject: &str, text: &str) -> Result<(), String> {
        (**self).share_text(subject, text)
    }

    fn cancel_run(&self) {
        (**self).cancel_run();
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        (**self).save_settings(settings)
    }

    fn android(&self) -> Option<&dyn AndroidHost> {
        (**self).android()
    }
}
