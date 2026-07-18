//! Reusable local lifecycle orchestration for Boson projects.
//!
//! The CLI owns application process orchestration while application binaries
//! stay small. Build execution, process supervision, state, logs, and
//! diagnostics remain hidden behind this crate's lifecycle API. Infrastructure
//! is supplied and operated independently by the developer.

mod diagnostics;
mod manager;
mod process;
mod project;
mod state;

pub use diagnostics::{Diagnostic, DiagnosticLevel, run as run_diagnostics};
pub use manager::{LifecycleManager, StatusEntry};
pub use project::{Project, ProjectManifest, find_project_root};
pub use state::{LifecycleState, StateStore, UnitKind, UnitState, process_is_alive};
