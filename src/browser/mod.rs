//! Browser panes: Chromium (CEF) inside the window, driven over the DevTools protocol from
//! inside the app (no port is opened).
mod client;
pub mod engine;
mod mac;
mod pane;

pub use mac::{between_events, in_run_loop};
pub use pane::{BrowserPane, BrowserPaneEvent, key_bindings};
