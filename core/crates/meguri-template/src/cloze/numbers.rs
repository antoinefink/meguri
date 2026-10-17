//! C5: the cloze numbers of a text, and top-level presence.

use std::collections::BTreeSet;

use super::tokenizer::{self, Item};

/// C5: the numbers other than 0 of every cloze in `text`, nested clozes
/// included. Card generation (U27, AE33) and the card picker take the union
/// over a note's fields; ordinal caps are theirs.
pub fn numbers(text: &str) -> BTreeSet<u16> {
    let mut numbers = BTreeSet::new();
    tokenizer::for_each_cloze(&tokenizer::parse(text), &mut |cloze| {
        numbers.extend(cloze.numbers.iter().copied().filter(|&n| n != 0));
    });
    numbers
}

/// C5, top-level presence: whether a top-level cloze of `text` has a number
/// other than 0. Nested clozes are not looked at. U27's note field check
/// uses this test.
pub fn has_cloze(text: &str) -> bool {
    tokenizer::parse(text).iter().any(|item| match item {
        Item::Cloze(cloze) => cloze.numbers.iter().any(|&n| n != 0),
        Item::Text(_) => false,
    })
}
