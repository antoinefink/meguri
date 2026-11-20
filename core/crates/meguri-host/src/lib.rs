//! Composition root: `CoreHost::open` opens a collection and wires every
//! engine port. The only crate that calls the engine's raw open entry point.

mod cloud_media;
mod cloud_policy;
mod collection_export;
mod manual_transfers;
mod provider;
mod provider_bootstrap;
mod provider_normal;
mod provider_setup;
pub use provider_setup::{MeguriSetupChange, MeguriSetupChoice, MeguriSetupDecision, MeguriSetupRow, MeguriSetupSettlement, MeguriSetupSide};
pub use provider_normal::{MeguriCollectionState, MeguriCollectionResult, MeguriMediaResult, MeguriRoundRecovery, MeguriRoundResult, MeguriRoundStage, MeguriRoundStart, MeguriRoundTask};
pub use provider_bootstrap::{MeguriAccess, MeguriBootstrapDirection, MeguriBootstrapError};
pub use provider::ProviderHandoff;
pub use manual_transfers::ManualTransferTask;
mod config;
pub use collection_export::{
    CollectionExportRemoval, WrittenCollectionExport, collection_export_provider_state, remove_collection_export,
};
mod open;
pub mod wiring;

pub use config::HostConfig;
pub use open::{CoreHost, InitialImportCompletion, ProcessCore};
