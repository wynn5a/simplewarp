//! Frontend-neutral orchestration domain: edit state, transitions,
//! validation, and catalog providers shared by the orchestration controls.
//!
//! Nothing in this module may depend on `warpui::elements` or any other
//! GUI rendering types; it only reads/writes app singletons through
//! `AppContext`.

mod config_state;
mod edit_state;
mod providers;
mod snapshots;
mod validation;

pub use config_state::OrchestrationConfigState;
pub use edit_state::OrchestrationEditState;
pub use snapshots::{OptionRow, OptionSnapshot, harness_snapshot, model_snapshot};
pub use validation::accept_disabled_reason_with_setup;
