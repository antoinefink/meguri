//! Find Duplicates' key and groups (`specs/browser/workspace.md` V10): what
//! makes two notes duplicates in one field. Nothing here reads the
//! collection; the engine reads the field values and hands them over.

use std::collections::HashMap;

use meguri_model::ids::NoteId;
use meguri_text::html;

use crate::fold;

/// V10's key of a field value: the value with HTML removed as `dupe:`
/// removes it (`text/strip-html` H5, media file names kept, which decodes
/// the character references by H3's all-or-nothing rule), then every run of
/// White_Space characters made one U+0020 and both ends trimmed, then
/// simple case folding (`search/case-folding` C3). An empty key means the
/// note is left out.
pub fn key(value: &str) -> String {
    let stripped = html::strip_markup_keep_media(value);
    let mut joined = String::with_capacity(stripped.len());
    // `split_whitespace` splits at White_Space characters.
    for word in stripped.split_whitespace() {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(word);
    }
    fold::simple_fold(&joined)
}

/// Notes gathered by key.
#[derive(Debug, Default)]
pub struct Groups {
    by_key: HashMap<String, Vec<NoteId>>,
}

impl Groups {
    pub fn new() -> Groups {
        Groups::default()
    }

    /// Adds `note`, whose field holds `value`; a value whose key is empty
    /// adds nothing.
    pub fn add(&mut self, note: NoteId, value: &str) {
        let key = key(value);
        if key.is_empty() {
            return;
        }
        self.by_key.entry(key).or_default().push(note);
    }

    /// The groups of two or more notes: each note once, ascending, and the
    /// groups by their first note.
    pub fn finish(self) -> Vec<Vec<NoteId>> {
        let mut groups: Vec<Vec<NoteId>> = self
            .by_key
            .into_values()
            .filter_map(|mut notes| {
                notes.sort_unstable();
                notes.dedup();
                (notes.len() >= 2).then_some(notes)
            })
            .collect();
        groups.sort_unstable_by_key(|notes| notes[0]);
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_strips_collapses_trims_and_folds() {
        for (value, expected) in [
            ("Zub", "zub"),
            ("<b>Zub</b>", "zub"),
            ("  Plo \t\n zub\u{a0} ", "plo zub"),
            ("Plo&nbsp;&nbsp;Zub", "plo zub"),
            // Media file names stay, sound tags are text.
            ("<img src=\"Fep.JPG\">Zub", "fep.jpg zub"),
            ("Zub[sound:Rel.mp3]", "zub[sound:rel.mp3]"),
            // H3 decodes all or nothing.
            ("Pim &amp; Vok", "pim & vok"),
            ("Pim & Vok &amp; ka", "pim & vok &amp; ka"),
            // Simple folding: ẞ folds to ß, never to ss.
            ("STRAẞE", "straße"),
            ("\u{3000}x\u{2028}y\u{3000}", "x y"),
            ("<br>", ""),
            ("", ""),
        ] {
            assert_eq!(key(value), expected, "{value:?}");
        }
    }

    #[test]
    fn groups_hold_two_or_more_notes_with_equal_keys() {
        let mut groups = Groups::new();
        for (note, value) in [
            (5, "Zub"),
            (2, "<i>zub</i>"),
            (9, "wem"),
            (1, "Wem "),
            (3, "kost"),
            (4, ""),
            (6, "  "),
            (7, "zub"),
        ] {
            groups.add(NoteId(note), value);
        }
        assert_eq!(
            groups.finish(),
            vec![vec![NoteId(1), NoteId(9)], vec![NoteId(2), NoteId(5), NoteId(7)]]
        );
    }

    #[test]
    fn a_note_listed_twice_counts_once() {
        let mut groups = Groups::new();
        groups.add(NoteId(1), "a");
        groups.add(NoteId(1), "A");
        assert!(groups.finish().is_empty());
    }
}
