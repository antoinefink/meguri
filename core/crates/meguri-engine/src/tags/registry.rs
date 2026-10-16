//! The tag registry, the `tags` table (`specs/tags/tags.md` T3, T4, T12,
//! T13, T14): the spelling a normalized name takes and the rows it
//! registers, the tags a note save stores, the collapsed state and the tree
//! the sidebar shows.
//!
//! Equality of names is `unicase` full folding (`text/unicode-primitives`
//! U2), which is also how the table's key compares: `ReadRows::tag(name)`
//! finds the row equal to `name`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use meguri_model::tag::Tag;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, StorageError, WriteScope};
use meguri_text::fold::{compare_folded, folded};

use super::canonical::{LEVEL, ancestors, components, join_note_tags, normalize, split_text};

/// A new row (T3, T14): `usn` -1, collapsed, no `config`.
pub fn new_row(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        usn: Usn::PENDING,
        collapsed: true,
        config: None,
    }
}

/// T3 for one normalized name: the spelling it takes, registering it when
/// no row is equal to it. Spellings are kept, so an operation that spells
/// one name many times reads the registry once for it: once spelled, a name
/// is equal to a row until the operation removes rows again.
#[derive(Debug, Default)]
pub struct Speller {
    spelled: HashMap<String, String>,
}

impl Speller {
    pub fn new() -> Speller {
        Speller::default()
    }

    /// T3: the spelling of `normalized` (a T2 result).
    pub fn spell(&mut self, scope: &mut WriteScope<'_>, normalized: &str) -> Result<String, StorageError> {
        if let Some(spelling) = self.spelled.get(normalized) {
            return Ok(spelling.clone());
        }
        let spelling = spell(scope, normalized)?;
        self.spelled.insert(normalized.to_owned(), spelling.clone());
        Ok(spelling)
    }
}

/// T3: a row equal to `normalized` lends its spelling and nothing is
/// registered; otherwise the nearest registered ancestor lends its spelling
/// to the leading components, and the result is registered as a new row.
pub fn spell(scope: &mut WriteScope<'_>, normalized: &str) -> Result<String, StorageError> {
    if let Some(row) = scope.tag(normalized)? {
        return Ok(row.name);
    }
    let mut spelling = normalized.to_owned();
    for ancestor in ancestors(normalized) {
        if let Some(row) = scope.tag(ancestor)? {
            spelling = format!("{}{}", row.name, &normalized[ancestor.len()..]);
            break;
        }
    }
    scope.put(&new_row(&spelling))?;
    Ok(spelling)
}

/// T4: what a note save stores for the tag list it is given (the editor,
/// the Add screen, AnkiConnect, U27's note ops), registering every tag that
/// is new. Items are split at separators; each tag is normalized and spelled
/// in list order, tags equal to an earlier one are dropped, and the rest are
/// sorted by `unicase` and joined.
pub fn note_tags_for_save<S: AsRef<str>>(scope: &mut WriteScope<'_>, items: &[S]) -> Result<String, StorageError> {
    let mut speller = Speller::new();
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for item in items {
        for piece in split_text(item.as_ref()) {
            let spelling = speller.spell(scope, &normalize(piece))?;
            if seen.insert(folded(&spelling)) {
                tags.push(spelling);
            }
        }
    }
    tags.sort_by(|a, b| compare_folded(a, b));
    Ok(join_note_tags(&tags))
}

/// T12 within `scope`: each name normalized (separators removed, not
/// split), spelled and registered when new, then its row's `collapsed` set.
/// A row that already has the state is not written; a changed one gets
/// `usn` -1 from the scope's stamping. Returns how many rows changed.
pub fn set_collapsed_in<S: AsRef<str>>(
    scope: &mut WriteScope<'_>,
    names: &[S],
    collapsed: bool,
) -> Result<usize, StorageError> {
    let mut changed = 0;
    for name in names {
        let spelling = spell(scope, &normalize(name.as_ref()))?;
        let Some(mut row) = scope.tag(&spelling)? else {
            continue;
        };
        if row.collapsed != collapsed {
            row.collapsed = collapsed;
            if scope.put(&row)?.changed() {
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// One node of the tag tree (T13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagNode {
    /// The component this node adds.
    pub name: String,
    /// The node's whole name, as the tree spells it: what the sidebar
    /// searches for and passes to reparent and collapse (T13).
    pub full_name: String,
    pub collapsed: bool,
    /// Whether no row has this name: an implicit parent.
    pub implicit: bool,
    pub children: Vec<TagNode>,
}

struct Building {
    node: TagNode,
    children: Vec<usize>,
}

/// T13: the registry as a tree. One node per row, collapsed as the row is,
/// and one per implicit parent (an ancestor of a row that no row equals),
/// collapsed and spelled as the first row in `unicase` order of `tag` that
/// has it as an ancestor spells it. Children are ordered by `unicase` of
/// their component names; the result is the top-level nodes in that order.
pub fn tree(rows: &[Tag]) -> Vec<TagNode> {
    let mut sorted: Vec<&Tag> = rows.iter().collect();
    sorted.sort_by(|a, b| compare_folded(&a.name, &b.name));
    let mut nodes: Vec<Building> = Vec::with_capacity(sorted.len());
    let mut by_name: HashMap<String, usize> = HashMap::with_capacity(sorted.len());
    let mut roots: Vec<usize> = Vec::new();

    fn attach(nodes: &mut Vec<Building>, roots: &mut Vec<usize>, parent: Option<usize>, node: TagNode) -> usize {
        let index = nodes.len();
        nodes.push(Building {
            node,
            children: Vec::new(),
        });
        match parent {
            Some(parent) => nodes[parent].children.push(index),
            None => roots.push(index),
        }
        index
    }

    for row in sorted {
        // Ancestors from the first component down, each made implicit when
        // no node has its name yet. A row sorts after every name equal to one
        // of its ancestors, so an ancestor that is a row has its node by now.
        let mut parent = None;
        let mut start = 0;
        let parts: Vec<&str> = components(&row.name).collect();
        for (depth, part) in parts.iter().enumerate() {
            let end = start + part.len();
            let full = &row.name[..end];
            let last = depth + 1 == parts.len();
            let key = folded(full);
            let existing = by_name.get(&key).copied();
            let index = match existing {
                Some(index) if !last => index,
                Some(index) => {
                    // Unreachable for a table keyed by unicase; keep the row's
                    // state should two rows ever fold alike.
                    nodes[index].node.collapsed = row.collapsed;
                    nodes[index].node.implicit = false;
                    index
                }
                None => {
                    let node = TagNode {
                        name: (*part).to_owned(),
                        full_name: full.to_owned(),
                        collapsed: if last { row.collapsed } else { true },
                        implicit: !last,
                        children: Vec::new(),
                    };
                    let index = attach(&mut nodes, &mut roots, parent, node);
                    by_name.insert(key, index);
                    index
                }
            };
            parent = Some(index);
            start = end + LEVEL.len();
        }
    }

    fn finish(nodes: &mut [Option<Building>], index: usize) -> TagNode {
        let Building { mut node, children } = nodes[index].take().expect("each node is finished once");
        node.children = children.into_iter().map(|child| finish(nodes, child)).collect();
        node.children.sort_by(by_component);
        node
    }
    let mut slots: Vec<Option<Building>> = nodes.into_iter().map(Some).collect();
    let mut top: Vec<TagNode> = roots.into_iter().map(|root| finish(&mut slots, root)).collect();
    top.sort_by(by_component);
    top
}

fn by_component(a: &TagNode, b: &TagNode) -> Ordering {
    compare_folded(&a.name, &b.name)
}

/// T13 on the stored registry.
pub fn read_tree(reader: &(impl ReadRows + ?Sized)) -> Result<Vec<TagNode>, StorageError> {
    Ok(tree(&reader.tag_rows()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(names: &[(&str, bool)]) -> Vec<Tag> {
        names
            .iter()
            .map(|(name, collapsed)| Tag {
                name: (*name).to_owned(),
                usn: Usn(0),
                collapsed: *collapsed,
                config: None,
            })
            .collect()
    }

    /// A tree as `name[*](children)`, `*` for collapsed.
    fn show(nodes: &[TagNode]) -> String {
        nodes
            .iter()
            .map(|n| {
                let star = if n.collapsed { "*" } else { "" };
                if n.children.is_empty() {
                    format!("{}{star}", n.name)
                } else {
                    format!("{}{star}({})", n.name, show(&n.children))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_t13_table() {
        let t = tree(&rows(&[("foo::BAR::a", false), ("FOO::bar::b", false)]));
        assert_eq!(show(&t), "foo*(BAR*(a b))");
        assert!(t[0].implicit && t[0].children[0].implicit);
        assert_eq!(t[0].children[0].children[1].full_name, "FOO::bar::b");
        assert_eq!(
            show(&tree(&rows(&[("one", false), ("one1", false), ("one::two", false)]))),
            "one(two) one1"
        );
        assert_eq!(show(&tree(&rows(&[("Bar::x", false), ("bar::y", false)]))), "Bar*(x y)");
        assert_eq!(
            show(&tree(&rows(&[
                ("a!", false),
                ("a::b", false),
                ("B", false),
                ("b::c", false)
            ]))),
            "a*(b) a! B(c)"
        );
        assert!(tree(&[]).is_empty());
    }
}
