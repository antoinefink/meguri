//! C10: what a cloze type-in field expects.

use super::{Side, render};

/// C10: the answer `{{type:cloze:F}}` expects on card `card` when F holds
/// `text`: empty with no active cloze, the single answer when all active
/// clozes' plain contents are identical, else all of them joined by `, `
/// with duplicates kept (R977). HTML is kept as written; U64 compares.
pub fn type_answer(text: &str, card: u32) -> String {
    let answers = render::entries(text, card, Side::Back);
    match answers.split_first() {
        None => String::new(),
        Some((first, rest)) if rest.iter().all(|answer| answer == first) => first.clone(),
        Some(_) => answers.join(", "),
    }
}
