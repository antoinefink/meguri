//! G4–G5: all fields, distinct capped ordinals, no template dependency.
use meguri_model::note::Note;
use std::collections::BTreeSet;
pub fn required(note: &Note) -> BTreeSet<u16> {
    note.field_values()
        .flat_map(meguri_template::cloze::numbers)
        .map(|number| (number - 1).min(499))
        .collect()
}
