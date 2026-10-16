//! Deck names (`specs/fixtures/anonymizer.md` A11.1) and tags (A12).

use std::collections::{BTreeMap, HashMap};

use meguri_text::{fold, normalize};

use super::keys::{Salt, TAG_DOMAIN};

/// The separator of deck name components as stored.
pub const STORED_SEPARATOR: char = '\u{1f}';
pub const SEPARATOR: &str = "::";

#[derive(Default)]
struct Node {
    rank: String,
    /// Children by the fold of their component: the map's order is U2's.
    children: BTreeMap<String, Node>,
}

impl Node {
    fn assign(&mut self) {
        let n = self.children.len();
        let width = (n.saturating_sub(1)).to_string().len();
        for (i, child) in self.children.values_mut().enumerate() {
            child.rank = format!("d{i:0width$}");
            child.assign();
        }
    }
}

/// A11.1: each deck's new stored name, by id. `decks` holds ids and stored
/// names (components separated by U+001F).
pub fn deck_ranks(decks: &[(i64, String)]) -> HashMap<i64, String> {
    let mut root = Node::default();
    let paths: Vec<(i64, Vec<String>)> = decks
        .iter()
        .map(|(id, name)| (*id, name.split(STORED_SEPARATOR).map(fold::folded).collect()))
        .collect();
    for (_, path) in &paths {
        let mut node = &mut root;
        for component in path {
            node = node.children.entry(component.clone()).or_default();
        }
    }
    root.assign();
    paths
        .into_iter()
        .map(|(id, path)| {
            let mut node = &root;
            let mut ranks = Vec::with_capacity(path.len());
            for component in &path {
                node = &node.children[component];
                ranks.push(node.rank.as_str());
            }
            (id, ranks.join(&STORED_SEPARATOR.to_string()))
        })
        .collect()
}

/// A12.1 with a cache: tags map component by component.
pub struct TagMap<'s> {
    salt: &'s Salt,
    cache: HashMap<String, String>,
}

impl<'s> TagMap<'s> {
    pub fn new(salt: &'s Salt) -> TagMap<'s> {
        TagMap {
            salt,
            cache: HashMap::new(),
        }
    }

    /// Whether A12.1 keeps a tag's first component.
    pub fn kept_first(component: &str) -> bool {
        let folded = fold::folded(component);
        folded == "leech" || folded == "marked"
    }

    /// A12.1: the clone tag.
    pub fn map(&mut self, tag: &str) -> String {
        if let Some(mapped) = self.cache.get(tag) {
            return mapped.clone();
        }
        let mapped = tag
            .split(SEPARATOR)
            .enumerate()
            .map(|(i, component)| {
                if i == 0 && Self::kept_first(component) {
                    component.to_owned()
                } else {
                    self.salt.hash(TAG_DOMAIN, &fold::folded(&normalize::nfc(component)))
                }
            })
            .collect::<Vec<_>>()
            .join(SEPARATOR);
        self.cache.insert(tag.to_owned(), mapped.clone());
        mapped
    }

    /// A12.2: a note's tag string in canonical form, mapped.
    pub fn note_tags(&mut self, tags: &str) -> String {
        let mut mapped: Vec<(String, String)> = split_note_tags(tags)
            .map(|tag| {
                let new = self.map(tag);
                (fold::folded(&new), new)
            })
            .collect();
        // Stable: of tags equal under U2, the first written stays.
        mapped.sort_by(|a, b| a.0.cmp(&b.0));
        mapped.dedup_by(|b, a| a.0 == b.0);
        if mapped.is_empty() {
            return String::new();
        }
        let joined: Vec<&str> = mapped.iter().map(|(_, tag)| tag.as_str()).collect();
        format!(" {} ", joined.join(" "))
    }
}

/// A note's tags: its tag string split at U+0020 and U+3000.
pub fn split_note_tags(tags: &str) -> impl Iterator<Item = &str> {
    tags.split([' ', '\u{3000}']).filter(|t| !t.is_empty())
}
