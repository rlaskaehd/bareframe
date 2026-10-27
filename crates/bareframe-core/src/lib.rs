//! Backend-independent workspace identity, session lifecycle and input state.
//! A terminal session and a command executed inside it have different lifetimes.
pub mod config;
pub mod fuzzy;
pub mod git;
pub mod ime;
pub mod keymap;
pub mod session;
pub mod settings;
pub mod state;
pub mod text;
pub mod workspace;
