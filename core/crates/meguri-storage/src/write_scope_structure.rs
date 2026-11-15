//! Checked contribution admission and the S2.5 pre-COMMIT journal guard.
use std::collections::BTreeSet;
use meguri_model::{ids::NotetypeId, proto::{ProtoBlob, notetype::NotetypeConfig}};
use crate::{ReadRows, RowKey, StorageError, Table};
use crate::journal::{After, LayoutShape, StructureReport, StructureReportToken};
use super::WriteScope;

fn invalid(detail: &str) -> StorageError { StorageError::Mode { detail: format!("invalid structure report: {detail}") } }
fn missing(id: i64) -> StorageError { StorageError::StructureNotReported { notetype_id: NotetypeId(id) } }

impl WriteScope<'_> {
    /// Apply only the final accepted, name-uniqued normal-transfer aggregate.
    /// This writes the aggregate itself: callers cannot mark earlier writes.
    /// The executor separately verifies the actual operation source at commit.
    /// No row image or origin map is retained beyond normal journal accounting.
    pub fn put_accepted_sync_notetype(&mut self, nt: &meguri_model::notetype::Notetype) -> Result<crate::Written, StorageError> {
        let mark = self.savepoint()?;
        if let Some(journal) = &mut self.journal { journal.begin_incoming_notetype(nt.id.0); }
        let result = self.put_notetype(nt);
        if let Some(journal) = &mut self.journal { journal.end_incoming_notetype(); }
        match result {
            Ok(written) => { self.release_savepoint(mark)?; Ok(written) }
            Err(error) => { self.rollback_to_savepoint(mark)?; Err(error) }
        }
    }

    /// Even direct storage-helper calls cannot enable an exception in another
    /// executor source, including sources otherwise exempt from S2.5.
    pub fn check_accepted_sync_source(&self, source: meguri_model::changes::OpSource) -> Result<(), StorageError> {
        if self.journal.as_ref().is_some_and(|j| j.incoming_used())
            && source != meguri_model::changes::OpSource::SyncIncoming {
            return Err(invalid("accepted normal-transfer aggregate requires SyncIncoming"));
        }
        Ok(())
    }

    /// Before a write, retain the checked map contribution. A later helper
    /// cannot supply authority for a prior raw edit. Complete the token
    /// only after its writes succeed.
    pub fn start_structure_report(&self, report: StructureReport) -> Result<Option<StructureReportToken>, StorageError> {
        report.validate()?;
        self.start_checked_structure_report(report)
    }

    /// Reuse checked history metadata, whose joined prefixes need not have
    /// primitive shape. The executor must reject this entry point unless its
    /// resolved history target was actually restored in this transaction.
    pub fn start_retained_structure_report(&mut self, report: StructureReport) -> Result<Option<StructureReportToken>, StorageError> {
        report.validate_retained()?;
        let token = self.start_checked_structure_report(report)?;
        if let Some(journal) = &mut self.journal { journal.use_retained_structure(); }
        Ok(token)
    }

    fn start_checked_structure_report(&self, report: StructureReport) -> Result<Option<StructureReportToken>, StorageError> {
        for (&id, layout) in &report.layouts {
            if self.structure_shape(id)? != Some(layout.before) { return Err(invalid("layout before shape differs from stored rows")); }
            self.check_structure_origins(id, &layout.field_map, &layout.template_map, layout.before.cloze)?;
        }
        for id in &report.removed {
            if self.row_image(Table::Notetypes, &RowKey::Id(*id))?.is_none() { return Err(invalid("removed note type is absent before the contribution")); }
        }
        for (&(old, _), pair) in &report.type_changes {
            if self.structure_shape(old)? != Some(pair.before) {
                return Err(invalid("type change source differs from stored rows"));
            }
            // The destination may be added/restored by this contribution.
            // Its authored shape is checked after all covered writes.
            self.check_structure_origins(old, &pair.field_map, &pair.template_map, pair.before.cloze || pair.after.cloze)?;
        }
        self.journal.as_ref().map(|j| j.start_structure_report(report)).transpose()
    }

    pub fn complete_structure_report(&mut self, token: Option<StructureReportToken>) -> Result<(), StorageError> {
        let Some(token) = token else { return Ok(()) };
        for (&id, layout) in &token.contribution.layouts {
            if self.structure_shape(id)? != Some(layout.after) { return Err(invalid("layout after shape differs from stored rows")); }
            self.check_structure_destinations(id, &layout.field_map, &layout.template_map, layout.after.cloze)?;
        }
        for id in &token.contribution.removed {
            if self.row_image(Table::Notetypes, &RowKey::Id(*id))?.is_some() { return Err(invalid("removed note type still exists")); }
        }
        for (&(_, new), pair) in &token.contribution.type_changes {
            if self.structure_shape(new)? != Some(pair.after) { return Err(invalid("type change destination differs after writes")); }
            self.check_structure_destinations(new, &pair.field_map, &pair.template_map, pair.before.cloze || pair.after.cloze)?;
        }
        let journal = self.journal.as_mut().ok_or_else(|| invalid("contribution lost its journal"))?;
        journal.complete_structure_report(token)
    }

    fn check_structure_origins(&self, id: i64, fields: &[i32], templates: &[i32], prefix: bool) -> Result<(), StorageError> {
        let shape = self.structure_shape(id)?.ok_or_else(|| invalid("missing map source"))?;
        if fields.iter().any(|n| *n >= 0 && *n as u32 >= shape.fields)
            || (!prefix && templates.iter().any(|n| *n >= 0 && *n as u32 >= shape.templates)) {
            return Err(invalid("map refers to an absent source ordinal"));
        }
        Ok(())
    }

    fn check_structure_destinations(&self, id: i64, fields: &[i32], templates: &[i32], prefix: bool) -> Result<(), StorageError> {
        let shape = self.structure_shape(id)?.ok_or_else(|| invalid("missing map destination"))?;
        if fields.iter().enumerate().any(|(ord, n)| *n >= 0 && ord >= shape.fields as usize)
            || (!prefix && templates.iter().enumerate().any(|(ord, n)| *n >= 0 && ord >= shape.templates as usize)) {
            return Err(invalid("map fills an absent destination ordinal"));
        }
        Ok(())
    }

    fn structure_shape(&self, id: i64) -> Result<Option<LayoutShape>, StorageError> {
        self.notetype(NotetypeId(id))?.as_ref().map(LayoutShape::of).transpose()
    }

    /// Reads the actual transaction-start head. A recovered missing type
    /// stays absent here; no layout is fabricated from its new rows.
    fn structure_head_before(&self, id: i64) -> Result<Option<crate::RowImage>, StorageError> {
        if let Some(j) = &self.journal && let Some(e) = j.entry(Table::Notetypes, &RowKey::Id(id)) {
            return j.before(e);
        }
        self.row_image(Table::Notetypes, &RowKey::Id(id))
    }

    fn structure_shape_before(&self, id: i64) -> Result<Option<LayoutShape>, StorageError> {
        let Some(head) = self.structure_head_before(id)? else { return Ok(None) };
        let row = crate::rows::RowCells { table: Table::Notetypes, cells: &head };
        let config: ProtoBlob<NotetypeConfig> = row.wrap(4, ProtoBlob::decode(row.blob(4)?))?;
        let current = self.notetype(NotetypeId(id))?;
        let mut fields: BTreeSet<u32> = current.as_ref().into_iter().flat_map(|n| n.fields.iter().map(|f| f.ord)).collect();
        let mut templates: BTreeSet<u32> = current.as_ref().into_iter().flat_map(|n| n.templates.iter().map(|t| t.ord)).collect();
        if let Some(j) = &self.journal {
            for e in j.entries() {
                if let RowKey::Pair(ntid, ord) = e.key() && *ntid == id {
                    let set = match e.table() { Table::Fields => &mut fields, Table::Templates => &mut templates, _ => continue };
                    let ord = u32::try_from(*ord).map_err(|_| invalid("invalid transaction-start ordinal"))?;
                    if e.existed_before() { set.insert(ord); } else { set.remove(&ord); }
                }
            }
        }
        Ok(Some(LayoutShape { fields: LayoutShape::checked_count(fields.len())?, templates: LayoutShape::checked_count(templates.len())?,
            cloze: config.kind == 1, sort_field: config.sort_field_idx }))
    }

    /// Executor-only policy: called for every source except CloudIncoming
    /// and CloudRekey, after the SCM guard and before SQLite COMMIT.
    /// It secures only compact facts and reads at most the current image;
    /// ordinary card/revlog/note-content writes require no additional reads.
    pub fn check_structure_report(&mut self) -> Result<(), StorageError> {
        let Some(journal) = &self.journal else { return Ok(()) };
        let mut affected = BTreeSet::new();
        let mut removed = BTreeSet::new();
        let mut pairs = BTreeSet::new();
        for e in journal.entries() {
            match (e.table(), e.key()) {
                (Table::Fields | Table::Templates, RowKey::Pair(id, _)) => {
                    let changed = if e.existed_before() != e.exists_after() { true }
                        else if e.existed_before() {
                            let before = journal.before(e)?.ok_or_else(|| invalid("missing part before image"))?;
                            let after = journal.after(e)?.ok_or_else(|| invalid("missing part after image"))?;
                            before.get(2) != after.get(2)
                        } else { false };
                    if changed { affected.insert(*id); }
                }
                (Table::Notetypes, RowKey::Id(id)) if e.existed_before() && !e.exists_after() => { removed.insert(*id); }
                (Table::Notetypes, RowKey::Id(id)) if e.existed_before() && e.exists_after() => {
                    let before = self.structure_shape_before(*id)?.ok_or_else(|| invalid("missing note type before shape"))?;
                    let after = self.structure_shape(*id)?.ok_or_else(|| invalid("missing note type after shape"))?;
                    if before.sort_field != after.sort_field || before.cloze != after.cloze { affected.insert(*id); }
                }
                (Table::Notes, key) if e.existed_before() && e.exists_after()
                    && matches!(e.after(), After::Note(digest) if digest.changed.contains(2)) => {
                    let before = journal.before(e)?.ok_or_else(|| invalid("missing note before image"))?;
                    let after = self.row_image(Table::Notes, key)?.ok_or_else(|| invalid("missing changed note"))?;
                    let mid = |row: &crate::RowImage| row.get(2).and_then(crate::Cell::as_i64).ok_or_else(|| invalid("invalid note mid"));
                    let (old, new) = (mid(&before)?, mid(&after)?);
                    if old != new && self.structure_head_before(old)?.is_some() { pairs.insert((old, new)); }
                }
                _ => {}
            }
        }
        let report = journal.structure_report();
        // Coverage belongs only to the bracketed writes, even when an outside
        // write sequence returns to identical endpoints.
        for &id in journal.incoming_layouts() {
            if journal.layout_unreported(id) { return Err(missing(id)); }
        }
        // Retained authored maps need authority even when later writes cancel
        // every net layout change. Completed contributions clear invalidation;
        // unreported writes do not, regardless of endpoint equality.
        if let Some(report) = report {
            for &id in report.layouts.keys() {
                if journal.layout_unreported(id) { return Err(missing(id)); }
            }
        }
        for id in affected {
            let Some(before) = self.structure_shape_before(id)? else { continue };
            let Some(after) = self.structure_shape(id)? else { continue };
            if journal.incoming_layout(id) { continue; }
            let map = report.and_then(|r| r.layouts.get(&id)).ok_or_else(|| missing(id))?;
            if journal.layout_unreported(id) || map.before != before || map.after != after { return Err(missing(id)); }
        }
        for id in removed {
            if journal.layout_unreported(id) || !report.is_some_and(|r| r.removed.contains(&id)) { return Err(missing(id)); }
        }
        for &(old, new) in &pairs {
            let pair = report.and_then(|r| r.type_changes.get(&(old, new))).ok_or_else(|| missing(old))?;
            if journal.unreported_moves().any(|(from, _)| *from == old)
                || self.structure_shape_before(old)? != Some(pair.before) || self.structure_shape(new)? != Some(pair.after) {
                return Err(missing(old));
            }
        }
        self.journal.as_mut().expect("journal checked above").retain_structure_pairs(&pairs);
        Ok(())
    }
}
