//! `sync/meguri-provider#MP7`: the durable record of a note type structure
//! save admitted for upload. It is written before the save and names the
//! binding the save ran under. Whether the upload is pending is never stored:
//! the Host reads it from the collection (`scm > ls`), so a save that never
//! committed, or was undone, withdraws it without a second write.
use super::bootstrap::{Result, check_document, readiness};
use super::*;

/// The server's collection facts this device is proved to hold: `col.ls`
/// and `col.scm` as a completed round or bootstrap left them, or the facts
/// read after this record's own retained server copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureBaseline {
    pub modified: i64,
    pub schema_modified: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeguriStructureUpload {
    binding: MeguriBinding,
    /// Absent when no baseline is proved: the upload then needs MP6.3's choice.
    baseline: Option<StructureBaseline>,
    /// Sync Automatically as stored when the save was admitted.
    automatic: bool,
}
impl MeguriStructureUpload {
    pub fn binding(&self) -> &MeguriBinding { &self.binding }
    pub fn baseline(&self) -> Option<StructureBaseline> { self.baseline }
    pub fn automatic(&self) -> bool { self.automatic }
    /// The collection the save was made on.
    pub fn publication(&self) -> meguri_engine::device_state::PublicationId { self.binding.publication }
}

impl Authority {
    /// Before the save, under the job's exclusive admission. Requires the
    /// exact ready binding and no other record.
    pub fn arm_meguri_structure_upload(&self, expected: &MeguriBinding, baseline: Option<StructureBaseline>,
        generation: u64, publication: Option<Publication>) -> Result<MeguriStructureUpload> {
        let settled = publication.clone();
        let mut armed = None;
        self.bootstrap_update(generation, publication, |doc| {
            check_document(doc, &expected.connection)?;
            if doc.meguri_binding.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::BindingChanged); }
            if readiness(doc, settled.as_ref()) != MeguriReadiness::Ready { return Err(MeguriReadyRefusal::PublicationChanged); }
            if doc.meguri_structure.is_some() { return Err(MeguriReadyRefusal::DocumentChanged); }
            let record = MeguriStructureUpload { binding: expected.clone(), baseline, automatic: doc.settings.automatic };
            doc.meguri_structure = Some(record.clone());
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            armed = Some(record);
            Ok(())
        })?;
        armed.ok_or(MeguriReadyRefusal::DocumentChanged)
    }
    /// After this record's own server copy was retained: the facts read
    /// after that download describe the same server content.
    pub fn advance_meguri_structure_baseline(&self, expected: &MeguriStructureUpload, baseline: StructureBaseline,
        generation: u64, publication: Option<Publication>) -> Result<MeguriStructureUpload> {
        let mut advanced = expected.clone();
        advanced.baseline = Some(baseline);
        let desired = advanced.clone();
        self.bootstrap_update(generation, publication, |doc| {
            check_document(doc, &expected.binding.connection)?;
            if doc.meguri_structure.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::DocumentChanged); }
            doc.meguri_structure = Some(desired);
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })?;
        Ok(advanced)
    }
    /// Nothing was saved, or the save was undone. Only the exact record goes.
    pub fn clear_meguri_structure_upload(&self, expected: &MeguriStructureUpload,
        generation: u64, publication: Option<Publication>) -> Result<()> {
        self.bootstrap_update(generation, publication, |doc| {
            if doc.meguri_structure.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::DocumentChanged); }
            doc.meguri_structure = None;
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })
    }
}
