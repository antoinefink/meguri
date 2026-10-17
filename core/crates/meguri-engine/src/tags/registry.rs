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

/// T3 for the names of one operation: the spelling each normalized name
/// takes, registering it when no row is equal to it. Spellings are kept, so
/// an operation that spells one name many times reads the registry once for
/// it: once spelled, a name is equal to a row until the operation removes
/// rows again.
#[derive(Debug, Default)]
pub struct Speller {
    spelled: HashMap<String, String>,
}

impl Speller {
    pub fn new() -> Speller {
        Speller::default()
    }

    /// T3: the spelling of `normalized` (a T2 result), registered as a new
    /// row when no row is equal to it.
    pub fn spell(&mut self, scope: &mut WriteScope<'_>, normalized: &str) -> Result<String, StorageError> {
        self.spell_with(scope, normalized, new_row)
    }

    /// T3 steps 1 and 2, registering a spelling no row is equal to as the
    /// row `register` makes of it: a renamed or moved tag's row takes its
    /// source's columns (T7 step 5).
    pub fn spell_with(
        &mut self,
        scope: &mut WriteScope<'_>,
        normalized: &str,
        register: impl FnOnce(&str) -> Tag,
    ) -> Result<String, StorageError> {
        if let Some(spelling) = self.spelled.get(normalized) {
            return Ok(spelling.clone());
        }
        let spelling = match spelling_of(&*scope, normalized)? {
            Spelling::Registered(name) => name,
            Spelling::New(name) => {
                scope.put(&register(&name))?;
                name
            }
        };
        self.spelled.insert(normalized.to_owned(), spelling.clone());
        Ok(spelling)
    }
}

/// What T3 gives a normalized name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spelling {
    /// Step 1: the spelling of the row equal to the name.
    Registered(String),
    /// Step 2: the spelling to register.
    New(String),
}

/// T3 on the registry as `reader` sees it, registering nothing. A row equal
/// to `normalized` lends its spelling (step 1). Otherwise the longest
/// leading part of `normalized` that is a node (T1), trying the name itself
/// and then its ancestors from its parent outward, lends its tree spelling
/// to that part (step 2).
pub fn spelling_of(reader: &(impl ReadRows + ?Sized), normalized: &str) -> Result<Spelling, StorageError> {
    if let Some(row) = reader.tag(normalized)? {
        return Ok(Spelling::Registered(row.name));
    }
    if let Some(node) = implicit_spelling(reader, normalized)? {
        return Ok(Spelling::New(node));
    }
    for ancestor in ancestors(normalized) {
        let node = match reader.tag(ancestor)? {
            Some(row) => Some(row.name),
            None => implicit_spelling(reader, ancestor)?,
        };
        if let Some(node) = node {
            return Ok(Spelling::New(format!("{node}{}", &normalized[ancestor.len()..])));
        }
    }
    Ok(Spelling::New(normalized.to_owned()))
}

/// T1's tree spelling of `name` as an implicit parent: its components as
/// the first row, in `unicase` order, that has an ancestor equal to `name`
/// spells them; `None` when no row has. Every such row folds to a text
/// beginning with `name`'s folding and `::`, so the first row at or after
/// `name::` in key order is the one, when there is one. Whether `name` is an
/// ancestor of a row beginning that way depends on `name` alone (`a:` is
/// none of `a:::b`'s, whose components are `a` and `:b`), so when that row
/// does not have it, no row does.
fn implicit_spelling(reader: &(impl ReadRows + ?Sized), name: &str) -> Result<Option<String>, StorageError> {
    let below = format!("{name}{LEVEL}");
    let Some(row) = reader.tag_rows_from(&below, 1)?.pop() else {
        return Ok(None);
    };
    if !folded(&row.name).starts_with(&folded(&below)) {
        return Ok(None);
    }
    Ok(ancestors(&row.name)
        .find(|ancestor| compare_folded(ancestor, name) == Ordering::Equal)
        .map(str::to_owned))
}

/// T3: the spelling of `normalized`, registered as a new row when no row is
/// equal to it.
pub fn spell(scope: &mut WriteScope<'_>, normalized: &str) -> Result<String, StorageError> {
    Speller::new().spell(scope, normalized)
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
    fn the_t13_rules() {
        // Implicit parents, spelled as the first row in unicase order spells
        // them, and collapsed.
        let t = tree(&rows(&[("KITE::wing::right", false), ("kite::WING::left", false)]));
        assert_eq!(show(&t), "kite*(WING*(left right))");
        assert!(t[0].implicit && t[0].children[0].implicit);
        assert_eq!(t[0].children[0].children[1].full_name, "KITE::wing::right");
        // `-` sorts before `:`, yet a child stays under its parent and the
        // longer sibling comes after it.
        assert_eq!(
            show(&tree(&rows(&[
                ("lamp-post", false),
                ("lamp::shade", false),
                ("lamp", false)
            ]))),
            "lamp(shade) lamp-post"
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
