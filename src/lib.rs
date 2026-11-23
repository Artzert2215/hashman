//! Hashman: generate and validate SHA-256 manifests for folders.
//!
//! * [`manifest`] is the hashing engine and has no GUI dependencies.
//! * [`App`] (behind the `gui` feature) is the egui front end.

pub mod manifest;

#[cfg(feature = "gui")]
mod app;
#[cfg(feature = "gui")]
pub use app::{App, COMPACT_HEIGHT, WINDOW_WIDTH};
