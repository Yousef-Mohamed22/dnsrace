//! dnsrace - find the fastest DNS resolver for your connection.
//!
//! This crate builds a native desktop application. The benchmark and system helpers live in
//! the library so they can be tested without starting a window.

#![forbid(unsafe_code)]

pub mod bench;
pub mod config;
pub mod dns;
pub mod engine;
pub mod gui;
pub mod platform;
pub mod providers;
pub mod report;
pub mod servers;
pub mod system;
pub mod util;
