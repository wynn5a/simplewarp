//! This crate defines the concrete Warp cloud object models and typed cloud object aliases built
//! on top of `cloud_objects`.
//!
//! Each model module should own the model payload for one cloud object family, plus any model-specific
//! adapters that should move with that model during future verticalization.
//!
//! Native SQLite adapters may live under model-local `persistence` modules, while shared persistence
//! infrastructure should stay in `cloud_object_persistence`.

// Multiple modules contain `persistence` submodules; it is expected that
// code from the persistence modules is imported with fully-qualified paths.
#![allow(ambiguous_glob_reexports)]

pub mod agent_config_snapshot;
pub mod ai_execution_profile;
pub mod ai_fact;
pub mod env_vars;
pub mod folder;
pub mod json_model;
pub mod mcp;
pub mod notebook;
pub mod preference;
pub mod source_repo;
pub mod workflow;
pub mod workflow_enum;

pub use agent_config_snapshot::*;
pub use ai_execution_profile::*;
pub use ai_fact::*;
pub use env_vars::*;
pub use folder::*;
pub use json_model::*;
pub use mcp::*;
pub use notebook::*;
pub use preference::*;
pub use source_repo::*;
pub use workflow::*;
pub use workflow_enum::*;
