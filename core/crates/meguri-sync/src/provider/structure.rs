//! `sync/meguri-provider#MP7`: the durable record of a note type structure
//! save admitted for upload. It is written before the save, with the
//! collection's `scm` as it was, and names the binding the save ran under.
//! Once the save committed it also holds the `scm` that the save set. The
//! upload is pending while the collection still carries that stamp, whatever
//! the device clock says against `ls`: undoing the save restores the earlier
//! stamp, and another structural change sets a new one, which this record
//! does not cover.
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
    /// Sync Automatically as stored when the save was admitted, then as
    /// stored when the upload held the binding.
    automatic: bool,
    /// `col.scm` before the save.
    schema_before: i64,
    /// `col.scm` as the committed save left it. Absent until that is recorded.
    #[serde(default)]
    schema_saved: Option<i64>,
}
impl MeguriStructureUpload {
    pub fn binding(&self) -> &MeguriBinding { &self.binding }
    pub fn baseline(&self) -> Option<StructureBaseline> { self.baseline }
    pub fn automatic(&self) -> bool { self.automatic }
    pub fn schema_saved(&self) -> Option<i64> { self.schema_saved }
    /// Whether a collection whose `scm` reads `schema` holds this record's
    /// save. Before the save's stamp is recorded (a kill right after the
    /// commit), any stamp other than the one before the save counts.
    pub fn covers(&self, schema: i64) -> bool {
        match self.schema_saved {
            Some(saved) => schema == saved,
            None => schema != self.schema_before,
        }
    }
    /// The collection the save was made on.
    pub fn publication(&self) -> meguri_engine::device_state::PublicationId { self.binding.publication }
}

impl Authority {
    /// Before the save, under the job's exclusive admission. Requires the
    /// exact ready binding and no other record.
    pub fn arm_meguri_structure_upload(&self, expected: &MeguriBinding, baseline: Option<StructureBaseline>,
        schema_before: i64, generation: u64, publication: Option<Publication>) -> Result<MeguriStructureUpload> {
        let settled = publication.clone();
        let mut armed = None;
        self.bootstrap_update(generation, publication, |doc| {
            check_document(doc, &expected.connection)?;
            if doc.meguri_binding.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::BindingChanged); }
            if readiness(doc, settled.as_ref()) != MeguriReadiness::Ready { return Err(MeguriReadyRefusal::PublicationChanged); }
            if doc.meguri_structure.is_some() { return Err(MeguriReadyRefusal::DocumentChanged); }
            let record = MeguriStructureUpload { binding: expected.clone(), baseline, automatic: doc.settings.automatic,
                schema_before, schema_saved: None };
            doc.meguri_structure = Some(record.clone());
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            armed = Some(record);
            Ok(())
        })?;
        armed.ok_or(MeguriReadyRefusal::DocumentChanged)
    }
    fn replace_meguri_structure(&self, expected: &MeguriStructureUpload, desired: MeguriStructureUpload,
        generation: u64, publication: Option<Publication>,
        also: impl FnOnce(&mut ProviderDocument) -> Result<()>) -> Result<MeguriStructureUpload> {
        let written = desired.clone();
        self.bootstrap_update(generation, publication, |doc| {
            check_document(doc, &expected.binding.connection)?;
            if doc.meguri_structure.as_ref() != Some(expected) { return Err(MeguriReadyRefusal::DocumentChanged); }
            also(doc)?;
            doc.meguri_structure = Some(desired);
            doc.revision = doc.revision.checked_add(1).ok_or_else(invalid)?;
            Ok(())
        })?;
        Ok(written)
    }
    /// After the save committed: the stamp it set is what this record covers.
    pub fn mark_meguri_structure_saved(&self, expected: &MeguriStructureUpload, schema: i64,
        generation: u64, publication: Option<Publication>) -> Result<MeguriStructureUpload> {
        let mut desired = expected.clone();
        desired.schema_saved = Some(schema);
        self.replace_meguri_structure(expected, desired, generation, publication, |_| Ok(()))
    }
    /// The upload is about to run: the binding the record names stops being
    /// ready, in the same write that keeps the Sync Automatically choice as
    /// it is stored now. A binding that an earlier attempt held stays held.
    pub fn hold_meguri_binding_for_structure_upload(&self, expected: &MeguriStructureUpload,
        generation: u64, publication: Option<Publication>) -> Result<MeguriStructureUpload> {
        let snapshot = self.snapshot()?;
        let Some(binding) = snapshot.meguri_binding else { return Ok(expected.clone()) };
        if binding.connection != expected.binding.connection || binding.account != expected.binding.account
            || binding.publication != expected.binding.publication { return Err(MeguriReadyRefusal::BindingChanged); }
        let mut desired = expected.clone();
        desired.automatic = snapshot.settings.automatic;
        let automatic = desired.automatic;
        self.replace_meguri_structure(expected, desired, generation, publication, |doc| {
            if doc.meguri_binding.as_ref() != Some(&binding) || doc.settings.automatic != automatic {
                return Err(MeguriReadyRefusal::BindingChanged);
            }
            doc.meguri_binding = None;
            doc.meguri_settings.automatic = false;
            if doc.selection == Selection::Meguri { doc.settings.automatic = false; }
            Ok(())
        })
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
