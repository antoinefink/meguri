//! T1/T13: the pinned crate owns parameter conversion and clipping. Cache
//! exact input bits; do not cache a fallback model after a refusal.
use crate::model::preset::PresetView;
use fsrs::FSRS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedParams;
impl std::fmt::Display for UnsupportedParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported FSRS parameters")
    }
}
impl std::error::Error for UnsupportedParams {}

#[derive(Default, Debug)]
pub struct FsrsCache {
    models: Vec<(Vec<u32>, FSRS)>,
}

impl FsrsCache {
    pub fn model(&mut self, preset: &PresetView) -> Result<&FSRS, UnsupportedParams> {
        let params = preset.parameters();
        if preset.has_later_fsrs_parameters()
            || !matches!(params.len(), 0 | 17 | 19 | 21)
            || params.iter().any(|p| !p.is_finite())
        {
            return Err(UnsupportedParams);
        }
        if let Some(index) = self
            .models
            .iter()
            .position(|(bits, _)| bits.len() == params.len() && bits.iter().zip(params).all(|(a, b)| *a == b.to_bits()))
        {
            self.models.swap(0, index);
        } else {
            let model = FSRS::new(params).map_err(|_| UnsupportedParams)?;
            if self.models.len() == 8 {
                self.models.pop();
            }
            self.models
                .insert(0, (params.iter().map(|p| p.to_bits()).collect(), model));
        }
        Ok(&self.models[0].1)
    }
}
