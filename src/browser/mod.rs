//! Browser panes: Chromium (CEF) inside the window, driven over the DevTools protocol from
//! inside the app (no port is opened), by the user and, when allowed, agents in terminals.
mod automation;
mod client;
pub mod control;
mod diagnostics;
pub mod engine;
mod mac;
mod pane;
pub mod protocol;

pub use mac::{between_events, in_run_loop};
pub use pane::{BrowserPane, BrowserPaneEvent, key_bindings};
