//! Media references in field text (`specs/text/media-references.md` M1–M5).

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::entities;

/// M2, built from its four parts. Case-insensitive throughout (H1); `\b`
/// is the Unicode word boundary, so `<img2` and `xsrc=` do not count.
static MEDIA_TAG: LazyLock<Regex> = LazyLock::new(|| {
    // 1. The element name, not followed by a word character.
    let element = r"<(?:img|audio|video|object|source)\b";
    // 2. The attribute run, as short as possible: single non-`>` characters
    //    first, a quoted run only to step over a `>` inside quotes.
    let attribute_run = r#"(?:[^>]|"[^"]+"|'[^']+')+?"#;
    // 3. The attribute, after a non-word character.
    let attribute = r"\b(?:src|data)=";
    // 4. The value: double-quoted, single-quoted, or bare up to `>` or to a
    //    space that starts the rest of the tag. Groups 1-3 hold the name.
    let value = [r#""([^"]+?)"[^>]*>"#, r"'([^']+?)'[^>]*>", r"([^>]+?)(?:>|\x20[^>]*>)"].join("|");
    Regex::new(&format!("(?i){element}{attribute_run}{attribute}(?:{value})")).expect("M2 pattern")
});

/// M3: the name runs to the first `]` after its first character.
static SOUND_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\[sound:(.+?)\]").expect("M3 pattern"));

/// M5: `http://` or `https://` at the start, case-insensitive (U+017F counts
/// as `s`).
static REMOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\Ahttps?://").expect("M5 pattern"));

/// One media tag match (M2): an `img`, `audio`, `video`, `object` or
/// `source` element naming a file in a `src` or `data` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaTag<'a> {
    /// Byte offset of the match's `<` in the scanned text.
    pub start: usize,
    /// The match, from its `<` to the `>` that closes the value.
    pub matched: &'a str,
    /// The file name as written, entities not decoded.
    pub raw_name: &'a str,
}

/// One sound tag match (M3): `[sound:<name>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundTag<'a> {
    /// Byte offset of the match's `[` in the scanned text.
    pub start: usize,
    /// The match, `[sound:` to `]`.
    pub matched: &'a str,
    /// The file name as written, entities not decoded.
    pub raw_name: &'a str,
}

/// The whole match and the first participating group of `captures`.
fn whole_and_name<'a>(captures: &Captures<'a>) -> (regex::Match<'a>, &'a str) {
    let whole = captures.get(0).expect("group 0 always participates");
    let name = captures
        .iter()
        .skip(1)
        .flatten()
        .next()
        .expect("every alternative has a name group");
    (whole, name.as_str())
}

/// M2: the media tag matches of `text`, left to right, never overlapping.
pub fn media_tags(text: &str) -> impl Iterator<Item = MediaTag<'_>> {
    MEDIA_TAG.captures_iter(text).map(|captures| {
        let (whole, raw_name) = whole_and_name(&captures);
        MediaTag {
            start: whole.start(),
            matched: whole.as_str(),
            raw_name,
        }
    })
}

/// M3: the sound tag matches of `text`, left to right, never overlapping.
pub fn sound_tags(text: &str) -> impl Iterator<Item = SoundTag<'_>> {
    SOUND_TAG.captures_iter(text).map(|captures| {
        let (whole, raw_name) = whole_and_name(&captures);
        SoundTag {
            start: whole.start(),
            matched: whole.as_str(),
            raw_name,
        }
    })
}

/// M5: a raw name starting with `http://` or `https://`, case-insensitive.
pub fn is_remote(raw_name: &str) -> bool {
    REMOTE.is_match(raw_name)
}

/// The raw names M4 lists, media tags first, then sounds.
fn raw_names(text: &str) -> impl Iterator<Item = &str> {
    media_tags(text)
        .map(|tag| tag.raw_name)
        .chain(sound_tags(text).map(|sound| sound.raw_name))
}

/// M4: the decoded names of all media tags, then of all sound tags, each in
/// text order, duplicates and remote names included.
pub fn references(text: &str) -> Vec<String> {
    raw_names(text)
        .map(|raw| entities::decode_text(raw).into_owned())
        .collect()
}

/// M5: the references that name local files: what Check Media counts as used
/// (R989).
pub fn local_references(text: &str) -> Vec<String> {
    raw_names(text)
        .filter(|raw| !is_remote(raw))
        .map(|raw| entities::decode_text(raw).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<&str> {
        media_tags(text).map(|t| t.raw_name).collect()
    }

    #[test]
    fn media_tag_grammar_matches_the_spec_table() {
        for (text, name) in [
            ("<img src=\"a b.jpg\">", Some("a b.jpg")),
            ("<img src=unq.png title=t>", Some("unq.png")),
            ("<img src=unq.png/>", Some("unq.png/")),
            ("<audio src= [br].png alt=\"z\">", Some(" [br].png")),
            ("<img src=a.png\nalt=x>", Some("a.png\nalt=x")),
            ("<img src='a'b'>", Some("a")),
            ("<img src=\"\">", Some("\"\"")),
            ("<img alt=\"src=x.png\" src=\"y.png\">", Some("x.png\"")),
            ("<img alt=\"q>r\" src=\"s.png\">", Some("s.png")),
            ("<img alt='t' > src='u.png'>", None),
            ("<video poster=\"p.jpg\" src=\"v.mp4\">", Some("v.mp4")),
            ("<img src=\"never closed.png\"", None),
            ("<img data-src=\"lazy.png\">", Some("lazy.png")),
            ("<object data=\"o.svg\"></object>", Some("o.svg")),
            ("<IMG SRC=\"e.png\">", Some("e.png")),
            ("<\u{17f}ource \u{17f}rc=f.mp3>", Some("f.mp3")),
            ("<img\u{a0}src=nbsp.png>", Some("nbsp.png")),
            ("<img srcset=\"set.png 2x\">", None),
            ("<img xsrc=\"x.png\">", None),
            ("<img src = \"spaced.png\">", None),
            ("<imgx src=a.png>", None),
            ("<img2 src=b.png>", None),
            ("<image src=\"d.png\">", None),
            ("<img\u{e9} src=c.png>", None),
        ] {
            let found = names(text);
            assert_eq!(found.first().copied(), name, "{text:?}");
            assert!(found.len() <= 1, "{text:?}: {found:?}");
        }
    }

    #[test]
    fn matches_carry_their_span() {
        let text = "a<img src=\"1.png\">b<img src='2.png'>";
        let tags: Vec<MediaTag<'_>> = media_tags(text).collect();
        assert_eq!(tags.len(), 2);
        assert_eq!((tags[0].start, tags[0].matched), (1, "<img src=\"1.png\">"));
        assert_eq!((tags[1].start, tags[1].matched), (19, "<img src='2.png'>"));
    }

    #[test]
    fn sound_tags_end_at_the_first_bracket_after_the_name() {
        fn sounds(text: &str) -> Vec<&str> {
            sound_tags(text).map(|s| s.raw_name).collect()
        }
        assert_eq!(sounds("[sound:a]b]"), ["a"]);
        assert_eq!(sounds("[sound:]]"), ["]"]);
        assert_eq!(sounds("[sound:\nx]"), ["\nx"]);
        assert!(sounds("[sound:]").is_empty());
        assert!(sounds("[Sound:no.mp3]").is_empty());
        assert_eq!(sounds("[sound:1][sound:2]"), ["1", "2"]);
    }

    #[test]
    fn references_list_media_tags_then_sounds_decoded() {
        assert_eq!(
            references("<img src=\"1.png\"><img src='2.png'>[sound:3.mp3]<img src=4.png>"),
            ["1.png", "2.png", "4.png", "3.mp3"]
        );
        assert_eq!(references("<img src=\"[sound:7.mp3]\">"), ["[sound:7.mp3]", "7.mp3"]);
        assert_eq!(references("<source src=\"a&amp;b.ogg\">"), ["a&b.ogg"]);
        assert_eq!(references("<img src=\"a&amp;b&c.png\">"), ["a&amp;b&c.png"]);
        assert_eq!(references("<img src=\"&nbsp;n.png\">"), [" n.png"]);
        assert_eq!(references("[sound:&amp;.mp3]"), ["&.mp3"]);
        assert_eq!(references("<img src=\"https://e.com/a.png\">"), ["https://e.com/a.png"]);
        assert!(references("").is_empty());
    }

    #[test]
    fn remote_names_are_tested_raw() {
        for name in [
            "http://e.com/a.png",
            "https://e.com/a.png",
            "HTTPS://E.COM/A",
            "http\u{17f}://x",
        ] {
            assert!(is_remote(name), "{name:?}");
        }
        for name in [
            " https://e.com/a.png",
            "http&#58;//x.org/a.png",
            "//host/x",
            "data:image/png,x",
            "httpx://a",
        ] {
            assert!(!is_remote(name), "{name:?}");
        }
        assert_eq!(
            local_references("<img src=\"https://e.com/a.png\"><img src=b.png>[sound:http://x/c.mp3]"),
            ["b.png"]
        );
        assert_eq!(
            local_references("<img src=\"http&#58;//x.org/a.png\">"),
            ["http://x.org/a.png"]
        );
    }
}
