//! The plan of an apply (`package/apkg-import` I14.1 step 3): note types
//! (I7 or the merge), then each package note's decision (I9), all in memory
//! against one snapshot of the target and the rows taken from the package.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use meguri_engine::{CancelToken, ProgressSink};
use meguri_model::ids::NoteId;
use meguri_model::notetype::Notetype;
use meguri_storage::{NoteIdentity, ReadRows};

use super::gather::{RawPackage, Taken};
use super::media::MediaOutcome;
use super::notetypes::{TypeStage, TypeTarget, plan_without_merge};
use super::{ImportOptions, ImportOutcome, UpdateCondition};

/// What I9 decided for one package note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    New,
    Updated { target: NoteId },
    /// A target note holds the GUID and the update rule does not hold.
    Duplicate { target: NoteId },
    /// An earlier counted package note holds the GUID (step 4): the index
    /// of that note's plan, and whether both go to one note type.
    DuplicateOf { earlier: usize, same_type: bool },
    Conflicting { existing: NoteId },
    MissingNotetype,
}

#[derive(Clone, Debug)]
pub(crate) struct NotePlan {
    pub decision: Decision,
    /// The package `mid`, the key of the stage's routes.
    pub package_type: i64,
    /// The field values after the merge's field map; for new and updated
    /// notes, after the media rewrite once staging ran.
    pub fields: Vec<String>,
}

pub(crate) struct ImportPlan {
    pub options: ImportOptions,
    /// The target's day at the plan (I11.2's T) and the package's (P).
    pub target_today: u32,
    pub package_today: u32,
    pub stage: TypeStage,
    /// One per package note, in ascending id.
    pub notes: Vec<NotePlan>,
    pub raw: Arc<RawPackage>,
    pub taken: Arc<Taken>,
    pub media: MediaOutcome,
}

impl ImportPlan {
    /// The rows the write phase is expected to write (I14.2's threshold).
    pub fn rows(&self) -> usize {
        let notes = self
            .notes
            .iter()
            .filter(|note| matches!(note.decision, Decision::New | Decision::Updated { .. }))
            .count();
        notes
            + self.taken.cards.len()
            + self.taken.revlog.len()
            + self.stage.writes.len()
            + usize::try_from(self.stage.rows).unwrap_or(usize::MAX / 2)
    }
}

fn update_holds(rule: UpdateCondition, target: i64, incoming: i64) -> bool {
    match rule {
        UpdateCondition::IfNewer => target < incoming,
        UpdateCondition::Always => target != incoming,
        UpdateCondition::Never => false,
    }
}

/// The target's notes that hold a GUID of the package, by GUID.
fn matching_notes<'p>(
    reader: &impl ReadRows,
    raw: &'p RawPackage,
    cancel: &CancelToken,
) -> ImportOutcome<HashMap<&'p [u8], NoteIdentity>> {
    let wanted: HashMap<&[u8], ()> = raw.notes.iter().map(|note| (note.guid.as_bytes(), ())).collect();
    let mut found = HashMap::new();
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.note_identities_after(after, 5000)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for identity in page {
            if let Some((guid, ())) = wanted.get_key_value(identity.guid.as_slice()) {
                // Two target notes with one GUID: the first by id is matched.
                found.entry(*guid).or_insert(identity);
            }
        }
    }
    Ok(found)
}

/// The field values of a package note as its route's note type holds them.
fn routed_fields(stored: &str, map: Option<&Vec<Option<usize>>>, record: &Notetype) -> Vec<String> {
    let values: Vec<&str> = stored.split('\u{1f}').collect();
    match map {
        Some(map) => map
            .iter()
            .map(|origin| origin.and_then(|at| values.get(at)).map_or(String::new(), |v| (*v).to_owned()))
            .collect(),
        None => {
            let mut values: Vec<String> = values.into_iter().map(str::to_owned).collect();
            // I3.3 repaired the count against the package's note type; a
            // route to an equal layout keeps it.
            if values.len() != record.fields.len() && !record.fields.is_empty() {
                let _ = meguri_engine::notes::fields::repair(&mut values, record.fields.len());
            }
            values
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    reader: &impl ReadRows,
    raw: &Arc<RawPackage>,
    taken: &Arc<Taken>,
    options: &ImportOptions,
    target_today: u32,
    package_today: u32,
    now_s: i64,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> ImportOutcome<ImportPlan> {
    let targets: BTreeMap<i64, Notetype> = meguri_engine::notetypes::read::all(reader)?
        .into_iter()
        .map(|notetype| (notetype.id.0, notetype))
        .collect();
    let matches = matching_notes(reader, raw, cancel)?;
    let stage = if options.merge_notetypes {
        super::merge::plan(reader, raw, &taken.notetypes, &targets, &matches, options, now_s, cancel, progress)?
    } else {
        plan_without_merge(&taken.notetypes, &targets, options, reader, cancel, progress)?
    };
    let mut notes = Vec::with_capacity(raw.notes.len());
    // GUID to the plan index of the first counted note that holds it.
    let mut counted: HashMap<&[u8], usize> = HashMap::new();
    for (index, note) in raw.notes.iter().enumerate() {
        if index % 1000 == 0 {
            cancel.check()?;
            progress.report(index as u64, Some(raw.notes.len() as u64));
        }
        let package_type = note.notetype_id.0;
        let Some(route) = stage.routes.get(&package_type) else {
            notes.push(NotePlan {
                decision: Decision::MissingNotetype,
                package_type,
                fields: note.fields.split('\u{1f}').map(str::to_owned).collect(),
            });
            continue;
        };
        let fields = routed_fields(&note.fields, route.field_map.as_ref(), &route.record);
        let guid = note.guid.as_bytes();
        let decision = if let Some(&earlier) = counted.get(guid) {
            let other = stage.routes.get(&notes[earlier].package_type).map(|route| route.target);
            Decision::DuplicateOf {
                earlier,
                same_type: other == Some(route.target),
            }
        } else if let Some(target) = matches.get(guid) {
            let mid = stage.mid_after(target.id, target.notetype.0);
            if route.target != TypeTarget::Id(mid) {
                // With a merge the stages bring both to one note type, so
                // this is the merge-off case (I9 step 5).
                Decision::Conflicting { existing: target.id }
            } else {
                let stored = if stage.stamped.contains(&target.notetype.0) { now_s } else { target.mtime };
                counted.insert(guid, index);
                if update_holds(options.update_notes, stored, note.mtime.0) {
                    Decision::Updated { target: target.id }
                } else {
                    Decision::Duplicate { target: target.id }
                }
            }
        } else {
            counted.insert(guid, index);
            Decision::New
        };
        notes.push(NotePlan {
            decision,
            package_type,
            fields,
        });
    }
    Ok(ImportPlan {
        options: *options,
        target_today,
        package_today,
        stage,
        notes,
        raw: Arc::clone(raw),
        taken: Arc::clone(taken),
        media: MediaOutcome::default(),
    })
}
