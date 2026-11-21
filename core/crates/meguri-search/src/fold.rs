//! Unicode 16.0 simple case folding (`specs/search/case-folding.md` C3):
//! one character to one, so a folded text has as many characters as its
//! text, and two characters are equal under the `regex` crate's
//! case-insensitive mode exactly when their foldings are equal. Suggestions
//! match names with it (`editing-services` D6.4, `tags/complete` C3), and
//! the tag and deck counts compare names as the terms' `(?i)` expressions
//! do (`filter-tokens` T12.4).

use icu_casemap::CaseMapper;

/// Each character of `text` folded.
pub(crate) fn simple_fold(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    let mapper = CaseMapper::new();
    text.chars().map(|c| mapper.simple_fold(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::simple_fold;

    #[test]
    fn simple_folding_maps_one_character_to_one() {
        for (text, folded) in [
            ("ABC", "abc"),
            ("Über", "über"),
            ("STRAẞE", "straße"),
            ("Straße", "straße"),
            ("ΣΟΦΊΑ", "σοφία"),
            ("ς", "σ"),
            ("\u{212A}", "k"),
            ("Ｇｒａ", "ｇｒａ"),
        ] {
            assert_eq!(simple_fold(text), folded, "{text:?}");
        }
    }

    // The `regex` crate's case-insensitive mode agrees: a text matches its
    // folding and the folding of every other spelling.
    #[test]
    fn folding_agrees_with_case_insensitive_expressions() {
        for (a, b) in [("Über", "üBER"), ("\u{212A}elvin", "kELVIN"), ("ΣΟΦΟΣ", "σοφος"), ("STRAẞE", "straße")] {
            assert_eq!(simple_fold(a), simple_fold(b), "{a:?} {b:?}");
            let expression = regex::Regex::new(&format!("(?i)^{}$", regex::escape(a))).unwrap();
            assert!(expression.is_match(b), "{a:?} {b:?}");
        }
        let expression = regex::Regex::new("(?i)^straße$").unwrap();
        assert!(!expression.is_match("strasse"));
        assert_ne!(simple_fold("straße"), simple_fold("strasse"));
    }
}
