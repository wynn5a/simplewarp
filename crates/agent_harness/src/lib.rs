//! Agent-loop policy for SimpleWarp's agent mode.
//!
//! The app runs the loop and `local_inference` calls the model; this crate decides things about
//! the conversation, as plain functions over plain data. It has no UI, no I/O and no network, so
//! every rule here is unit-tested in isolation.

pub mod budget;
