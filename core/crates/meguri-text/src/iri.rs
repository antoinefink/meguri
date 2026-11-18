//! Percent-encoding of local media names in field HTML shown in web views,
//! and its inverse before an editor save (`specs/text/media-references.md`
//! M6–M8). Fields store raw names (R978); only display HTML is encoded.

use std::borrow::Cow;
use std::fmt::Write;

use crate::media_refs::{is_remote, media_tags};

/// M6: in each local media tag match, replaces the raw name by its encoding
/// ([`encode_name`]) everywhere it occurs inside the match.
pub fn encode_for_display(html: &str) -> Cow<'_, str> {
    rename_local_media(html, encode_name)
}

/// M7: in each local media tag match, replaces the raw name by its decoding
/// ([`decode_name`]) everywhere it occurs inside the match.
pub fn decode_for_saving(html: &str) -> Cow<'_, str> {
    rename_local_media(html, decode_name)
}

/// Editor H3 steps 3–5. Step 1–2's serialization comes from the editor
/// runtime, while unchanged fields bypass this function entirely.
pub fn editor_target(html: &str) -> String {
    if html == "<br>" || html == "<div><br></div>" {
        return String::new();
    }
    let without_nulls = html.replace('\0', "");
    rename_local_media(&without_nulls, |name| {
        let decoded = decode_name(name);
        if decoded.contains('"') {
            Cow::Owned(decoded.replace('"', "&quot;"))
        } else {
            decoded
        }
    }).into_owned()
}

/// M6 step 1: the raw name with ASCII controls, space, `"`, `<`, `>`,
/// backtick and `#` written as `%XX` (upper-case hex). Everything else,
/// `%` and non-ASCII included, is kept; entities are not decoded.
pub fn encode_name(raw_name: &str) -> Cow<'_, str> {
    if !raw_name.contains(needs_escape) {
        return Cow::Borrowed(raw_name);
    }
    let mut out = String::with_capacity(raw_name.len() + 8);
    for c in raw_name.chars() {
        if needs_escape(c) {
            write!(out, "%{:02X}", u32::from(c)).expect("writing to a String");
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// M7 step 1: each `%` + two hex digits becomes that byte, any other `%`
/// stays, and the bytes are read as UTF-8 with each invalid sequence as
/// U+FFFD.
pub fn decode_name(raw_name: &str) -> Cow<'_, str> {
    if !raw_name.contains('%') {
        return Cow::Borrowed(raw_name);
    }
    let bytes = raw_name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = match bytes.get(i..i + 3) {
            Some([b'%', high, low]) => hex_value(*high).zip(hex_value(*low)).map(|(h, l)| h << 4 | l),
            _ => None,
        };
        if let Some(byte) = escaped {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

/// Rewrites the local media tag matches of `html` whose name `rename`
/// changes; everything else is copied as it is.
fn rename_local_media(html: &str, rename: fn(&str) -> Cow<'_, str>) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut copied = 0;
    for tag in media_tags(html) {
        if is_remote(tag.raw_name) {
            continue;
        }
        let renamed = rename(tag.raw_name);
        if renamed == tag.raw_name {
            continue;
        }
        let out = out.get_or_insert_with(|| String::with_capacity(html.len() + 16));
        out.push_str(&html[copied..tag.start]);
        out.push_str(&tag.matched.replace(tag.raw_name, &renamed));
        copied = tag.start + tag.matched.len();
    }
    match out {
        None => Cow::Borrowed(html),
        Some(mut out) => {
            out.push_str(&html[copied..]);
            Cow::Owned(out)
        }
    }
}

/// M6 step 1: the ASCII characters a displayed name must not carry raw.
fn needs_escape(c: char) -> bool {
    c.is_ascii_control() || matches!(c, ' ' | '"' | '<' | '>' | '`' | '#')
}

fn hex_value(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).and_then(|v| u8::try_from(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_target_keeps_markup_and_decoded_quotes_inside_attributes() {
        assert_eq!(editor_target("<img src=\"q%22r.png\" onerror=\"f()\">"),
            "<img src=\"q&quot;r.png\" onerror=\"f()\">");
        assert_eq!(editor_target("<script>f()</script><style>div{display:none}</style>"),
            "<script>f()</script><style>div{display:none}</style>");
        assert_eq!(editor_target("<br>"), "");
        assert_eq!(editor_target("<div><br></div>"), "");
        assert_eq!(editor_target("<br>\0"), "<br>");
        assert_eq!(editor_target("<img src=\"https://example.invalid/a%20b\">"),
            "<img src=\"https://example.invalid/a%20b\">");
        assert_eq!(editor_target("<img src=\"a%00b\">"), "<img src=\"a\0b\">");
    }

    #[test]
    fn encoding_matches_the_spec_examples() {
        for (html, encoded) in [
            ("<img src=\"a b#c.jpg\">", "<img src=\"a%20b%23c.jpg\">"),
            ("<img src=\"100%.jpg\">", "<img src=\"100%.jpg\">"),
            ("<img src='q\"r.png'>", "<img src='q%22r.png'>"),
            ("<img src=\"\">", "<img src=%22%22>"),
            (
                "<img src=\"http&#58;//x.org/a b.png\">",
                "<img src=\"http&%2358;//x.org/a%20b.png\">",
            ),
            ("<img src=\"日本.png\">", "<img src=\"日本.png\">"),
            ("<img title=\"a b\" src=\"a b\">", "<img title=\"a%20b\" src=\"a%20b\">"),
            (
                "<img src=\"a\u{1}\u{7f}`<>.png\">",
                "<img src=\"a%01%7F%60%3C%3E.png\">",
            ),
            ("<img src=\"'&?=+/~%.png\">", "<img src=\"'&?=+/~%.png\">"),
            (
                "<img src=\"https://e.com/a b.png\">",
                "<img src=\"https://e.com/a b.png\">",
            ),
            ("a b <i>c d</i> [sound:e f.mp3]", "a b <i>c d</i> [sound:e f.mp3]"),
            ("x <img src=\"a b.png\"> y", "x <img src=\"a%20b.png\"> y"),
        ] {
            assert_eq!(encode_for_display(html), encoded, "{html:?}");
        }
        assert!(matches!(encode_for_display("<img src=\"ok.png\">"), Cow::Borrowed(_)));
        assert_eq!(encode_name("a b\t#"), "a%20b%09%23");
        assert_eq!(encode_name("日本 .png"), "日本%20.png");
        assert_eq!(decode_name("100%"), "100%");
        assert_eq!(decode_name("%41%4a%4A"), "AJJ");
    }

    #[test]
    fn decoding_matches_the_spec_examples() {
        let name = |n: &str| {
            let html = format!("<img src=\"{n}\">");
            let out = decode_for_saving(&html).into_owned();
            out["<img src=\"".len()..out.len() - 2].to_owned()
        };
        assert_eq!(name("%20lead.png"), " lead.png");
        assert_eq!(name("a%2Fb.png"), "a/b.png");
        assert_eq!(name("%e6%97%a5.png"), "日.png");
        assert_eq!(name("%ff.png"), "\u{fffd}.png");
        assert_eq!(name("%E6%97.png"), "\u{fffd}.png");
        assert_eq!(name("a%2.png"), "a%2.png");
        assert_eq!(name("a%00b.png"), "a\u{0}b.png");
        assert_eq!(name("a%%41.png"), "a%A.png");
        assert_eq!(
            decode_for_saving("<img src=\"https://e.com/a%20b.png\">"),
            "<img src=\"https://e.com/a%20b.png\">"
        );
    }

    #[test]
    fn the_round_trip_breaks_only_where_m8_says() {
        // Condition 1: an encoded-looking raw name decodes.
        assert_eq!(encode_for_display("<img src=\"a%20b.jpg\">"), "<img src=\"a%20b.jpg\">");
        assert_eq!(decode_for_saving("<img src=\"a%20b.jpg\">"), "<img src=\"a b.jpg\">");
        // Condition 2: the encoded name already occurs in the match.
        let encoded = encode_for_display("<img title=\"a%20b\" src=\"a b\">");
        assert_eq!(encoded, "<img title=\"a%20b\" src=\"a%20b\">");
        assert_eq!(decode_for_saving(&encoded), "<img title=\"a b\" src=\"a b\">");
        // Condition 3: the raw name is part of the tag's own syntax.
        let encoded = encode_for_display("<img src=\"<\">");
        assert_eq!(encoded, "%3Cimg src=\"%3C\">");
        assert_eq!(decode_for_saving(&encoded), encoded);
        // Condition 4: encoding changes which tags match.
        let text = "<video title=\"a\">x<img src=\"a>b.png\"> <audio title=\"t\" src=\"https://e.org/p.png\">";
        let encoded = encode_for_display(text);
        assert_eq!(
            encoded,
            "<video title=\"a\">x<img src=\"a%3Eb.png\"> <audio title=\"t\" src=\"https://e.org/p.png\">"
        );
        assert_eq!(decode_for_saving(&encoded), encoded);
        // A text meeting all four conditions comes back.
        let text = "<img src=\"a b#c.jpg\"> and <audio src='q\"r.mp3'>";
        assert_eq!(decode_for_saving(&encode_for_display(text)), text);
    }
}
