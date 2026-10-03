//! Reprodução local, salas sincronizadas e cenários offline do sync2gether.

pub mod app;
#[cfg(feature = "demo")]
pub mod demo;
pub mod diagnostics;
pub mod model;
mod player_ui;
pub mod ui;
pub mod window;

pub mod media;
pub mod network;
pub mod player;
pub mod protocol;
pub mod runtime;
pub mod session;
pub mod sync;

pub mod discovery;
