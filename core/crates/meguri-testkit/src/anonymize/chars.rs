//! Character replacement (`specs/fixtures/anonymizer.md` A5): which
//! characters are replaced, the pool each draws from, the guards, and the
//! entity grammar of `text/strip-html#H3` judged at one position.

use std::sync::LazyLock;

use regex::Regex;

use super::keys::Stream;

/// A5.1 for characters outside ASCII: general category L, M or N as the
/// pinned `regex` crate defines them.
static LETTER_MARK_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[\p{L}\p{M}\p{N}]\z").expect("A5.1 class"));
/// A5.2: the upper- and title-case letters of the 2-byte pools.
static UPPER_OR_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{Lu}\p{Lt}]\z").expect("A5.2 class"));

/// A5.1: whether `c` is replaced (a letter, mark or number other than a
/// variation selector).
pub fn is_replaced(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    if matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}') {
        return false;
    }
    let mut buf = [0u8; 4];
    LETTER_MARK_NUMBER.is_match(c.encode_utf8(&mut buf))
}

/// A5.2: the pool a replaced character draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Lower,
    Upper,
    Digit,
    Hiragana,
    Katakana,
    Han,
    Hangul,
    /// Any other 2-byte character of category Lu or Lt.
    Upper2,
    /// Any other 2-byte character.
    Lower2,
    /// Any other 3-byte character (drawn from the Han pool).
    Other3,
    /// Any 4-byte character.
    Other4,
}

const POOL_LOWER: &[(char, char)] = &[('a', 'z')];
const POOL_UPPER: &[(char, char)] = &[('A', 'Z')];
const POOL_DIGIT: &[(char, char)] = &[('0', '9')];
const POOL_HIRAGANA: &[(char, char)] = &[('\u{3041}', '\u{3096}')];
const POOL_KATAKANA: &[(char, char)] = &[('\u{30A1}', '\u{30FA}')];
const POOL_HAN: &[(char, char)] = &[('\u{4E00}', '\u{9FA5}')];
const POOL_HANGUL: &[(char, char)] = &[('\u{AC00}', '\u{D7A3}')];
const POOL_UPPER2: &[(char, char)] = &[('\u{C0}', '\u{D6}'), ('\u{D8}', '\u{DE}')];
const POOL_LOWER2: &[(char, char)] = &[('\u{DF}', '\u{F6}'), ('\u{F8}', '\u{FF}')];
const POOL_OTHER4: &[(char, char)] = &[('\u{20000}', '\u{2A6D6}')];

impl Class {
    /// The class of a replaced character (A5.2's table, first row that
    /// applies).
    pub fn of(c: char) -> Class {
        match c {
            'a'..='z' => Class::Lower,
            'A'..='Z' => Class::Upper,
            '0'..='9' => Class::Digit,
            '\u{3041}'..='\u{309F}' => Class::Hiragana,
            '\u{30A1}'..='\u{30FF}' => Class::Katakana,
            '\u{3005}'
            | '\u{3007}'
            | '\u{3021}'..='\u{3029}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}' => Class::Han,
            '\u{AC00}'..='\u{D7A3}' => Class::Hangul,
            _ => match c.len_utf8() {
                2 => {
                    let mut buf = [0u8; 4];
                    if UPPER_OR_TITLE.is_match(c.encode_utf8(&mut buf)) {
                        Class::Upper2
                    } else {
                        Class::Lower2
                    }
                }
                3 => Class::Other3,
                _ => Class::Other4,
            },
        }
    }

    fn pool(self) -> &'static [(char, char)] {
        match self {
            Class::Lower => POOL_LOWER,
            Class::Upper => POOL_UPPER,
            Class::Digit => POOL_DIGIT,
            Class::Hiragana => POOL_HIRAGANA,
            Class::Katakana => POOL_KATAKANA,
            Class::Han | Class::Other3 => POOL_HAN,
            Class::Hangul => POOL_HANGUL,
            Class::Upper2 => POOL_UPPER2,
            Class::Lower2 => POOL_LOWER2,
            Class::Other4 => POOL_OTHER4,
        }
    }

    /// Whether `c` is in this class's pool (V1).
    pub fn pool_contains(self, c: char) -> bool {
        self.pool().iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
    }

    /// One character drawn uniformly from the pool, without `excluded`
    /// (the guards' letter).
    pub fn draw(self, stream: &mut Stream, excluded: Option<char>) -> char {
        let pool = self.pool();
        let size: u32 = pool.iter().map(|&(lo, hi)| hi as u32 - lo as u32 + 1).sum();
        let mut index = stream.choose(size - u32::from(excluded.is_some()));
        if let Some(skip) = excluded {
            // `excluded` is in the pool's only range (a-z): every index from
            // its position on moves up by one.
            if index >= skip as u32 - pool[0].0 as u32 {
                index += 1;
            }
        }
        for &(lo, hi) in pool {
            let len = hi as u32 - lo as u32 + 1;
            if index < len {
                return char::from_u32(lo as u32 + index).expect("pools hold scalar values");
            }
            index -= len;
        }
        unreachable!("the index is below the pool's size")
    }
}

/// A5.4 G1 and G2: the letter a lower-case replacement may not be, given
/// the output before it.
pub fn guard_letter(class: Class, before: &str) -> Option<char> {
    if class != Class::Lower {
        None
    } else if before.ends_with("{{") {
        Some('c')
    } else if before.ends_with("[soun") {
        Some('d')
    } else {
        None
    }
}

/// A5.3: the entity starting at the beginning of `text` (which starts with
/// `&`), judged at that position alone: its length in bytes and its
/// character, or `None` when `&` does not start a valid entity
/// (`text/strip-html#H3`'s grammar, decided by the pinned `htmlescape`).
pub fn entity_at(text: &str) -> Option<(usize, char)> {
    let bytes = text.as_bytes();
    debug_assert_eq!(bytes.first(), Some(&b'&'));
    let body_start = 1;
    let mut i = body_start;
    if bytes.get(i) == Some(&b'#') {
        i += 1;
        let hex = bytes.get(i) == Some(&b'x');
        if hex {
            i += 1;
        }
        let digits = i;
        while bytes
            .get(i)
            .is_some_and(|b| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() })
        {
            i += 1;
        }
        if i == digits {
            return None;
        }
    } else {
        // HTML 4 names are ASCII letters and digits, at most 8 of them.
        while i - body_start <= 8 && bytes.get(i).is_some_and(u8::is_ascii_alphanumeric) {
            i += 1;
        }
        if i == body_start {
            return None;
        }
    }
    if bytes.get(i) != Some(&b';') {
        return None;
    }
    let candidate = &text[..=i];
    let decoded = htmlescape::decode_html(candidate).ok()?;
    let mut chars = decoded.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some((i + 1, c)),
        _ => None,
    }
}

/// Whether the whole of `text` is one valid entity (G3's test).
pub fn is_entity(text: &str) -> bool {
    text.starts_with('&') && entity_at(text).is_some_and(|(len, _)| len == text.len())
}

/// A5.4 G3: after an `&` that does not start a valid entity, the length of
/// the run of ASCII letters, digits and `#` up to and including a `;`, when
/// there is one.
pub fn g3_run(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let run = bytes
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'#')
        .count();
    (run > 0 && bytes.get(run) == Some(&b';')).then_some(run + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_characters_are_letters_marks_and_numbers() {
        for c in [
            'a', 'Z', '7', 'é', 'ß', '\u{301}', '日', 'に', 'カ', 'ー', '한', '٣', '𠀀', 'Ω',
        ] {
            assert!(is_replaced(c), "{c:?}");
        }
        for c in [
            ' ',
            '\t',
            '\n',
            '{',
            ':',
            '&',
            ';',
            '\u{a0}',
            '\u{3000}',
            '・',
            '゛',
            '😀',
            '\u{200d}',
            '\u{fe0f}',
            '\u{e0100}',
            '\u{e000}',
            '€',
        ] {
            assert!(!is_replaced(c), "{c:?}");
        }
    }

    #[test]
    fn classes_follow_the_pool_table() {
        assert_eq!(Class::of('q'), Class::Lower);
        assert_eq!(Class::of('Q'), Class::Upper);
        assert_eq!(Class::of('4'), Class::Digit);
        assert_eq!(Class::of('に'), Class::Hiragana);
        assert_eq!(Class::of('ゟ'), Class::Hiragana);
        assert_eq!(Class::of('ー'), Class::Katakana);
        assert_eq!(Class::of('々'), Class::Han);
        assert_eq!(Class::of('語'), Class::Han);
        assert_eq!(Class::of('한'), Class::Hangul);
        assert_eq!(Class::of('É'), Class::Upper2);
        assert_eq!(Class::of('Ω'), Class::Upper2);
        assert_eq!(Class::of('ǅ'), Class::Upper2);
        assert_eq!(Class::of('é'), Class::Lower2);
        assert_eq!(Class::of('\u{301}'), Class::Lower2);
        assert_eq!(Class::of('ก'), Class::Other3);
        assert_eq!(Class::of('𠀀'), Class::Other4);
    }

    #[test]
    fn pools_keep_byte_length_and_nfc() {
        for class in [
            Class::Lower,
            Class::Upper,
            Class::Digit,
            Class::Hiragana,
            Class::Katakana,
            Class::Han,
            Class::Hangul,
            Class::Upper2,
            Class::Lower2,
            Class::Other3,
            Class::Other4,
        ] {
            for &(lo, hi) in class.pool() {
                let len = lo.len_utf8();
                for c in lo..=hi {
                    assert_eq!(c.len_utf8(), len, "{c:?}");
                    let s = c.to_string();
                    assert_eq!(meguri_text::normalize::nfc(&s), s, "{c:?} is not NFC-stable");
                    assert!(is_replaced(c), "{c:?} is not a letter");
                }
            }
        }
    }

    #[test]
    fn entities_are_judged_where_they_start() {
        assert_eq!(entity_at("&amp; x"), Some((5, '&')));
        assert_eq!(entity_at("&eacute;"), Some((8, 'é')));
        assert_eq!(entity_at("&#20320;"), Some((8, '你')));
        assert_eq!(entity_at("&#x2014;"), Some((8, '—')));
        assert_eq!(entity_at("&#0;"), Some((4, '\0')));
        assert_eq!(entity_at("&sup2;"), Some((6, '²')));
        for text in [
            "&",
            "& amp;",
            "&;",
            "&#;",
            "&#x;",
            "&#X41;",
            "&#55296;",
            "&#1114112;",
            "&AMP;",
            "&apos;",
            "&amp b;",
            "&amp",
            "&unknown;",
        ] {
            assert_eq!(entity_at(text), None, "{text:?}");
        }
        assert!(is_entity("&lt;"));
        assert!(!is_entity("&lt;x"));
        assert!(!is_entity("&qq;"));
    }

    #[test]
    fn g3_runs_end_at_a_semicolon() {
        assert_eq!(g3_run("foo; x"), Some(4));
        assert_eq!(g3_run("#12a;"), Some(5));
        assert_eq!(g3_run(";"), None);
        assert_eq!(g3_run("foo bar;"), None);
        assert_eq!(g3_run("foo"), None);
    }

    #[test]
    fn guards_name_the_letter_a_replacement_may_not_be() {
        assert_eq!(guard_letter(Class::Lower, "x {{"), Some('c'));
        assert_eq!(guard_letter(Class::Lower, "[soun"), Some('d'));
        assert_eq!(guard_letter(Class::Upper, "{{"), None);
        assert_eq!(guard_letter(Class::Lower, "{"), None);
    }
}
