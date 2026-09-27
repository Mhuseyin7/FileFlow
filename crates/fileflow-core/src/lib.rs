//! FileFlow core library.
//!
//! Pure Rust, no Tauri deps — so the rule engine, planner, safety validator,
//! filesystem operations, and journal can be unit-tested in isolation and reused
//! by CLI tools or the Tauri desktop app.
//!
//! Architecture (see ARCHITECTURE.md):
//!
//! file events / on-demand scan
//!   -> normalize                    (model::FileMeta)
//!   -> rule engine                  (rule::evaluate)
//!   -> action planner               (planner::plan)
//!   -> safety validator             (safety::validate)
//!   -> preview / execute            (fs_ops::execute)
//!   -> journal                      (journal::record)
//!   -> undo                         (journal::undo)

pub mod category;
pub mod db;
pub mod error;
pub mod fs_ops;
pub mod journal;
pub mod metadata;
pub mod model;
pub mod planner;
pub mod rename;
pub mod rule;
pub mod safety;
pub mod scheduler;
pub mod stability;
pub mod watcher;

pub use error::{Error, Result};
