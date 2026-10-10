//! FileFlow app library. The Tauri shell is behind the `tauri` feature so that
//! `cargo test -p fileflow-core` and `cargo build -p fileflow-app` (with no
//! features) both work without a system WebView / GTK toolchain.
//!
//! The command layer here is deliberately thin: it wraps `fileflow-core` and
//! serializes to/from serde_json.

pub mod commands;
pub mod engine;
pub mod state;
