//! Backend-independent workspace identity, session lifecycle and input state.
//! A terminal session and a command executed inside it have different lifetimes.
pub mod config;
pub mod ime;
pub mod session;
pub mod workspace;
