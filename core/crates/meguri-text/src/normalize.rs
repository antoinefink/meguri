//! Field save normalization (`specs/text/checksum-and-sort-field.md` C1) and
//! the Unicode primitives it shares with tags and media names
//! (`specs/text/unicode-primitives.md` U1, U3).

use std::borrow::Cow;

use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

/// U1: U+0000–U+001F and U+007F.
pub fn is_ascii_control(c: char) -> bool {
    c.is_ascii_control()
}

/// The text without the characters `drop` selects; borrowed when none is.
fn without(text: &str, drop: impl Fn(char) -> bool) -> Cow<'_, str> {
    if text.chars().any(&drop) {
        Cow::Owned(text.chars().filter(|&c| !drop(c)).collect())
    } else {
        Cow::Borrowed(text)
    }
}

/// U1: removes every ASCII control character (tag canonicalization, R891).
pub fn remove_ascii_controls(text: &str) -> Cow<'_, str> {
    without(text, is_ascii_control)
}

/// U3: NFC (`unicode-normalization` 0.1.24, Unicode 16.0.0).
pub fn nfc(text: &str) -> Cow<'_, str> {
    match is_nfc_quick(text.chars()) {
        IsNormalized::Yes => Cow::Borrowed(text),
        IsNormalized::No | IsNormalized::Maybe => {
            let composed: String = text.nfc().collect();
            if composed == text {
                Cow::Borrowed(text)
            } else {
                Cow::Owned(composed)
            }
        }
    }
}

/// C1: what a note save stores for a field. ASCII control characters other
/// than TAB and LF are removed, then the text is NFC-normalized unless the
/// collection's `normalize_note_text` is false.
pub fn field_as_saved(field: &str, normalize_note_text: bool) -> Cow<'_, str> {
    let kept = without(field, |c| is_ascii_control(c) && c != '\t' && c != '\n');
    if !normalize_note_text {
        return kept;
    }
    match kept {
        Cow::Borrowed(text) => nfc(text),
        Cow::Owned(text) => Cow::Owned(nfc(&text).into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_removes_controls_but_tab_and_lf() {
        assert_eq!(field_as_saved("line\r\nbreak", true), "line\nbreak");
        assert_eq!(field_as_saved("a\u{1}b\u{7f}c\u{1f}d\u{b}e\u{c}f", true), "abcdef");
        assert_eq!(field_as_saved("\t5\n", true), "\t5\n");
        assert_eq!(field_as_saved("\u{0}", false), "");
        // U+0080 and U+0085 are not ASCII controls.
        assert_eq!(field_as_saved("a\u{80}\u{85}", true), "a\u{80}\u{85}");
    }

    #[test]
    fn saving_composes_to_nfc_unless_switched_off() {
        assert_eq!(field_as_saved("e\u{301}", true), "\u{e9}");
        assert_eq!(field_as_saved("e\u{301}", false), "e\u{301}");
        assert_eq!(field_as_saved("\u{1100}\u{1161}", true), "\u{ac00}");
        // Removing a control can join a base and a combining mark.
        assert_eq!(field_as_saved("e\u{1}\u{301}", true), "\u{e9}");
        assert!(matches!(field_as_saved("plain", true), Cow::Borrowed(_)));
    }

    #[test]
    fn controls_are_the_ascii_ones() {
        assert!(is_ascii_control('\u{0}') && is_ascii_control('\t') && is_ascii_control('\u{7f}'));
        assert!(!is_ascii_control(' ') && !is_ascii_control('\u{80}') && !is_ascii_control('\u{2028}'));
        assert_eq!(remove_ascii_controls("a\tb\nc\u{7f}"), "abc");
        assert_eq!(nfc("A\u{30a}"), "\u{c5}");
    }
}
