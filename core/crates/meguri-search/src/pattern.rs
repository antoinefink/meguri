//! The expressions terms build from their values: glob conversion and its
//! wildcard test (`specs/search/text-matching.md` T13), the exact text of a
//! name (`names-and-ids.md` N1), the combining-free transform (T8.1) and
//! compiling an expression at plan time (T11.1, T12).

use std::borrow::Cow;

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::error::{ErrorKind, SearchFailure};

/// One unit of T13.1: a backslash with the character after it, or one
/// character (a lone trailing backslash is `Escaped(None)`).
enum Unit {
    Escaped(Option<char>),
    Plain(char),
}

fn units(text: &str) -> impl Iterator<Item = Unit> + '_ {
    let mut chars = text.chars();
    std::iter::from_fn(move || {
        let c = chars.next()?;
        Some(if c == '\\' { Unit::Escaped(chars.next()) } else { Unit::Plain(c) })
    })
}

/// What `*` and `_` become in a glob conversion.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wildcards {
    pub any_run: &'static str,
    pub any_one: &'static str,
}

impl Wildcards {
    /// T13.1.
    pub(crate) const TEXT: Wildcards = Wildcards {
        any_run: ".*",
        any_one: ".",
    };
    /// N2.2 step 4: wildcards never cross White_Space.
    pub(crate) const TAG: Wildcards = Wildcards {
        any_run: r"\S*",
        any_one: r"\S",
    };
}

/// T13.1: `text` as an expression that matches what the glob matches.
pub(crate) fn glob_to_regex(text: &str, wildcards: Wildcards) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for unit in units(text) {
        match unit {
            Unit::Escaped(Some('\\')) | Unit::Escaped(None) => out.push_str(r"\\"),
            Unit::Escaped(Some('*')) => out.push_str(r"\*"),
            Unit::Escaped(Some('_')) => out.push('_'),
            Unit::Escaped(Some(other)) => {
                out.push_str(r"\\");
                out.push_str(&regex::escape(other.encode_utf8(&mut [0; 4])));
            }
            Unit::Plain('*') => out.push_str(wildcards.any_run),
            Unit::Plain('_') => out.push_str(wildcards.any_one),
            Unit::Plain(other) => out.push_str(&regex::escape(other.encode_utf8(&mut [0; 4]))),
        }
    }
    out
}

/// T13.2: a lone `*` or `_` (after an even number of backslashes).
pub(crate) fn has_wildcards(text: &str) -> bool {
    units(text).any(|unit| matches!(unit, Unit::Plain('*' | '_')))
}

/// N1's exact text: each backslash removed together with its escaping role
/// (`\\` is `\`, `\*` is `*`, `\_` is `_`).
pub(crate) fn exact_text(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for unit in units(text) {
        match unit {
            Unit::Escaped(Some(c)) | Unit::Plain(c) => out.push(c),
            Unit::Escaped(None) => out.push('\\'),
        }
    }
    Cow::Owned(out)
}

/// T8.1: NFKD, without marks, then T8.2's replacements.
pub fn combining_free(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.nfkd().filter(|c| !is_combining_mark(*c)) {
        match replacement(c) {
            Some(text) => out.push_str(text),
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// T8.2.
fn replacement(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{20AC}' => "E",
        '\u{00C6}' => "AE",
        '\u{00D0}' => "D",
        '\u{00D8}' => "O",
        '\u{00DE}' => "TH",
        '\u{00DF}' => "s",
        '\u{00E6}' => "ae",
        '\u{00F0}' => "d",
        '\u{00F8}' => "o",
        '\u{00FE}' => "th",
        '\u{0110}' => "D",
        '\u{0111}' => "d",
        '\u{0126}' => "H",
        '\u{0127}' => "h",
        '\u{0131}' => "i",
        '\u{0138}' => "k",
        '\u{0141}' => "L",
        '\u{0142}' => "l",
        '\u{014A}' => "N",
        '\u{014B}' => "n",
        '\u{0152}' => "OE",
        '\u{0153}' => "oe",
        '\u{0166}' => "T",
        '\u{0167}' => "t",
        '\u{018F}' => "E",
        '\u{01DD}' => "e",
        '\u{0251}' => "a",
        _ => return None,
    })
}

/// T11.1, T12: compiles `expression` as every search expression is compiled.
/// `pattern` is T16's user text for the messages. A syntax error is
/// `invalid_regex`, a compiled form over the size limit `search_failed`.
pub(crate) fn compile(expression: &str, pattern: &str) -> Result<regex::Regex, ErrorKind> {
    regex::Regex::new(expression).map_err(|error| match error {
        regex::Error::CompiledTooBig(_) => ErrorKind::SearchFailed(SearchFailure::ExpressionTooLarge {
            pattern: pattern.to_owned(),
        }),
        regex::Error::Syntax(text) => ErrorKind::InvalidRegex {
            pattern: pattern.to_owned(),
            reason: reason(&text),
        },
        // The enum is non-exhaustive; no other error is known.
        other => ErrorKind::InvalidRegex {
            pattern: pattern.to_owned(),
            reason: other.to_string(),
        },
    })
}

/// T16's `{reason}`: after `error: ` on the last line that starts with it.
fn reason(text: &str) -> String {
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix("error: "))
        .unwrap_or(text)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_convert_by_unit() {
        for (text, expression) in [
            ("a*b", "a.*b"),
            ("a_b", "a.b"),
            (r"a\*b", r"a\*b"),
            (r"a\_b", "a_b"),
            (r"a\\b", r"a\\b"),
            (r"a\", r"a\\"),
            ("c++", r"c\+\+"),
            ("a.b", r"a\.b"),
            (r"\x", r"\\x"),
            (r"\.", r"\\\."),
            ("über", "über"),
        ] {
            assert_eq!(glob_to_regex(text, Wildcards::TEXT), expression, "{text:?}");
        }
        assert_eq!(glob_to_regex("x*i_", Wildcards::TAG), r"x\S*i\S");
    }

    #[test]
    fn wildcard_units_are_unescaped() {
        for (text, wild) in [
            ("a*", true),
            ("_", true),
            (r"a\*", false),
            (r"a\_", false),
            (r"a\\*", true),
            (r"a\\\*", false),
            ("abc", false),
            ("", false),
        ] {
            assert_eq!(has_wildcards(text), wild, "{text:?}");
        }
    }

    #[test]
    fn exact_text_drops_escaping_backslashes() {
        assert_eq!(exact_text(r"x\_y"), "x_y");
        assert_eq!(exact_text(r"a\*b\\c"), r"a*b\c");
        assert_eq!(exact_text("plain"), "plain");
    }

    #[test]
    fn combining_free_folds_compatibility_forms_and_the_table() {
        for (text, folded) in [
            ("Übung", "Ubung"),
            ("ｆｕｌｌ", "full"),
            ("ﬁ", "fi"),
            ("\u{130}", "I"),
            ("Straße", "Strase"),
            ("þaw ayakkabı €xit", "thaw ayakkabi Exit"),
            ("\u{1E9E}", "\u{1E9E}"),
            ("ﾊﾞ", "ハ"),
            ("a％b", "a%b"),
        ] {
            assert_eq!(combining_free(text), folded, "{text:?}");
        }
    }

    #[test]
    fn compile_errors_are_t12s_kinds() {
        assert!(compile("(?i)a", "a").is_ok());
        assert_eq!(
            compile("(?i)(", "(").unwrap_err(),
            ErrorKind::InvalidRegex {
                pattern: "(".to_owned(),
                reason: "unclosed group".to_owned()
            }
        );
        let large = format!("(?i)^{}$", ".".repeat(30_000));
        assert_eq!(
            compile(&large, "p").unwrap_err(),
            ErrorKind::SearchFailed(SearchFailure::ExpressionTooLarge { pattern: "p".to_owned() })
        );
    }
}
