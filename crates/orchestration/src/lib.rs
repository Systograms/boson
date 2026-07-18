//! Reusable local lifecycle orchestration for Boson projects.
//!
//! The CLI owns orchestration while application binaries stay small. Docker,
//! Cargo, process supervision, state, logs, and diagnostics remain hidden
//! behind this crate's lifecycle API.

mod diagnostics;
mod infrastructure;
mod manager;
mod process;
mod project;
mod state;

pub use diagnostics::{Diagnostic, DiagnosticLevel, run as run_diagnostics};
pub use infrastructure::{DockerComposeBackend, InfrastructureBackend, InfrastructureStatus};
pub use manager::{LifecycleManager, StatusEntry};
pub use project::{Project, ProjectManifest, find_project_root};
pub use state::{LifecycleState, StateStore, UnitKind, UnitState, process_is_alive};
