//! Sort-field text and note checksum
//! (`specs/text/checksum-and-sort-field.md` C2, C4). Both take fields as
//! saved (C1, `normalize::field_as_saved`).

use std::borrow::Cow;

use sha1::{Digest, Sha1};

use crate::html::strip_markup_keep_media;

/// C2: the sort-field text of a saved field: [`strip_markup_keep_media`],
/// neither trimmed nor collapsed. Bind it as TEXT; the column's INTEGER
/// affinity decides whether SQLite stores a number (C3).
pub fn sort_text(field: &str) -> Cow<'_, str> {
    strip_markup_keep_media(field)
}

/// C2: the sort-field text of a note's saved fields; an index past the last
/// field gives the empty string.
pub fn sort_text_of<S: AsRef<str>>(fields: &[S], sort_field_idx: usize) -> Cow<'_, str> {
    fields
        .get(sort_field_idx)
        .map_or(Cow::Borrowed(""), |field| sort_text(field.as_ref()))
}

/// C4: `notes.csum` of a note whose saved first field is `first_field`: the
/// first four bytes, big-endian, of the SHA-1 of its sort-field text.
pub fn note_checksum(first_field: &str) -> u32 {
    checksum_text(&sort_text(first_field))
}

/// C4 on text already stripped (the draft check's comparison text, C5).
pub fn checksum_text(text: &str) -> u32 {
    let digest = Sha1::digest(text.as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_text_keeps_media_names_and_whitespace() {
        for (field, text) in [
            ("<b>私</b>", "私"),
            ("<img src=\"a b.jpg\">text", " a b.jpg text"),
            ("[sound:私.mp3]", "[sound:私.mp3]"),
            ("<style>x</style>私", "私"),
            ("Tom &amp; Jerry", "Tom & Jerry"),
            ("&#49;&#50;", "12"),
            ("\t5\n", "\t5\n"),
        ] {
            assert_eq!(sort_text(field), text, "{field:?}");
        }
    }

    #[test]
    fn the_sort_field_index_picks_the_field() {
        let fields = ["<i>front</i>", "back"];
        assert_eq!(sort_text_of(&fields, 0), "front");
        assert_eq!(sort_text_of(&fields, 1), "back");
        assert_eq!(sort_text_of(&fields, 2), "");
    }

    #[test]
    fn checksums_match_the_spec_table() {
        for (first, csum) in [
            ("", 3661210606),
            ("私", 1270766077),
            ("<b>私</b>", 1270766077),
            ("<div>私</div>", 1270766077),
            ("&#31169;", 1270766077),
            (" 私", 872227349),
            ("私 ", 2729607979),
            ("<img src=\"a b.jpg\">text", 4222853378),
            ("\u{e9}", 3205873265),
            ("ab", 3659751758),
            ("Tom & Jerry", 495331847),
            ("Tom &amp; Jerry", 495331847),
        ] {
            assert_eq!(note_checksum(first), csum, "{first:?}");
        }
    }
}
