//! Operation-authored ordinal origins (S2.5, K19.2, M9.11). These are compact
//! journal metadata, not row images, layout counters or a generation ledger.
use std::collections::{BTreeMap, BTreeSet};
use meguri_model::{ids::NotetypeId, notetype::Notetype};
use crate::StorageError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StructureReason {
    FieldsChanged = 1,
    TemplatesChanged = 2,
    SortFieldChanged = 3,
    NotetypeRemoved = 4,
    NotetypeRestored = 5,
    NotesNotetypeChanged = 6,
    PresetRemoved = 7,
    ApkgApply = 8,
    NotetypesMerged = 9,
    CheckDatabaseRepair = 10,
}

/// Captured on both sides by the author; inverting a deletion needs the old
/// arity even when its last ordinal appears nowhere in the forward map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutShape {
    /// Ordered-list counts; gaps in physical stored keys create no positions.
    pub fields: u32,
    pub templates: u32,
    pub cloze: bool,
    pub sort_field: u32,
}
impl LayoutShape {
    pub fn of(nt: &Notetype) -> Result<Self, StorageError> {
        Ok(Self { fields: Self::checked_count(nt.fields.len())?,
            templates: Self::checked_count(nt.templates.len())?,
            cloze: nt.config.kind == 1, sort_field: nt.config.sort_field_idx })
    }
    pub(crate) fn checked_count(count: usize) -> Result<u32, StorageError> {
        i32::try_from(count).map(|n| n as u32)
            .map_err(|_| invalid("ordered-list count exceeds the map representation"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutReport {
    pub before: LayoutShape,
    pub after: LayoutShape,
    pub field_map: Vec<i32>,
    pub template_map: Vec<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeChangeReport {
    pub before: LayoutShape,
    pub after: LayoutShape,
    pub field_map: Vec<i32>,
    pub template_map: Vec<i32>,
}

fn identity(n: u32) -> Vec<i32> { (0..n).map(|i| i as i32).collect() }
fn is_identity(map: &[i32]) -> bool { map.iter().enumerate().all(|(i, n)| usize::try_from(*n) == Ok(i)) }
/// Whether a map may name one origin at several entries. Only a type
/// change's field map may: Change Note Type copies one old field into
/// several new ones (K19.2 "Repeated field origins"). Layout field maps and
/// every template map stay injective.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origins { Unique, MayRepeat }

fn check_map(map: &[i32], old: u32, new: u32, prefix: bool, origins: Origins) -> Result<(), StorageError> {
    if !prefix && map.len() != new as usize { return Err(invalid("map has the wrong destination arity")); }
    let mut seen = BTreeSet::new();
    for n in map {
        // Cloze ordinals are not template positions. An explicit prefix may
        // refer beyond the number of stored cloze templates (K19.2).
        let repeated = *n >= 0 && !seen.insert(*n) && origins == Origins::Unique;
        if *n < -1 || (!prefix && *n >= old as i32) || repeated {
            return Err(invalid("map has an invalid or repeated origin"));
        }
    }
    Ok(())
}
fn field_origins(pair: bool) -> Origins { if pair { Origins::MayRepeat } else { Origins::Unique } }
fn validate(before: LayoutShape, after: LayoutShape, fields: &[i32], templates: &[i32], pair: bool) -> Result<(), StorageError> {
    check_map(fields, before.fields, after.fields, false, field_origins(pair))?;
    if pair && (before.cloze || after.cloze) {
        let expected = if before.cloze { Vec::new() } else { identity(before.templates) };
        if templates != expected { return Err(invalid("cloze type change must preserve card ordinals")); }
    } else {
        check_map(templates, before.templates, after.templates, before.cloze, Origins::Unique)?;
        if before.cloze && after.cloze && templates.iter().any(|n| *n >= 0 && *n as usize >= templates.len()) {
            return Err(invalid("cloze prefix duplicates an implicit tail origin"));
        }
    }
    Ok(())
}
// Retained compositions are partial injections on endpoint domains. A cloze
// source supplies an identity tail; a standard destination truncates that tail.
fn validate_retained(before: LayoutShape, after: LayoutShape, fields: &[i32], templates: &[i32], pair: bool) -> Result<(), StorageError> {
    check_map(fields, before.fields, after.fields, false, field_origins(pair))?;
    check_map(templates, before.templates, after.templates, before.cloze || after.cloze, Origins::Unique)?;
    if !before.cloze && templates.iter().any(|n| *n >= before.templates as i32) {
        return Err(invalid("map refers to an absent source ordinal"));
    }
    if !before.cloze && templates.is_empty() {
        return Err(invalid("a standard source requires an explicit prefix"));
    }
    if !after.cloze && templates.len() > after.templates as usize {
        return Err(invalid("prefix exceeds the destination domain"));
    }
    if before.cloze && templates.iter().any(|n| *n >= 0 && *n as usize >= templates.len()
        && (after.cloze || (*n as u32) < after.templates)) {
        return Err(invalid("cloze prefix duplicates an implicit tail origin"));
    }
    Ok(())
}
/// One entry per old ordinal: the lowest new ordinal whose entry names it,
/// or -1. A repeated origin (a copied field) reads back from its first copy,
/// so the inverse never repeats (K19.2 "Repeated field origins", Inverse).
fn invert(map: &[i32], old: u32) -> Vec<i32> {
    let mut out = vec![-1; old as usize];
    for (new, old) in map.iter().enumerate() {
        if *old >= 0 && out[*old as usize] < 0 { out[*old as usize] = new as i32; }
    }
    out
}
fn at(map: &[i32], ord: i32, cloze: bool) -> i32 {
    if ord < 0 { -1 } else { map.get(ord as usize).copied().unwrap_or(if cloze { ord } else { -1 }) }
}
fn compose(first: &[i32], second: &[i32], old_cloze: bool, middle_cloze: bool) -> Vec<i32> {
    let len = if old_cloze || middle_cloze { first.len().max(second.len()) } else { second.len() };
    (0..len).map(|i| at(first, at(second, i as i32, middle_cloze), old_cloze)).collect()
}
fn template_at(map: &[i32], ord: i32, before: LayoutShape, after: LayoutShape) -> i32 {
    if ord < 0 || (!after.cloze && ord as u32 >= after.templates) { return -1; }
    let origin = at(map, ord, before.cloze);
    if !before.cloze && origin >= before.templates as i32 { -1 } else { origin }
}
fn invert_templates(map: &[i32], before: LayoutShape, after: LayoutShape) -> Vec<i32> {
    if !before.cloze {
        // The reversed destination is finite, even if the forward one
        // was cloze. Missing forward origins stay -1.
        invert(map, before.templates)
    } else if !after.cloze {
        // Enumerate logical standard destinations, including the checked
        // implicit cloze tail. Never size this by stored template keys.
        let len = map.iter().copied().filter(|n| *n >= 0).map(|n| n as usize + 1)
            .max().unwrap_or(0).max(after.templates as usize);
        let mut out = vec![-1; len];
        for new in 0..after.templates {
            let old = template_at(map, new as i32, before, after);
            if old >= 0 { out[old as usize] = new as i32; }
        }
        out
    } else {
        // Both sides have an implicit identity tail. Admission excludes
        // explicit origins colliding with that tail.
        invert(map, map.len() as u32)
    }
}
fn compose_templates(first: &[i32], second: &[i32], before: LayoutShape, middle: LayoutShape, after: LayoutShape) -> Result<Vec<i32>, StorageError> {
    if before.cloze && after.cloze && !middle.cloze {
        // A finite intermediate domain deleted the infinite cloze tail.
        // No finite prefix with an implicit identity tail can express that.
        return Err(invalid("cloze tail through a standard domain cannot be represented"));
    }
    let len = if after.cloze { first.len().max(second.len()) } else { after.templates as usize };
    Ok((0..len).map(|i| template_at(first, template_at(second, i as i32, middle, after), before, middle)).collect())
}
impl LayoutReport {
    /// Only the checked native edit may assert that every ordinal kept its
    /// origin. Rows alone cannot distinguish a rename from a reorder.
    pub fn identity(nt: &Notetype) -> Result<Self, StorageError> {
        let shape = LayoutShape::of(nt)?;
        let field_map = identity(shape.fields);
        // A full identity prefix is valid for cloze layouts too, and keeps
        // the existing native identity projection usable without changing
        // its consumer. Cloze type-change pairs use the separate empty-map rule.
        let template_map = identity(shape.templates);
        Ok(Self { before: shape, after: shape, field_map, template_map })
    }
    pub fn is_identity(&self) -> bool {
        self.before == self.after && is_identity(&self.field_map) && is_identity(&self.template_map)
    }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        validate(self.before, self.after, &self.field_map, &self.template_map, false)
    }
    fn then(&self, later: &Self) -> Result<Self, StorageError> {
        if self.after != later.before { return Err(invalid("layout contributions have different intermediate shapes")); }
        let out = Self { before: self.before, after: later.after,
            field_map: compose(&self.field_map, &later.field_map, false, false),
            template_map: compose_templates(&self.template_map, &later.template_map, self.before, self.after, later.after)? };
        validate_retained(out.before, out.after, &out.field_map, &out.template_map, false)?;
        Ok(out)
    }
    fn inverse(&self) -> Self {
        let template_map = invert_templates(&self.template_map, self.before, self.after);
        Self { before: self.after, after: self.before, field_map: invert(&self.field_map, self.before.fields), template_map }
    }
}
impl TypeChangeReport {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        validate(self.before, self.after, &self.field_map, &self.template_map, true)
    }
    fn inverse(&self) -> Self {
        let primitive = (self.before.cloze || self.after.cloze)
            && self.template_map == if self.before.cloze { Vec::new() } else { identity(self.before.templates) };
        let template_map = if primitive {
            if self.after.cloze { Vec::new() } else { identity(self.after.templates) }
        } else { invert_templates(&self.template_map, self.before, self.after) };
        Self { before: self.after, after: self.before, field_map: invert(&self.field_map, self.before.fields),
            template_map }
    }
}

/// Sorted, unique K19 reasons, one authored layout per anchor and one map per
/// (old,new) pair. The journal owns this value immutably after COMMIT.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructureReport {
    pub reasons: BTreeSet<StructureReason>,
    pub layouts: BTreeMap<i64, LayoutReport>,
    pub removed: BTreeSet<i64>,
    pub type_changes: BTreeMap<(i64, i64), TypeChangeReport>,
}
impl StructureReport {
    pub fn is_empty(&self) -> bool { self.reasons.is_empty() && self.layouts.is_empty() && self.removed.is_empty() && self.type_changes.is_empty() }
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        for layout in self.layouts.values() { layout.validate()?; }
        for ((old, new), pair) in &self.type_changes {
            if old == new { return Err(invalid("a type change must name different note types")); }
            pair.validate()?;
        }
        Ok(())
    }
    pub(crate) fn validate_retained(&self) -> Result<(), StorageError> {
        for layout in self.layouts.values() {
            validate_retained(layout.before, layout.after, &layout.field_map, &layout.template_map, false)?;
        }
        for ((old, new), pair) in &self.type_changes {
            if old == new { return Err(invalid("a type change must name different note types")); }
            validate_retained(pair.before, pair.after, &pair.field_map, &pair.template_map, true)?;
        }
        Ok(())
    }
    pub fn inverse(&self) -> Self {
        Self { reasons: self.reasons.clone(), removed: BTreeSet::new(),
            layouts: self.layouts.iter().map(|(id, map)| (*id, map.inverse())).collect(),
            type_changes: self.type_changes.iter().map(|((old, new), map)| ((*new, *old), map.inverse())).collect() }
    }
    /// Called in execution order, never journal-key/SQL order. Additional
    /// pair candidates are retained until the guard selects actual final
    /// (old,new) pairs; only the authors supply their ordinal origins.
    pub(crate) fn then(&self, later: &Self) -> Result<Self, StorageError> {
        let mut out = self.clone();
        out.reasons.extend(&later.reasons);
        out.removed.extend(&later.removed);
        for (id, layout) in &later.layouts {
            for ((_, new), pair) in &mut out.type_changes {
                if new == id {
                    if pair.after != layout.before { return Err(invalid("pair destination layout differs")); }
                    pair.field_map = compose(&pair.field_map, &layout.field_map, false, false);
                    pair.template_map = compose_templates(&pair.template_map, &layout.template_map, pair.before, pair.after, layout.after)?;
                    pair.after = layout.after;
                }
            }
            let composed = match out.layouts.get(id) { Some(first) => first.then(layout)?, None => layout.clone() };
            out.layouts.insert(*id, composed);
        }
        for (&(old, new), authored) in &later.type_changes {
            let mut pair = authored.clone();
            if let Some(layout) = self.layouts.get(&old) {
                if layout.after != pair.before { return Err(invalid("pair source layout differs")); }
                pair.field_map = compose(&layout.field_map, &pair.field_map, false, false);
                pair.template_map = compose_templates(&layout.template_map, &pair.template_map, layout.before, layout.after, pair.after)?;
                pair.before = layout.before;
            }
            for (&(origin, destination), first) in &self.type_changes {
                if destination == old && origin != new {
                    if first.after != authored.before { return Err(invalid("type change path has different intermediate shapes")); }
                    let combined = TypeChangeReport { before: first.before, after: authored.after,
                        field_map: compose(&first.field_map, &authored.field_map, false, false),
                        template_map: compose_templates(&first.template_map, &authored.template_map, first.before, first.after, authored.after)? };
                    insert_pair(&mut out.type_changes, (origin, new), combined)?;
                }
            }
            insert_pair(&mut out.type_changes, (old, new), pair)?;
        }
        for id in &later.removed { out.layouts.remove(id); }
        out.validate_retained()?;
        Ok(out)
    }
}
fn insert_pair(pairs: &mut BTreeMap<(i64, i64), TypeChangeReport>, key: (i64, i64), pair: TypeChangeReport) -> Result<(), StorageError> {
    if pairs.get(&key).is_some_and(|old| old != &pair) { return Err(invalid("different authored maps for the same type-change pair")); }
    pairs.insert(key, pair);
    Ok(())
}
pub(crate) fn invalid(detail: &str) -> StorageError { StorageError::Mode { detail: format!("invalid structure report: {detail}") } }
pub(crate) fn missing(id: i64) -> StorageError { StorageError::StructureNotReported { notetype_id: NotetypeId(id) } }

/// K19.2 "Repeated field origins": which maps may repeat an origin, and the
/// lowest-copy inverse.
#[cfg(test)]
mod tests {
    use super::*;

    fn standard(fields: u32) -> LayoutShape {
        LayoutShape { fields, templates: 1, cloze: false, sort_field: 0 }
    }

    fn pair(before: LayoutShape, after: LayoutShape, fields: &[i32]) -> TypeChangeReport {
        TypeChangeReport { before, after, field_map: fields.to_vec(), template_map: vec![0] }
    }

    fn layout(shape: LayoutShape, fields: &[i32]) -> LayoutReport {
        LayoutReport { before: shape, after: shape, field_map: fields.to_vec(), template_map: vec![0] }
    }

    fn report_with_pair(old: i64, new: i64, change: TypeChangeReport) -> StructureReport {
        let mut report = StructureReport::default();
        report.reasons.insert(StructureReason::NotesNotetypeChanged);
        report.type_changes.insert((old, new), change);
        report
    }

    #[test]
    fn a_type_change_field_map_may_repeat_an_origin_authored_and_retained() {
        let report = report_with_pair(1, 2, pair(standard(2), standard(3), &[0, 0, 1]));
        report.validate().unwrap();
        report.validate_retained().unwrap();
    }

    #[test]
    fn layout_field_maps_and_template_maps_still_refuse_a_repeat() {
        let mut layouts = StructureReport::default();
        layouts.layouts.insert(1, layout(standard(2), &[0, 0]));
        assert!(layouts.validate().is_err());
        assert!(layouts.validate_retained().is_err());
        let two = LayoutShape { fields: 1, templates: 2, cloze: false, sort_field: 0 };
        let mut templates = report_with_pair(1, 2, TypeChangeReport {
            before: two, after: two, field_map: vec![0], template_map: vec![1, 1] });
        assert!(templates.validate().is_err());
        assert!(templates.validate_retained().is_err());
        templates.type_changes.clear();
        templates.layouts.insert(1, LayoutReport { before: two, after: two, field_map: vec![0], template_map: vec![0, 0] });
        assert!(templates.validate().is_err());
        assert!(templates.validate_retained().is_err());
    }

    #[test]
    fn a_repeated_origin_inverts_to_its_lowest_copy() {
        // (forward map, old field count, inverse): a highest-copy inverse
        // would give 1, 2 / -1, 1 / 2.
        for (map, old, inverse) in [
            (&[0, 0, 1][..], 2, &[0, 2][..]),
            (&[1, 1, -1][..], 2, &[-1, 0][..]),
            (&[-1, 0, 0][..], 1, &[1][..]),
        ] {
            assert_eq!(invert(map, old), inverse, "{map:?}");
            let new = map.len() as u32;
            let reversed = pair(standard(old), standard(new), map).inverse();
            assert_eq!(reversed.field_map, inverse);
            assert_eq!((reversed.before, reversed.after), (standard(new), standard(old)));
            report_with_pair(2, 1, reversed).validate_retained().unwrap();
        }
    }

    #[test]
    fn the_reports_inverse_reverses_the_pair_with_the_lowest_copy() {
        let report = report_with_pair(1, 2, pair(standard(2), standard(3), &[0, 0, 1]));
        let undo = report.inverse();
        assert_eq!(undo.reasons, report.reasons);
        assert_eq!(undo.type_changes.keys().copied().collect::<Vec<_>>(), [(2, 1)]);
        assert_eq!(undo.type_changes[&(2, 1)].field_map, [0, 2]);
        undo.validate_retained().unwrap();
        // Redo reports the step's own report again; it is admitted.
        report.validate_retained().unwrap();
    }

    /// A's fields reordered, its notes moved to B with 0, 0, 1, then B's
    /// fields reversed: the pair composes to 1, 1, 0 and then 0, 1, 1, and
    /// inverts to 0, 1; the two layouts stay as reported.
    #[test]
    fn a_composed_report_keeps_the_repeat_and_inverts_its_final_map() {
        let (a, b) = (standard(2), standard(3));
        let mut first = StructureReport::default();
        first.reasons.insert(StructureReason::FieldsChanged);
        first.layouts.insert(1, layout(a, &[1, 0]));
        let second = report_with_pair(1, 2, pair(a, b, &[0, 0, 1]));
        let mut third = StructureReport::default();
        third.reasons.insert(StructureReason::FieldsChanged);
        third.layouts.insert(2, layout(b, &[2, 1, 0]));
        let moved = first.then(&second).unwrap();
        assert_eq!(moved.type_changes[&(1, 2)].field_map, [1, 1, 0]);
        let composed = moved.then(&third).unwrap();
        let change = &composed.type_changes[&(1, 2)];
        assert_eq!((change.field_map.as_slice(), change.before, change.after), (&[0, 1, 1][..], a, b));
        assert_eq!(composed.layouts[&1].field_map, [1, 0]);
        assert_eq!(composed.layouts[&2].field_map, [2, 1, 0]);
        composed.validate_retained().unwrap();
        let undo = composed.inverse();
        assert_eq!(undo.type_changes[&(2, 1)].field_map, [0, 1]);
        undo.validate_retained().unwrap();
    }
}
