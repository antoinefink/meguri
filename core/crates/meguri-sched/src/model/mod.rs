//! Lossless scheduling views; storage writes and operation effects remain
//! with the callers (sched/card-data, sched/presets, sched/config-keys).
pub mod card_data;
pub mod card_sched;
pub mod deck_sched;
pub mod position;
pub mod preset;
pub mod sched_config;
