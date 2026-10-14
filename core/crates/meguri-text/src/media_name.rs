//! The media file name transform (`specs/text/media-filenames.md` F1). The
//! media store builds on it (U31): lowercasing, deduplication and conflict
//! renames are its business, not this module's.

use crate::normalize::nfc;
use crate::unicode10_unassigned::UNASSIGNED;

/// F1 step 6: the longest name kept, in UTF-8 bytes.
const MAX_NAME_BYTES: usize = 120;
/// F1 step 6.2: the longest extension kept, in UTF-8 bytes.
const MAX_EXTENSION_BYTES: usize = 10;

/// F1 step 4, ASCII case-insensitive.
const DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// F1: the name a desired media file name is stored under. It is at most 120
/// bytes and may be empty (`???`), which the media store must refuse.
pub fn safe_media_name(desired: &str) -> String {
    let mut name: String = nfc(desired)
        .chars()
        .filter(|&c| !is_removed(c))
        .map(|c| if c == '\u{a0}' { ' ' } else { c })
        .collect();
    if let Some(word) = DEVICE_NAMES.iter().find(|word| starts_with_device(&name, word)) {
        name.insert(word.len(), '_');
    }
    mark_trailing_space_or_dot(&mut name);
    if name.len() > MAX_NAME_BYTES {
        name = shortened(&name);
        mark_trailing_space_or_dot(&mut name);
    }
    name
}

/// F1 step 2: forbidden punctuation, ASCII controls and characters that are
/// unassigned in Unicode 10.0.0.
fn is_removed(c: char) -> bool {
    matches!(
        c,
        '[' | ']' | '<' | '>' | ':' | '"' | '/' | '?' | '*' | '^' | '\\' | '|'
    ) || c.is_ascii_control()
        || unassigned_in_unicode_10(c)
}

fn unassigned_in_unicode_10(c: char) -> bool {
    let cp = u32::from(c);
    let after = UNASSIGNED.partition_point(|&(first, _)| first <= cp);
    after > 0 && cp <= UNASSIGNED[after - 1].1
}

/// F1 step 4: the name is `word`, or `word` followed by a `.`.
fn starts_with_device(name: &str, word: &str) -> bool {
    name.get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
        && matches!(name.as_bytes().get(word.len()), None | Some(b'.'))
}

/// F1 steps 5 and 6.4.
fn mark_trailing_space_or_dot(name: &mut String) {
    if name.ends_with([' ', '.']) {
        name.push('_');
    }
}

/// F1 step 6: extension capped at 10 bytes, stem at what is left of 120.
fn shortened(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let extension = prefix_within(extension, MAX_EXTENSION_BYTES);
    let stem = prefix_within(stem, MAX_NAME_BYTES - extension.len() - 2);
    if extension.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{extension}")
    }
}

/// The longest prefix of `text` that ends on a character boundary within
/// `limit` bytes.
fn prefix_within(text: &str, limit: usize) -> &str {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::safe_media_name;

    fn t(input: &str) -> String {
        safe_media_name(input)
    }

    #[test]
    fn characters_are_removed_and_spaces_replaced() {
        for (input, output) in [
            ("a:b?.png", "ab.png"),
            ("[x]<y>\"z\"/w*^\\|.png", "xyzw.png"),
            ("a\u{1}b\u{1f}c\u{7f}.png", "abc.png"),
            ("tab\tname.png", "tabname.png"),
            ("a\u{a0}b.png", "a b.png"),
            ("a\u{202f}b\u{3000}.png", "a\u{202f}b\u{3000}.png"),
            ("???", ""),
        ] {
            assert_eq!(t(input), output, "{input:?}");
        }
    }

    #[test]
    fn names_are_nfc_before_removal_only() {
        for (input, output) in [
            ("e\u{301}.png", "\u{e9}.png"),
            ("\u{1100}\u{1161}.png", "\u{ac00}.png"),
            // Removal can leave a decomposed sequence; it is not composed
            // again.
            ("e?\u{301}.png", "e\u{301}.png"),
        ] {
            assert_eq!(t(input), output, "{input:?}");
        }
    }

    #[test]
    fn unassigned_means_unassigned_in_unicode_10() {
        for c in [
            '\u{378}',
            '\u{fdd0}',
            '\u{ffff}',
            '\u{8be}',
            '\u{1f970}',
            '\u{1fae9}',
            '\u{10ffff}',
            '\u{e0000}',
        ] {
            assert_eq!(t(&format!("{c}.png")), ".png", "U+{:04X}", c as u32);
        }
        for c in ['\u{e000}', '\u{e0001}', '\u{860}', '\u{1f600}', '\u{200b}', '\u{1f9c0}'] {
            let name = format!("{c}.png");
            assert_eq!(t(&name), name, "U+{:04X}", c as u32);
        }
    }

    #[test]
    fn windows_device_names_get_an_underscore() {
        for (input, output) in [
            ("con.jpg", "con_.jpg"),
            ("con", "con_"),
            ("nul.tar.gz", "nul_.tar.gz"),
            ("CON.jpg", "CON_.jpg"),
            ("prn", "prn_"),
            ("Aux.txt", "Aux_.txt"),
            ("com1.png", "com1_.png"),
            ("LPT9", "LPT9_"),
            ("console.txt", "console.txt"),
            ("com0", "com0"),
            ("com10.txt", "com10.txt"),
            ("lpt10.png", "lpt10.png"),
            ("acon.jpg", "acon.jpg"),
            ("con .jpg", "con .jpg"),
        ] {
            assert_eq!(t(input), output, "{input:?}");
        }
    }

    #[test]
    fn a_trailing_space_or_dot_gets_an_underscore() {
        assert_eq!(t("test."), "test._");
        assert_eq!(t("test "), "test _");
        assert_eq!(t("con."), "con_._");
        assert_eq!(t(".."), ".._");
        assert_eq!(t("."), "._");
        assert_eq!(t(".png"), ".png");
        assert_eq!(t("a\u{a0}"), "a _");
    }

    #[test]
    fn long_names_are_cut_on_character_boundaries() {
        let r = |s: &str, n: usize| s.repeat(n);
        assert_eq!(t(&(r("x", 196) + ".jpg")), r("x", 115) + ".jpg");
        assert_eq!(t(&r("y", 200)), r("y", 118));
        assert_eq!(t(&(r("日", 70) + ".png")), r("日", 38) + ".png");
        assert_eq!(t(&(r("q", 116) + ".jpg")), r("q", 116) + ".jpg");
        assert_eq!(t(&(r("a", 100) + "." + &r("b", 30))), r("a", 100) + "." + &r("b", 10));
        assert_eq!(t(&(r("m", 120) + "." + &r("é", 6))), r("m", 108) + "." + &r("é", 5));
        assert_eq!(
            t(&(r("n", 120) + "." + &r("\u{1f600}", 3))),
            r("n", 110) + "." + &r("\u{1f600}", 2)
        );
        assert_eq!(t(&(r("w", 115) + ".abcdefghi jkl")), r("w", 108) + ".abcdefghi _");
        assert_eq!(t(&(r("t", 117) + " " + &r("t", 10))), r("t", 117) + " _");
        assert_eq!(t("z.abcdefghijklmnop"), "z.abcdefghijklmnop");
        assert!(t(&r("日", 200)).len() <= 120);
    }
}
