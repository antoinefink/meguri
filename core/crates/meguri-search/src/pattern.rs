//! The expressions terms build from their values: glob conversion and its
//! wildcard test (`specs/search/text-matching.md` T13), the exact text of a
//! name (`names-and-ids.md` N1), the combining-free transform (T8.1) and
//! compiling an expression at plan time (T11.1, T12); the wildcard patterns
//! of T3 and the cloze markup removal of T9.2.

use std::borrow::Cow;
use std::sync::LazyLock;

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

/// T3.3: the longest wildcard pattern, in UTF-8 bytes.
pub(crate) const LIKE_LIMIT: usize = 50_000;

/// T3.1: a value as a wildcard pattern, in one pass from left to right.
pub(crate) fn like_pattern(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'\\') => {
                chars.next();
                out.push_str(r"\\");
            }
            '\\' if chars.peek() == Some(&'*') => {
                chars.next();
                out.push('*');
            }
            '*' => out.push('%'),
            '%' => out.push_str(r"\%"),
            other => out.push(other),
        }
    }
    out
}

/// A wildcard pattern ready to test texts (T3.2).
#[derive(Debug)]
pub(crate) enum Like {
    /// A `\` at the very end: the pattern matches nothing.
    Never,
    Regex(regex::Regex),
}

impl Like {
    /// T3.2 with the fold mode of `case-folding.md` C2: Unicode simple
    /// folding (C3, the `regex` crate's `(?i)` table) or ASCII (C6).
    pub(crate) fn new(pattern: &str, unicode: bool) -> Result<Like, regex::Error> {
        enum Piece {
            Run,
            One,
            Char(char),
        }
        let mut pieces = Vec::with_capacity(pattern.len());
        let mut chars = pattern.chars();
        while let Some(c) = chars.next() {
            pieces.push(match c {
                '%' => {
                    if matches!(pieces.last(), Some(Piece::Run)) {
                        continue;
                    }
                    Piece::Run
                }
                '_' => Piece::One,
                '\\' => match chars.next() {
                    Some(escaped) => Piece::Char(escaped),
                    None => return Ok(Like::Never),
                },
                other => Piece::Char(other),
            });
        }
        // A leading or trailing run is the same as no anchor there.
        let start = usize::from(matches!(pieces.first(), Some(Piece::Run)));
        let end = if pieces.len() > start && matches!(pieces.last(), Some(Piece::Run)) {
            pieces.len() - 1
        } else {
            pieces.len()
        };
        let mut expression = String::from(if unicode { "(?si)" } else { "(?s)" });
        if start == 0 {
            expression.push('^');
        }
        let mut buf = [0; 4];
        for piece in &pieces[start..end] {
            match piece {
                Piece::Run => expression.push_str(".*"),
                Piece::One => expression.push('.'),
                Piece::Char(c) if !unicode && c.is_ascii_alphabetic() => {
                    expression.push('[');
                    expression.push(c.to_ascii_lowercase());
                    expression.push(c.to_ascii_uppercase());
                    expression.push(']');
                }
                Piece::Char(c) => expression.push_str(&regex::escape(c.encode_utf8(&mut buf))),
            }
        }
        if end == pieces.len() {
            expression.push('$');
        }
        // A pattern holds at most T3.3's 50,000 bytes, but one made of `_`
        // alone compiles past the default size limit, which is for the
        // user's expressions (T11.1), not for these.
        let regex = regex::RegexBuilder::new(&expression).size_limit(1 << 30).build()?;
        Ok(Like::Regex(regex))
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Like::Never => false,
            Like::Regex(regex) => regex.is_match(text),
        }
    }
}

/// T9.2's cloze opener: `{{c`, one or more decimal digits (Nd) or commas,
/// then `::`.
static CLOZE_OPENER: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\{\{c[\d,]+::").expect("the cloze opener compiles"));

/// T9.2: each cloze replaced by its answer, the hint cut off.
pub(crate) fn strip_cloze(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut copied = 0;
    while let Some(opener) = CLOZE_OPENER.find_at(text, copied) {
        let inside = &text[opener.end()..];
        // No `}}` after this opener means none after any later one either.
        let Some(close) = inside.find("}}") else {
            break;
        };
        let answer = &inside[..close];
        let answer = answer.find("::").map_or(answer, |cut| &answer[..cut]);
        out.push_str(&text[copied..opener.start()]);
        out.push_str(answer);
        copied = opener.end() + close + 2;
    }
    if copied == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
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

    // T3.1, and the examples under its table.
    #[test]
    fn values_become_wildcard_patterns_in_one_pass() {
        for (value, pattern) in [
            ("dog", "dog"),
            ("do*g", "do%g"),
            (r"a\*b", "a*b"),
            (r"a\\b", r"a\\b"),
            ("a%b", r"a\%b"),
            ("d_g", "d_g"),
            (r"a\_b", r"a\_b"),
            (r"\\*", r"\\%"),
            (r"a\", r"a\"),
        ] {
            assert_eq!(like_pattern(value), pattern, "{value:?}");
        }
    }

    fn like(pattern: &str, unicode: bool, text: &str) -> bool {
        Like::new(pattern, unicode).unwrap().is_match(text)
    }

    // T3.2 on its own and through T5.2's rows.
    #[test]
    fn wildcard_patterns_match_whole_texts() {
        for (pattern, text, matches) in [
            ("dog", "dog", true),
            ("dog", "hot dog", false),
            ("%dog%", "hot dog", true),
            ("d_g", "dig", true),
            ("d_g", "dg", false),
            ("%do%g%", "dosomethingg", true),
            (r"%a*b%", "a*b", true),
            (r"%a*b%", "axb", false),
            (r"%a\_b%", "a_b", true),
            (r"%a\_b%", "axb", false),
            (r"%a\\b%", r"a\b", true),
            (r"%a\%b%", "a%b", true),
            (r"%a\%b%", "axb", false),
            ("%dog%cat%", "dog\u{1f}cat", true),
            ("%dog_cat%", "dog\u{1f}cat", true),
            ("%dog cat%", "dog\u{1f}cat", false),
            ("%dog_", "dog\u{1f}", true),
            ("a%", "a\nb", true),
            ("_", "\u{1f600}", true),
            ("__", "\u{1f600}", false),
            (r"%\D%", "d", true),
            (r"%a\", "a%", false),
            ("", "", true),
            ("", "x", false),
            ("%", "", true),
        ] {
            for unicode in [false, true] {
                assert_eq!(like(pattern, unicode, text), matches, "{pattern:?} {text:?} {unicode}");
            }
        }
    }

    // C2, C3, C6: only Unicode mode folds beyond ASCII.
    #[test]
    fn the_fold_mode_decides_case_beyond_ascii() {
        for (pattern, text, ascii, unicode) in [
            ("DOG", "dog", true, true),
            ("über", "Über", false, true),
            ("ß", "\u{1e9e}", false, true),
            ("ß", "SS", false, false),
            ("ς", "Σ", false, true),
            ("ǆ", "ǅ", false, true),
            ("ａｂｃ", "ＡＢＣ", false, true),
            ("ａ", "A", false, false),
            ("k", "\u{212a}", false, true),
            ("s", "ſ", false, true),
            ("i", "İ", false, false),
            ("I", "ı", false, false),
            ("は", "ハ", false, false),
        ] {
            assert_eq!(like(pattern, false, text), ascii, "{pattern:?} {text:?} ascii");
            assert_eq!(like(pattern, true, text), unicode, "{pattern:?} {text:?} unicode");
        }
    }

    // T3.3: the longest patterns compile, `_` alone included, which passes
    // the default size limit. (An unanchored pattern that long costs a time
    // proportional to its length times the text's, as SQLite's LIKE does.)
    #[test]
    fn the_longest_patterns_compile() {
        let ones = "_".repeat(LIKE_LIMIT);
        assert!(like(&ones, true, &"x".repeat(LIKE_LIMIT)));
        assert!(!like(&ones, true, &"x".repeat(LIKE_LIMIT - 1)));
        let letters = "a".repeat(LIKE_LIMIT);
        assert!(like(&letters, true, &"A".repeat(LIKE_LIMIT)));
        assert!(!like(&format!("%{letters}%"), true, "short"));
    }

    // T9.2's table.
    #[test]
    fn clozes_become_their_answers() {
        for (text, removed) in [
            ("{{c1::capital}} city", "capital city"),
            ("{{c1::one::hint}} two", "one two"),
            ("{{c3::blue::two::parts::here}} end", "blue end"),
            ("{{c1::a:b::c}}", "a:b"),
            ("{{c4::green::}}{{c5::::empty answer}}stop", "greenstop"),
            ("{{c1,2::both}} {{c3::}}end", "both end"),
            ("a {{c2::b {{c1::c}} d}} e", "a b {{c1 d}} e"),
            ("{{c١::arabic}}", "arabic"),
            ("{{c1::x\u{1f}y}} z", "x\u{1f}y z"),
            ("{{C1::upper}}", "{{C1::upper}}"),
            ("{{c::none}}", "{{c::none}}"),
            ("{{c1:single}}", "{{c1:single}}"),
            ("{{c1::x", "{{c1::x"),
            ("{{{c1::x}}", "{x"),
            ("no cloze", "no cloze"),
        ] {
            assert_eq!(strip_cloze(text), removed, "{text:?}");
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
            ErrorKind::SearchFailed(SearchFailure::ExpressionTooLarge {
                pattern: "p".to_owned()
            })
        );
    }
}
