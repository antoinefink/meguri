//! HTML stripping and what a note save derives from it never panic, on any
//! input (specs/text/strip-html.md, checksum-and-sort-field.md,
//! field-emptiness.md).

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_text::{checksum, field_empty, html, normalize};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    // H2: a `<` that stays has no `>` after it.
    let removed = html::remove_markup(&text);
    if let Some(open) = removed.find('<') {
        assert!(!removed[open..].contains('>'), "{text:?} -> {removed:?}");
    }
    let _ = html::strip_markup(&text);
    let _ = html::strip_markup_keep_media(&text);
    // H6 step 5: no Unicode whitespace at either end.
    for line in [html::text_line(&text), html::text_line_keep_media(&text)] {
        assert_eq!(line.trim(), line, "{text:?}");
    }

    for normalize_note_text in [true, false] {
        let saved = normalize::field_as_saved(&text, normalize_note_text);
        assert!(!saved.contains(|c: char| c.is_ascii_control() && c != '\t' && c != '\n'));
        let _ = checksum::sort_text(&saved);
        let _ = checksum::note_checksum(&saved);
        let _ = field_empty::counts_as_empty(&saved);
    }
});
