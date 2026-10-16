//! Oracle-side transforms of known deviations (dsl.md D13). Add a transform
//! here, in `suite::Transform`, in deviations.schema.json and in the spec
//! together.

use serde_json::Value;

use super::pointer;
use crate::suite::{Deviation, Transform};

/// Applies the deviation's transform to every matching pointer of the expected
/// value; true when its pointer matched. `apple_overlay` needs the
/// scenario's overlay, so `diff::compare` applies it (`observe::overlay`);
/// here it changes nothing.
pub fn apply(deviation: &Deviation, expected: &mut Value) -> bool {
    let Transform::Replace(replacement) = &deviation.transform else {
        return false;
    };
    let pattern = pointer::parse(&deviation.pointer);
    let mut matched = false;
    pointer::for_each_match(expected, &pattern, &mut |value| {
        matched = true;
        *value = replacement.clone();
    });
    matched
}
