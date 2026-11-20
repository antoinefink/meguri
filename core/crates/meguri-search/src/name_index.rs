//! The name index suggestions read (`specs/search/editing-services.md`
//! D6.7): decks with their ids, filtered state and whether they have
//! subdecks; registered tags; note types with their ids, template names by
//! ordinal and field names; preset names; saved searches as
//! `browser/config-keys` K7.1 reads `savedFilters`; flag shown names as
//! `tags/flags-and-marked` F4 reads `flagLabels`.
//!
//! Every list is in D6.5 step 3's order, `ORDER BY name COLLATE unicase,
//! name` (a deck by its stored name, levels joined by U+001F), so a reader
//! never sorts. The index holds values only: meguri-engine builds it when
//! a collection opens and replaces the parts a change set touches
//! ([`Sections::of`]). Each part is shared, so a change copies only the
//! part it changes, and a reader keeps the index it was handed.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use meguri_model::changes::ChangeSet;
use meguri_model::ids::{DeckConfigId, DeckId, NotetypeId};
use meguri_model::note::FIELD_SEPARATOR;
use meguri_storage::ConfigValue;
use meguri_text::fold::{compare_folded, folded};

use crate::config::{Key, SavedSearches};

/// One deck as a collection stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckRow {
    pub id: DeckId,
    /// The stored name: levels joined by U+001F.
    pub native: String,
    pub filtered: bool,
}

/// One deck in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckName {
    pub id: DeckId,
    /// The name as shown and as suggestions label it: levels joined by `::`.
    pub name: String,
    /// The stored name, which orders the list.
    pub native: String,
    pub filtered: bool,
    pub has_subdecks: bool,
}

/// One note type's names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotetypeNames {
    pub id: NotetypeId,
    pub name: String,
    /// Template names by ordinal.
    pub templates: Vec<String>,
    /// Field names by ordinal.
    pub fields: Vec<String>,
}

/// One preset (`deck_config` row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetName {
    pub id: DeckConfigId,
    pub name: String,
}

/// One saved search (K7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSearch {
    pub name: String,
    pub text: String,
}

/// The index. Cloning it shares every part.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NameIndex {
    decks: Arc<Vec<DeckName>>,
    tags: Arc<Vec<String>>,
    notetypes: Arc<Vec<NotetypeNames>>,
    presets: Arc<Vec<PresetName>>,
    saved_searches: Arc<Vec<SavedSearch>>,
    flag_names: Arc<[String; 7]>,
}

/// D6.5 step 3: unicase first, then the bytes.
fn name_order(a: &str, b: &str) -> Ordering {
    compare_folded(a, b).then_with(|| a.cmp(b))
}

impl NameIndex {
    pub fn decks(&self) -> &[DeckName] {
        &self.decks
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn notetypes(&self) -> &[NotetypeNames] {
        &self.notetypes
    }

    pub fn presets(&self) -> &[PresetName] {
        &self.presets
    }

    pub fn saved_searches(&self) -> &[SavedSearch] {
        &self.saved_searches
    }

    /// The shown names of flags 1 to 7, by flag number.
    pub fn flag_names(&self) -> &[String; 7] {
        &self.flag_names
    }

    /// Every deck, in stored-name order, each marked when another deck lies
    /// below it.
    pub fn set_decks(&mut self, rows: Vec<DeckRow>) {
        let mut parents = HashSet::new();
        for row in &rows {
            for (at, _) in row.native.match_indices(FIELD_SEPARATOR) {
                parents.insert(folded(&row.native[..at]));
            }
        }
        let mut decks: Vec<DeckName> = rows
            .into_iter()
            .map(|row| DeckName {
                id: row.id,
                name: meguri_model::deck::human_name(&row.native),
                has_subdecks: parents.contains(&folded(&row.native)),
                native: row.native,
                filtered: row.filtered,
            })
            .collect();
        decks.sort_by(|a, b| name_order(&a.native, &b.native).then(a.id.cmp(&b.id)));
        self.decks = Arc::new(decks);
    }

    /// Every registered tag.
    pub fn set_tags(&mut self, mut tags: Vec<String>) {
        tags.sort_by(|a, b| name_order(a, b));
        tags.dedup();
        self.tags = Arc::new(tags);
    }

    /// A tag registered, or respelled: the stored names are unique under
    /// unicase (`tags/tags` T3), so a name equal under unicase is replaced.
    pub fn put_tag(&mut self, name: &str) {
        let tags = Arc::make_mut(&mut self.tags);
        match tags.binary_search_by(|tag| compare_folded(tag, name)) {
            Ok(at) => name.clone_into(&mut tags[at]),
            Err(at) => tags.insert(at, name.to_owned()),
        }
    }

    /// A tag no longer registered.
    pub fn remove_tag(&mut self, name: &str) {
        let tags = Arc::make_mut(&mut self.tags);
        if let Ok(at) = tags.binary_search_by(|tag| compare_folded(tag, name)) {
            tags.remove(at);
        }
    }

    /// Every note type.
    pub fn set_notetypes(&mut self, mut notetypes: Vec<NotetypeNames>) {
        notetypes.sort_by(notetype_order);
        self.notetypes = Arc::new(notetypes);
    }

    /// A note type added or changed.
    pub fn put_notetype(&mut self, names: NotetypeNames) {
        let notetypes = Arc::make_mut(&mut self.notetypes);
        notetypes.retain(|n| n.id != names.id);
        let at = notetypes.partition_point(|n| notetype_order(n, &names) == Ordering::Less);
        notetypes.insert(at, names);
    }

    /// A note type removed.
    pub fn remove_notetype(&mut self, id: NotetypeId) {
        if self.notetypes.iter().any(|n| n.id == id) {
            Arc::make_mut(&mut self.notetypes).retain(|n| n.id != id);
        }
    }

    /// Every preset.
    pub fn set_presets(&mut self, mut presets: Vec<PresetName>) {
        presets.sort_by(|a, b| name_order(&a.name, &b.name).then(a.id.cmp(&b.id)));
        self.presets = Arc::new(presets);
    }

    /// The saved searches K7.1 reads from the stored `savedFilters` value.
    pub fn set_saved_searches(&mut self, stored: Option<&ConfigValue>) {
        let mut saved: Vec<SavedSearch> = SavedSearches::from_stored(stored)
            .list()
            .into_iter()
            .map(|(name, text)| SavedSearch {
                name: name.to_owned(),
                text: text.to_owned(),
            })
            .collect();
        saved.sort_by(|a, b| name_order(&a.name, &b.name));
        self.saved_searches = Arc::new(saved);
    }

    /// The shown names of flags 1 to 7 (F4).
    pub fn set_flag_names(&mut self, names: [String; 7]) {
        self.flag_names = Arc::new(names);
    }
}

fn notetype_order(a: &NotetypeNames, b: &NotetypeNames) -> Ordering {
    name_order(&a.name, &b.name).then(a.id.cmp(&b.id))
}

/// The parts of the index a change set touches (D6.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sections {
    pub decks: bool,
    pub tags: bool,
    pub notetypes: bool,
    pub presets: bool,
    pub saved_searches: bool,
    pub flag_names: bool,
}

impl Sections {
    pub const ALL: Sections = Sections {
        decks: true,
        tags: true,
        notetypes: true,
        presets: true,
        saved_searches: true,
        flag_names: true,
    };

    /// Decks, tags, note types (their fields and templates included),
    /// presets, and the `savedFilters` and `flagLabels` keys; everything
    /// after a whole-collection change, which lists no rows.
    pub fn of(changes: &ChangeSet) -> Sections {
        if changes.whole_collection {
            return Sections::ALL;
        }
        let key = |key: Key| changes.config_keys.iter().any(|change| change.key == key.name());
        Sections {
            decks: !changes.decks.is_empty(),
            tags: !changes.tags.is_empty(),
            notetypes: !changes.notetypes.is_empty(),
            presets: !changes.deck_configs.is_empty(),
            saved_searches: key(Key::SavedFilters),
            flag_names: key(Key::FlagLabels),
        }
    }

    pub fn any(self) -> bool {
        self != Sections::default()
    }
}
