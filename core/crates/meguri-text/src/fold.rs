//! The `unicase` comparison (`specs/text/unicode-primitives.md` U2): the
//! collation every SQLite connection registers (R1451, U22).

use std::cmp::Ordering;

use unicase::UniCase;

/// U2: compares the full case foldings of `a` and `b` (the pinned `unicase`
/// 2.6.0 table, quirks included) code point by code point.
pub fn compare_folded(a: &str, b: &str) -> Ordering {
    UniCase::new(a).cmp(&UniCase::new(b))
}

#[cfg(test)]
mod tests {
    use super::compare_folded;
    use std::cmp::Ordering::{self, Equal, Greater, Less};

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
