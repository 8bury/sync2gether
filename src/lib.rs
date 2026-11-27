pub mod app;
#[cfg(feature = "demo")]
pub mod demo;
pub mod diagnostics;
pub mod i18n;
pub mod model;
mod player_ui;
pub mod ui;
pub mod window;

pub mod media;
pub mod network;
pub mod player;
pub mod preferences;
pub mod runtime;
pub mod stream;
pub use sync2gether_core::{host, protocol, session, sync};

pub mod discovery;
