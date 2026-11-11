//! dnsrace - find the fastest DNS resolver for your connection.
//!
//! This crate builds a native desktop application. The benchmark and system helpers live in
//! the library so they can be tested without starting a window.

// Android needs one exported entry point and thin JNI wrappers; everything else is safe Rust.
#![cfg_attr(not(target_os = "android"), forbid(unsafe_code))]

pub mod android;
pub mod android_entry;
pub mod android_guide;
pub mod bench;
pub mod config;
pub mod dns;
pub mod engine;
pub mod gui;
pub mod platform;
pub mod profile;
pub mod providers;
pub mod recommend;
pub mod report;
pub mod servers;
pub mod system;
pub mod util;
