//! Media of an import (`package/apkg-import` I12): which entries the new
//! and updated notes use, their stored names, the staging of their payloads
//! and the rewrite of the notes' references.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;

use meguri_engine::media::MediaService;
use meguri_engine::{CancelToken, ProgressPhase, ProgressSink};
use meguri_storage::media::{MediaError, StagedOp};
use meguri_text::entities::decode_text;
use meguri_text::media_name::safe_media_name;
use meguri_text::media_refs::{media_tags, sound_tags};

use super::archive::{DeckArchive, MediaEntry};
use super::plan::{Decision, ImportPlan};
use super::session::ImportSession;
use super::{ApkgImportError, ImportOutcome, corrupt};

const MARGIN: u64 = 4 * 1024 * 1024;
/// The largest static entry the post-commit exact-name write takes: that
/// write holds the file in memory (`MediaStore::write_static_if_absent`),
/// so a larger one is skipped rather than read whole (R1852). Checked
/// entries have no such limit: they stream into staging.
const STATIC_LIMIT: u64 = 64 * 1024 * 1024;

/// What staging decided, carried by the plan into the write phase.
#[derive(Default)]
pub(crate) struct MediaOutcome {
    /// The staged checked entries, bound to the op at its commit.
    pub stage: Option<StagedOp>,
    /// The plan indices of the notes whose final fields name a staged file.
    pub witnesses: Vec<usize>,
    /// Static entries decided "written": indices into the session's map.
    pub statics: Vec<usize>,
    pub written: u64,
    pub reused: u64,
    pub skipped: u64,
    /// Decoded bytes charged to the package once staging read its
    /// payloads (P8): where the post-commit static reads start.
    pub decoded: u64,
}

/// I12.1: a static entry is never matched by a reference or renamed.
pub(crate) fn is_static(name: &str) -> bool {
    name.starts_with('_') || name.starts_with("latex-")
}

/// I12.2 step 1.
fn eligible(decoded: &str) -> bool {
    !decoded.is_empty() && !decoded.contains('/') && decoded != "." && decoded != ".."
}

/// I12.2 step 3's escaping of a replacement for a name that held entities.
fn escaped(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// One pass of I12.2 over matches given as (start, matched text, raw name).
fn rewrite_matches<'t>(
    text: &'t str,
    matches: impl Iterator<Item = (usize, &'t str, &'t str)>,
    replacement: &dyn Fn(&str, &str) -> Option<String>,
) -> Option<String> {
    let mut out: Option<String> = None;
    let mut done = 0;
    for (start, matched, raw) in matches {
        let decoded = decode_text(raw);
        if !eligible(&decoded) {
            continue;
        }
        let transformed = safe_media_name(&decoded);
        let Some(mut new) = replacement(&decoded, &transformed) else {
            continue;
        };
        if decoded != raw {
            new = escaped(&new);
        }
        let out = out.get_or_insert_with(String::new);
        out.push_str(&text[done..start]);
        out.push_str(&matched.replace(raw, &new));
        done = start + matched.len();
    }
    out.map(|mut out| {
        out.push_str(&text[done..]);
        out
    })
}

/// I12.2 on one field: media tags left to right, then the sound tags of the
/// result. `replacement(d, N)` gives the name to write, if any.
pub(crate) fn rewrite_field(field: &str, replacement: &dyn Fn(&str, &str) -> Option<String>) -> Option<String> {
    let first = rewrite_matches(field, media_tags(field).map(|tag| (tag.start, tag.matched, tag.raw_name)), replacement);
    let text = first.as_deref().unwrap_or(field);
    let second = rewrite_matches(text, sound_tags(text).map(|tag| (tag.start, tag.matched, tag.raw_name)), replacement);
    second.or(first)
}

/// The transformed names a field's eligible references ask for.
fn wanted(field: &str, mut visit: impl FnMut(String)) {
    let raws = media_tags(field)
        .map(|tag| tag.raw_name)
        .chain(sound_tags(field).map(|tag| tag.raw_name));
    for raw in raws {
        let decoded = decode_text(raw);
        if eligible(&decoded) {
            visit(safe_media_name(&decoded));
        }
    }
}

fn media_error(error: MediaError) -> ApkgImportError {
    ApkgImportError::Io {
        detail: error.to_string(),
    }
}

fn hex(bytes: &[u8; 20]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decodes one payload into `path` (created new), bounded by its declared
/// size, and returns its SHA-1.
fn decode(
    archive: &mut DeckArchive,
    entry: &MediaEntry,
    path: &Path,
    check: &(dyn Fn() -> bool + Sync),
) -> ImportOutcome<[u8; 20]> {
    let _ = std::fs::remove_file(path);
    let mut output = File::options().write(true).create_new(true).open(path)?;
    let outcome = archive.read_payload(entry, check, |part| output.write_all(part).map_err(Into::into));
    let flushed = output.flush();
    drop(output);
    match outcome {
        Ok((_, sha1)) => {
            flushed?;
            Ok(sha1)
        }
        Err(error) => {
            let _ = std::fs::remove_file(path);
            Err(error.into())
        }
    }
}

/// I14.1 steps 5 and 6: preflight, the stored name of every used and static
/// entry with its payload staged, then the reference rewrite of the new and
/// updated notes. A failure or a cancel leaves nothing staged.
pub(crate) fn stage(
    session: &ImportSession,
    service: &MediaService,
    plan: &mut ImportPlan,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> ImportOutcome<()> {
    let check = || cancel.is_cancelled();
    let check: &(dyn Fn() -> bool + Sync) = &check;
    let entries = &session.media;
    // The checked entry each transformed name selects: the first in map order.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if !is_static(&entry.name) {
            by_name.entry(safe_media_name(&entry.name)).or_insert(index);
        }
    }
    let mut used: BTreeSet<usize> = BTreeSet::new();
    for note in &plan.notes {
        if matches!(note.decision, Decision::New | Decision::Updated { .. }) {
            for field in &note.fields {
                wanted(field, |name| {
                    if let Some(index) = by_name.get(&name) {
                        used.insert(*index);
                    }
                });
            }
        }
    }
    let needed: Vec<usize> = (0..entries.len())
        .filter(|index| used.contains(index) || is_static(&entries[*index].name))
        .collect();
    plan.media = MediaOutcome::default();

    // I15's third row, on the media folder's file system.
    let bytes = needed
        .iter()
        .try_fold(0_u64, |sum, index| sum.checked_add(entries[*index].size))
        .and_then(|sum| sum.checked_add(session.collection_size().checked_mul(2)?))
        .and_then(|sum| sum.checked_add(MARGIN))
        .ok_or_else(|| corrupt("media preflight overflow"))?;
    meguri_storage::space::require_space(session.context.space.as_ref(), &session.context.collection_directory, bytes)
        .map_err(|error| match error {
            meguri_storage::StorageError::InsufficientSpace { required, available } => {
                ApkgImportError::InsufficientSpace {
                    needed: required,
                    available,
                }
            }
            other => other.into(),
        })?;

    if needed.is_empty() {
        return Ok(());
    }
    progress.set_phase(ProgressPhase::Named("media"));
    let mut archive = DeckArchive::open(&session.source_path(), check, session.decoded)?;
    let mut staged = service.begin_staging()?;
    let scratch = session.staging_path().join("payload");
    // By entry index: the stored name of a checked entry that has one.
    let mut stored: HashMap<usize, String> = HashMap::new();
    // Case keys of the static names this import already decided to write.
    let mut reserved: HashSet<String> = HashSet::new();
    let mut outcome = MediaOutcome::default();
    let total = needed.len() as u64;
    for (done, index) in needed.iter().enumerate() {
        cancel.check()?;
        let entry = &entries[*index];
        let sha1 = decode(&mut archive, entry, &scratch, check)?;
        if is_static(&entry.name) {
            // Decoded and checked like any payload; written only after the
            // commit, through the immediate path (I12.4).
            let _ = std::fs::remove_file(&scratch);
            let free = !reserved.contains(&entry.name.to_lowercase())
                && !service.store().case_variant_present(&entry.name).map_err(media_error)?;
            if free && entry.size <= STATIC_LIMIT && safe_media_name(&entry.name) == entry.name {
                reserved.insert(entry.name.to_lowercase());
                outcome.statics.push(*index);
            } else {
                outcome.skipped += 1;
            }
        } else {
            let desired = safe_media_name(&entry.name);
            match staged.add_file(&desired, &scratch, &hex(&sha1)) {
                Ok((name, wrote)) => {
                    if wrote {
                        outcome.written += 1;
                    } else {
                        outcome.reused += 1;
                    }
                    stored.insert(*index, name);
                }
                // M3's refusals skip the entry: it has no stored name.
                Err(MediaError::InvalidName | MediaError::IgnoredName) => outcome.skipped += 1,
                Err(other) => {
                    let _ = std::fs::remove_file(&scratch);
                    return Err(media_error(other));
                }
            }
            let _ = std::fs::remove_file(&scratch);
        }
        progress.report(done as u64 + 1, Some(total));
    }
    outcome.decoded = archive.charged();
    drop(archive);

    // I12.2's rewrite, now that the stored names are known.
    let replacement = |decoded: &str, transformed: &str| -> Option<String> {
        let name = match by_name.get(transformed).and_then(|index| stored.get(index)) {
            Some(stored) => stored.as_str(),
            None => transformed,
        };
        (name != decoded).then(|| name.to_owned())
    };
    let names: HashSet<String> = staged.names().map_err(media_error)?.into_iter().collect();
    for (index, note) in plan.notes.iter_mut().enumerate() {
        if !matches!(note.decision, Decision::New | Decision::Updated { .. }) {
            continue;
        }
        let mut witness = false;
        for field in &mut note.fields {
            if let Some(rewritten) = rewrite_field(field, &replacement) {
                *field = rewritten;
            }
            if !names.is_empty() && !witness {
                witness = meguri_text::media_refs::local_references(field)
                    .iter()
                    .any(|name| names.contains(name));
            }
        }
        if witness {
            outcome.witnesses.push(index);
        }
    }
    if names.is_empty() {
        staged.rollback().map_err(media_error)?;
    } else {
        outcome.stage = Some(staged);
    }
    plan.media = outcome;
    Ok(())
}

/// I12.4: after the commit, each static entry decided "written" is decoded
/// again and written by its exact name when the folder still has no case
/// variant of it. Returns how many were written and how many skipped.
pub(crate) fn write_statics(
    session: &ImportSession,
    service: &MediaService,
    statics: &[usize],
    charged: u64,
) -> (u64, u64) {
    if statics.is_empty() {
        return (0, 0);
    }
    let check: &(dyn Fn() -> bool + Sync) = &|| false;
    let Ok(mut archive) = DeckArchive::open(&session.source_path(), check, charged.max(session.decoded)) else {
        return (0, statics.len() as u64);
    };
    let mut written = Vec::new();
    for index in statics {
        let entry = &session.media[*index];
        let mut bytes = Vec::new();
        let decoded = archive.read_payload(entry, check, |part| {
            bytes.extend_from_slice(part);
            Ok(())
        });
        if decoded.is_ok()
            && let Ok(Some(name)) = service.store().write_static_if_absent(&entry.name, &bytes)
        {
            written.push(name);
        }
    }
    let count = written.len() as u64;
    let _ = service.media_arrived(written);
    (count, statics.len() as u64 - count)
}
