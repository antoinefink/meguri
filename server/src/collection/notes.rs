// SPDX-License-Identifier: MIT
//! Normal-channel derived values from the neutral text specs. Full-file transfer
//! never calls this adapter: stored field bytes and derived columns stay opaque.
use regex::Regex;
use sha1::{Digest, Sha1};
use std::sync::LazyLock;

pub fn fields(stored: &str) -> impl Iterator<Item = &str> { stored.split('\u{1f}') }

/// Text to bind as TEXT for SQLite sort-field affinity; checksum as INTEGER.
/// The server normal channel removes ASCII controls except TAB/LF, without NFC.
#[derive(Debug, PartialEq, Eq)]
pub struct DerivedNoteValues {
    pub fields: String,
    pub sort_field: String,
    pub checksum: u32,
}

pub fn derive_values(stored: &str, sort_field_index: usize) -> DerivedNoteValues {
    // Split before removing controls so the field separator is preserved.
    let normalized: Vec<String> = fields(stored).map(|field| {
        field.chars().filter(|c| !c.is_ascii_control() || matches!(*c, '\t' | '\n')).collect()
    }).collect();
    derive_saved_values(&normalized.join("\u{1f}"), sort_field_index)
}

/// C7 patches preserve omitted field bytes; derive columns from that saved text
/// without applying sync-ingest normalization to the complete note again.
pub(super) fn derive_saved_values(stored: &str, sort_field_index: usize) -> DerivedNoteValues {
    let values: Vec<_> = fields(stored).collect();
    let first = comparison_text(values[0]);
    let sort_field = values.get(sort_field_index).map(|s| comparison_text(s)).unwrap_or_default();
    // RustCrypto owns the primitive. This legacy checksum is not authentication.
    let digest = Sha1::digest(first.as_bytes());
    let checksum = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    DerivedNoteValues { fields: stored.to_owned(), sort_field, checksum }
}

fn comparison_text(field: &str) -> String {
    static MEDIA: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r#"(?is)<(?:img|audio|video|object|source)\b(?:[^>]|"[^"]+"|'[^']+')+?\b(?:src|data)=(?:"(?P<double>[^"]+?)"[^>]*>|'(?P<single>[^']+?)'[^>]*>|(?P<bare>[^>]+?)(?:>| [^>]*>))"#
    ).expect("neutral media grammar"));
    static MARKUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r"(?is)<!--.*?-->|<style[^>]*>.*?</style>|<script[^>]*>.*?</script>|<[^>]*>"
    ).expect("neutral stripping grammar"));
    let media = MEDIA.replace_all(field, |captures: &regex::Captures<'_>| {
        let name = captures.name("double").or_else(|| captures.name("single")).or_else(|| captures.name("bare")).expect("media value");
        format!(" {} ", name.as_str())
    });
    let text = MARKUP.replace_all(&media, "");
    if text.contains('&') {
        if let Ok(decoded) = htmlescape::decode_html(&text) { return decoded.replace('\u{a0}', " "); }
    }
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_sort_and_legacy_checksum_vectors() {
        for (field, text, checksum) in [
            ("", "", 3661210606),
            ("私", "私", 1270766077),
            ("<b>私</b>", "私", 1270766077),
            ("&#31169;", "私", 1270766077),
            (" 私", " 私", 872227349),
            ("私 ", "私 ", 2729607979),
            (r#"<img src="a b.jpg">text"#, " a b.jpg text", 4222853378),
            ("a\u{1}b", "ab", 3659751758),
            ("Tom & Jerry", "Tom & Jerry", 495331847),
            ("Tom &amp; Jerry", "Tom & Jerry", 495331847),
            ("é", "é", 3205873265),
            ("e\u{301}", "e\u{301}", 2118106353),
        ] {
            let result = derive_values(field, 0);
            assert_eq!((result.sort_field.as_str(), result.checksum), (text, checksum), "{field:?}");
        }
    }

    #[test]
    fn server_policy_preserves_field_boundaries_and_skips_nfc() {
        let result = derive_values("a\0\u{7f}b\u{1f}\t5\r\n\u{1f}e\u{301}", 1);
        assert_eq!(result.fields, "ab\u{1f}\t5\n\u{1f}e\u{301}");
        assert_eq!(result.sort_field, "\t5\n");
        assert_eq!(result.checksum, 3659751758);
        assert_eq!(derive_values("a\u{1f}b", 4).sort_field, "");
        // Entity-produced controls arrive after field normalization.
        assert_eq!(derive_values("&#0;z", 0).sort_field, "\0z");
    }

    #[test]
    fn neutral_markup_and_entity_vectors_are_not_html_parser_repairs() {
        for (field, expected) in [
            ("a < b > c", "a  c"), ("a<b<c>d", "ad"), ("<<em>>", ">"),
            ("x<style>no end", "xno end"), ("<!-- open > tail", " tail"),
            ("<!-->b-->tail", "tail"), ("<styles>x</style>y", "y"),
            ("<ſtyle>hidden</STYLE>私", "私"), ("<SCRIPT>\nx</script>z", "z"),
            ("a<no end", "a<no end"),
            ("ok &amp; then &unknown; later", "ok &amp; then &unknown; later"),
            ("&amp;nbsp;", "&nbsp;"), ("&apos; &amp;", "&apos; &amp;"),
            ("&#X41; &amp;", "&#X41; &amp;"), ("&#55296; &amp;", "&#55296; &amp;"),
            ("&#x41;&#49;", "A1"), ("&AMP;", "&AMP;"),
            ("raw\u{a0}nbsp", "raw\u{a0}nbsp"),
            ("raw\u{a0}nbsp &amp; x", "raw nbsp & x"),
            ("raw\u{a0}nbsp &bogus; x", "raw\u{a0}nbsp &bogus; x"),
        ] { assert_eq!(derive_values(field, 0).sort_field, expected, "{field:?}"); }
    }

    #[test]
    fn neutral_media_names_keep_raw_spacing_and_unicode_matching() {
        for (field, expected) in [
            (r#"<img src="a<b>.jpg">"#, " a.jpg "),
            (r#"<img src="a&amp;b.png"> &bogus;"#, " a&amp;b.png  &bogus;"),
            (r#"<img alt="q>r" src="s.png">"#, " s.png "),
            (r#"<img alt="src=x.png" src="y.png">"#, " x.png\" "),
            ("<img src=unq.png title=t>", " unq.png "),
            ("<img src=unq.png/>", " unq.png/ "),
            ("<audio src= [br].png alt='z'>", "  [br].png "),
            ("<img src=a.png\nalt=x>", " a.png\nalt=x "),
            ("<img src='a'b'>", " a "), ("<img src=\"\">", " \"\" "),
            ("<img data-src='x'>", " x "), ("<ſource src='x'>", " x "),
            ("<imgé src='x'>", ""), ("<img xsrc='x'>", ""),
            ("<img src ='x'>", ""), ("<img srcset='x'>", ""),
            ("<img src='x'", "<img src='x'"),
            ("[sound:私.mp3]", "[sound:私.mp3]"),
        ] { assert_eq!(derive_values(field, 0).sort_field, expected, "{field:?}"); }
    }
}
