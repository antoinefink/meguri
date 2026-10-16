//! The `unicase` comparison (`specs/text/unicode-primitives.md` U2): the
//! collation every SQLite connection registers (R1451, U22).

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use unicase::UniCase;

/// U2: compares the full case foldings of `a` and `b` (the pinned `unicase`
/// 2.6.0 table, quirks included) code point by code point.
pub fn compare_folded(a: &str, b: &str) -> Ordering {
    UniCase::new(a).cmp(&UniCase::new(b))
}

/// U2's folding of `text` as one string: each character replaced by its
/// folding, in the order the comparison uses (a three-character folding as
/// third, first, second). Two texts compare equal exactly when their folds
/// are equal. These are the bytes `UniCase::new(text)` feeds a hasher.
pub fn folded(text: &str) -> String {
    /// Keeps the bytes a `Hash` implementation writes.
    struct Bytes(Vec<u8>);
    impl Hasher for Bytes {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes);
        }
    }
    let mut bytes = Bytes(Vec::with_capacity(text.len()));
    UniCase::new(text).hash(&mut bytes);
    // unicase writes each folded character as whole UTF-8 (ASCII text as its
    // lower-case bytes), so the bytes are UTF-8.
    String::from_utf8(bytes.0).expect("unicase hashes UTF-8 characters")
}

#[cfg(test)]
mod tests {
    use super::{compare_folded, folded};
    use std::cmp::Ordering::{self, Equal, Greater, Less};

    #[test]
    fn folds_are_the_characters_u2_compares() {
        for (text, fold) in [
            ("ABC", "abc"),
            ("Straße", "strasse"),
            ("STRASSE", "strasse"),
            ("\u{1e9e}", "ss"),
            ("\u{130}", "i\u{307}"),
            ("\u{fb03}", "iff"),
            ("\u{212a}", "k"),
            ("\u{131}", "\u{131}"),
            ("\u{a7c0}", "\u{a7c0}"),
            ("", ""),
            ("Photo.JPG", "photo.jpg"),
        ] {
            assert_eq!(folded(text), fold, "{text:?}");
        }
    }

    #[test]
    fn equal_folds_are_equal_comparisons() {
        let words = [
            "a", "B", "Straße", "STRASSE", "\u{130}", "i\u{307}", "i", "\u{fb03}", "iff", "ffi", "\u{131}", "I",
        ];
        for a in words {
            for b in words {
                assert_eq!(folded(a) == folded(b), compare_folded(a, b) == Equal, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn comparisons_match_the_spec_table() {
        let rows: [(&str, &str, Ordering); 11] = [
            ("a", "B", Less),
            ("Z", "[", Greater),
            ("_", "a", Less),
            ("Straße", "STRASSE", Equal),
            ("\u{130}", "i", Greater),
            ("\u{130}", "i\u{307}", Equal),
            ("\u{fb03}", "ffi", Greater),
            ("\u{fb03}", "iff", Equal),
            ("\u{a7c0}X", "\u{a7c1}x", Less),
            ("", "a", Less),
            ("\u{131}", "I", Greater),
        ];
        for (a, b, ord) in rows {
            assert_eq!(compare_folded(a, b), ord, "{a:?} vs {b:?}");
            assert_eq!(compare_folded(b, a), ord.reverse(), "{b:?} vs {a:?}");
        }
    }

    #[test]
    fn foldings_cover_the_named_cases() {
        assert_eq!(compare_folded("\u{1e9e}", "ss"), Equal);
        assert_eq!(compare_folded("\u{1c5}", "\u{1c6}"), Equal);
        assert_eq!(compare_folded("\u{212a}", "k"), Equal);
        assert_eq!(compare_folded("\u{212b}", "\u{e5}"), Equal);
        assert_eq!(compare_folded("\u{390}", "\u{301}\u{3b9}\u{308}"), Equal);
    }
}
