//! The note types a plan reads (`specs/search/text-matching.md` T7, T9.1,
//! T15; `fields.md` F2): each type's kind, sort field and fields with their
//! "exclude from unqualified searches" option. The caller reads them on the
//! read transaction the statement runs on, or builds the plan again when the
//! note types changed (T15); a cache keyed on the context values alone does
//! not suffice.

use std::cmp::Ordering;

use meguri_model::ids::NotetypeId;
use meguri_storage::{ReadRows, StorageError, Table};

/// Every note type of the collection, types without notes included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchNotetypes {
    /// Ascending id.
    types: Vec<NotetypeShape>,
}

/// What the text terms read of one note type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotetypeShape {
    pub id: NotetypeId,
    pub name: String,
    /// Of the cloze kind (`type` 1, T9.1).
    pub cloze: bool,
    /// `sortf` (`sort_field_idx`).
    pub sort_field: u32,
    /// In field order.
    pub fields: Vec<FieldShape>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldShape {
    pub name: String,
    /// `exclude_from_search` (T7.1).
    pub excluded: bool,
}

impl SearchNotetypes {
    pub fn new(mut types: Vec<NotetypeShape>) -> SearchNotetypes {
        types.sort_by_key(|shape| shape.id.0);
        SearchNotetypes { types }
    }

    /// Reads every note type through `rows`.
    pub fn read(rows: &impl ReadRows) -> Result<SearchNotetypes, StorageError> {
        const PAGE: usize = 250;
        let mut types = Vec::new();
        let mut after = None;
        loop {
            let ids = rows.ids_after(Table::Notetypes, after, PAGE)?;
            for &id in &ids {
                if let Some(notetype) = rows.notetype(NotetypeId(id))? {
                    types.push(NotetypeShape {
                        id: notetype.id,
                        name: notetype.name,
                        cloze: notetype.config.kind == 1,
                        sort_field: notetype.config.sort_field_idx,
                        fields: notetype
                            .fields
                            .into_iter()
                            .map(|field| FieldShape {
                                name: field.name,
                                excluded: field.config.exclude_from_search,
                            })
                            .collect(),
                    });
                }
            }
            if ids.len() < PAGE {
                break;
            }
            after = ids.last().copied();
        }
        Ok(SearchNotetypes::new(types))
    }

    /// Ascending id.
    pub fn types(&self) -> &[NotetypeShape] {
        &self.types
    }

    /// T7.1: some note type has an excluded field.
    pub(crate) fn any_excluded(&self) -> bool {
        self.types.iter().any(NotetypeShape::has_excluded)
    }

    /// T16's `{notetype}`: the first type without an included field, by the
    /// unicase order of names, then by id.
    pub(crate) fn first_without_included(&self) -> Option<&NotetypeShape> {
        self.types.iter().filter(|shape| !shape.has_included()).min_by(|a, b| {
            match meguri_text::fold::compare_folded(&a.name, &b.name) {
                Ordering::Equal => a.id.0.cmp(&b.id.0),
                other => other,
            }
        })
    }
}

impl NotetypeShape {
    pub(crate) fn has_excluded(&self) -> bool {
        self.fields.iter().any(|field| field.excluded)
    }

    pub(crate) fn has_included(&self) -> bool {
        self.fields.iter().any(|field| !field.excluded)
    }

    /// The included positions as runs of consecutive positions (T7.2),
    /// inclusive `(start, end)`, ascending.
    pub(crate) fn included_runs(&self) -> Vec<(usize, usize)> {
        let positions: Vec<usize> = (0..self.fields.len()).filter(|&k| !self.fields[k].excluded).collect();
        runs(&positions)
    }

    /// The included positions, ascending.
    pub(crate) fn included(&self) -> Vec<usize> {
        (0..self.fields.len()).filter(|&k| !self.fields[k].excluded).collect()
    }

    /// T7.2: the sort text is tested unless the sort field is excluded. A
    /// sort field past the last field is no field, so nothing excludes it.
    pub(crate) fn sort_field_included(&self) -> bool {
        usize::try_from(self.sort_field)
            .ok()
            .and_then(|k| self.fields.get(k))
            .is_none_or(|field| !field.excluded)
    }
}

/// Maximal runs of consecutive positions, from ascending `positions`.
pub(crate) fn runs(positions: &[usize]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &k in positions {
        match out.last_mut() {
            Some((_, end)) if *end + 1 == k => *end = k,
            _ => out.push((k, k)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(id: i64, name: &str, excluded: &[bool]) -> NotetypeShape {
        NotetypeShape {
            id: NotetypeId(id),
            name: name.to_owned(),
            cloze: false,
            sort_field: 0,
            fields: excluded
                .iter()
                .enumerate()
                .map(|(k, &excluded)| FieldShape {
                    name: format!("f{k}"),
                    excluded,
                })
                .collect(),
        }
    }

    #[test]
    fn included_positions_form_maximal_runs() {
        assert_eq!(runs(&[0, 1, 3, 5, 6, 7]), vec![(0, 1), (3, 3), (5, 7)]);
        assert_eq!(runs(&[]), vec![]);
        let memo = shape(1, "Memo", &[false, false, true]);
        assert_eq!(memo.included_runs(), vec![(0, 1)]);
        assert_eq!(
            shape(1, "x", &[true, false, true, false]).included_runs(),
            vec![(1, 1), (3, 3)]
        );
    }

    // T16: `b` and `B2`, or `Z` and `a`, give `b`, or `a`.
    #[test]
    fn the_named_type_is_first_by_unicase_name_then_id() {
        let set = SearchNotetypes::new(vec![shape(5, "B2", &[true]), shape(9, "b", &[true])]);
        assert_eq!(set.first_without_included().unwrap().name, "b");
        let set = SearchNotetypes::new(vec![shape(1, "Z", &[true]), shape(2, "a", &[true])]);
        assert_eq!(set.first_without_included().unwrap().name, "a");
        let set = SearchNotetypes::new(vec![shape(3, "A", &[true]), shape(2, "a", &[true])]);
        assert_eq!(set.first_without_included().unwrap().id, NotetypeId(2));
        let set = SearchNotetypes::new(vec![shape(3, "A", &[false, true])]);
        assert!(set.first_without_included().is_none());
    }
}
