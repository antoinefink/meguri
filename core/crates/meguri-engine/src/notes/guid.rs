//! A random u64 encoded with text/guid G3; supplied GUIDs bypass this.
use meguri_model::rng::RandomSource;
pub fn new(random: &dyn RandomSource) -> String {
    meguri_text::base91::encode(random.next_u64())
}
