//! Tag names (`specs/tags/tags.md` T1, T2): splitting tag text, the
//! components and ancestors of a name, the normalization every name goes
//! through before it enters the registry or a note, the stored form of a
//! note's tag list (T4 step 5) and the refusal of a new name (T7 step 1).

use std::fmt;

use meguri_text::normalize::{is_ascii_control, nfc};

/// The component separator of a hierarchical name.
pub const LEVEL: &str = "::";

/// What a component left empty becomes (T2 step 4).
pub const BLANK: &str = "blank";

/// T1: the separators, which a stored tag never contains.
pub fn is_separator(c: char) -> bool {
    matches!(c, '\u{20}' | '\u{3000}')
}

/// T1's trim set: the White_Space characters T2 step 2 has not removed.
fn in_trim_set(c: char) -> bool {
    matches!(
        c,
        '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}'
    )
}

/// T1's white space, for T7's refusal: the trim set, U+0009 to U+000D and
/// the separators.
pub fn is_white_space(c: char) -> bool {
    in_trim_set(c) || matches!(c, '\u{9}'..='\u{d}') || is_separator(c)
}

/// T1 tag text: the pieces between separators, empty ones dropped.
pub fn split_text(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_separator).filter(|piece| !piece.is_empty())
}

/// T1: a name's components, split at each `::` from the left without
/// overlap (`a:::b` gives `a`, `:b`).
pub fn components(name: &str) -> std::str::Split<'_, &'static str> {
    name.split(LEVEL)
}

/// The byte offsets of the `::` between a name's components, found from the
/// left without overlap (searching from the right would split `a:::b`
/// elsewhere).
fn levels(name: &str) -> Vec<usize> {
    name.match_indices(LEVEL).map(|(at, _)| at).collect()
}

/// T1: a name's ancestors from its parent to its first component.
pub fn ancestors(name: &str) -> impl Iterator<Item = &str> {
    levels(name).into_iter().rev().map(move |at| &name[..at])
}

/// A name's first component.
pub fn first_component(name: &str) -> &str {
    name.find(LEVEL).map_or(name, |at| &name[..at])
}

/// A name's last component.
pub fn last_component(name: &str) -> &str {
    levels(name).last().map_or(name, |&at| &name[at + LEVEL.len()..])
}

/// T2: NFC, then ASCII controls and separators removed, then the trim set
/// removed from the ends, component by component; an empty component
/// becomes `blank`. The result is never empty.
pub fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, component) in components(name).enumerate() {
        if i > 0 {
            out.push_str(LEVEL);
        }
        let composed = nfc(component);
        let kept: String = composed
            .chars()
            .filter(|&c| !is_ascii_control(c) && !is_separator(c))
            .collect();
        let trimmed = kept.trim_matches(in_trim_set);
        out.push_str(if trimmed.is_empty() { BLANK } else { trimmed });
    }
    out
}

/// T4 step 5 (and T6 step 3): a space, the tags joined by single spaces and
/// a space; the empty string for no tags.
pub fn join_note_tags<S: AsRef<str>>(tags: &[S]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let mut out = String::from(" ");
    for tag in tags {
        out.push_str(tag.as_ref());
        out.push(' ');
    }
    out
}

/// Why T7 refuses a new name: refusal kind `invalid_name`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidName;

impl InvalidName {
    /// The refusal kind (`specs/conformance/tag-ops.md` G4).
    pub const KIND: &'static str = "invalid_name";
}

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid_name: a tag name may not be empty, only white space, or hold a space")
    }
}

/// T7 step 1: a new name with a separator, or empty or made only of white
/// space, is refused. Other control characters are not refused: T2 removes
/// them.
pub fn check_new_name(name: &str) -> Result<(), InvalidName> {
    if name.chars().any(is_separator) || name.chars().all(is_white_space) {
        Err(InvalidName)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_follows_the_t2_table() {
        for (input, normalized) in [
            ("x\ty", "xy"),
            ("a b", "ab"),
            ("a::::b", "a::blank::b"),
            ("a:::b", "a:::b"),
            ("foo::", "foo::blank"),
            ("::", "blank::blank"),
            ("::lead", "blank::lead"),
            ("\u{a0}q\u{a0}", "q"),
            ("r\u{a0}s", "r\u{a0}s"),
            ("\u{a0}", "blank"),
            ("\u{85}p\u{85}", "p"),
            ("\u{2028}z", "z"),
            ("t\u{200b}u", "t\u{200b}u"),
            ("e\u{301}", "\u{e9}"),
            ("e\u{1}\u{301}", "e\u{301}"),
            ("A\u{30a}\u{2}", "\u{c5}"),
            ("\u{1}", "blank"),
            ("k::\u{1}::m", "k::blank::m"),
            ("say\"hi\"", "say\"hi\""),
            ("a\u{3000}b", "ab"),
            ("\u{7f}", "blank"),
        ] {
            assert_eq!(normalize(input), normalized, "{input:?}");
        }
    }

    #[test]
    fn components_split_from_the_left_without_overlap() {
        assert_eq!(components("a::b").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(components("a:::b").collect::<Vec<_>>(), ["a", ":b"]);
        assert_eq!(components("a::::b").collect::<Vec<_>>(), ["a", "", "b"]);
        assert_eq!(components("::").collect::<Vec<_>>(), ["", ""]);
        assert_eq!(ancestors("a::b::c").collect::<Vec<_>>(), ["a::b", "a"]);
        assert_eq!(ancestors("a:::b").collect::<Vec<_>>(), ["a"]);
        assert_eq!(ancestors("a").count(), 0);
        assert_eq!((first_component("a::b::c"), last_component("a::b::c")), ("a", "c"));
        assert_eq!((first_component("a:::b"), last_component("a:::b")), ("a", ":b"));
        assert_eq!((first_component("solo"), last_component("solo")), ("solo", "solo"));
    }

    #[test]
    fn tag_text_splits_at_both_separators() {
        assert_eq!(split_text(" a\u{3000}b  c\td ").collect::<Vec<_>>(), ["a", "b", "c\td"]);
        assert_eq!(split_text("\u{3000} ").count(), 0);
        assert_eq!(join_note_tags(&["A", "b"]), " A b ");
        assert_eq!(join_note_tags::<&str>(&[]), "");
    }

    #[test]
    fn new_names_with_a_separator_or_only_white_space_are_refused() {
        for refused in ["has space", "", "   ", "a\u{3000}b", "\t", "\u{a0}\u{2028}", "\u{d}"] {
            assert_eq!(check_new_name(refused), Err(InvalidName), "{refused:?}");
        }
        for accepted in ["x\ty", "b::", "\u{1}", "a\u{a0}b", "c++"] {
            assert_eq!(check_new_name(accepted), Ok(()), "{accepted:?}");
        }
    }
}
