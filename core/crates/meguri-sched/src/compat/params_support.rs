//! Policy consumes the same model constructor as answer states and recompute.
use crate::{model::preset::PresetView, states::fsrs_glue::FsrsCache};

pub fn supported(cache: &mut FsrsCache, preset: &PresetView) -> bool {
    cache.model(preset).is_ok()
}
