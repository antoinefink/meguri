//! Composition root: `CoreHost::open` opens a collection and wires every
//! engine port. The only crate that calls the engine's raw open entry point.

mod config;
mod open;
pub mod wiring;

pub use config::HostConfig;
pub use open::{CoreHost, InitialImportCompletion, ProcessCore};
