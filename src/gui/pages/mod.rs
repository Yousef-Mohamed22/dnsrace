//! The page renderers shared by the desktop and mobile shells.
//!
//! A page reads `AppState`, emits `UiAction` values and never performs I/O. The shells only
//! decide how the page is framed: a rail and top bar on desktop, an app bar and bottom
//! navigation on a phone.

pub mod benchmark;
pub mod history;
pub mod servers;
pub mod settings;

pub use benchmark::render_overview;
pub use history::render_history;
pub use servers::render_servers;
pub use settings::{render_about, render_settings};
