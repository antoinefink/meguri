//! H10: one generation-bound, cancellable catalog op. Decisions are re-derived
//! from live rows; safe marking uses sequence proofs and captured R.
use super::{
    Result, engine_error,
    stage::{self, Meta},
};
use meguri_engine::{
    CancelToken, Collection, EngineError, OpSource, OpSpec, ProgressSink, SyncHold, SyncMerge, executor::catalog,
};
use meguri_model::{
    deck::Deck,
    deck_config::DeckConfig,
    grave::{Grave, GraveKind},
    ids::*,
    note::Note,
    tag::Tag,
    time::EpochMillis,
    usn::Usn,
};
use meguri_storage::{
    ReadRows, Record, RowKey, StampMode, Table,
    normal_stage::{Namespace, NormalStage},
};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Default)]
pub struct MergeOutcome {
    pub one_way: bool,
    pub skipped: Vec<(Table, i64)>,
    pub timing: meguri_engine::OpTiming,
    pub write_budget: Duration,
    pub seq: Option<meguri_engine::Seq>,
}
pub(super) fn apply(
    collection: &Collection,
    stage: &NormalStage,
    hold: SyncHold,
    schema: EpochMillis,
    usn: Usn,
    time: EpochMillis,
    meta: Meta,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<MergeOutcome> {
    let path = stage.path().to_path_buf();
    if cancel.is_cancelled() {
        return Err(engine_error(EngineError::Cancelled));
    }
    let rows = stage.incoming_count().map_err(super::storage_error)?;
    let validations = stage.note_validation_count().map_err(super::storage_error)?;
    // Full payload work is per distinct key; scalar live validation is per
    // occurrence. Budget both conservatively without changing sanity counts
    // or the distinct target set used for reconciliation and diagnostics.
    let budget = Duration::from_micros(rows.saturating_add(validations).saturating_mul(25).max(1));
    let mut spec = OpSpec::new(catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming)
        .stamp(StampMode::CallerSupplied)
        .cancel(cancel)
        .progress(progress);
    if budget > Duration::from_millis(200) {
        spec = spec.declare_long_write(budget);
    }
    let result = collection
        .run(spec, move |ctx| {
            ctx.check_cancel()?;
            if ctx.scope().col()?.schema_modified != schema {
                return Ok(MergeOutcome {
                    one_way: true,
                    ..Default::default()
                });
            }
            let post_mod = ctx.local_modification_since(hold.snapshot)
                || !ctx.untouched_since(Table::Col, &RowKey::Id(1), hold.snapshot);
            ctx.scope().attach_normal_stage(&path)?;
            let mut outcome = MergeOutcome::default();
            let mut targets = Vec::new();
            // All received targets, including rejected objects and graves. Do not
            // derive this set from the merge journal: that would lose rejected rows.
            for table in [
                Table::Notes,
                Table::Cards,
                Table::Decks,
                Table::Notetypes,
                Table::DeckConfig,
                Table::Tags,
                Table::Config,
                Table::Revlog,
            ] {
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx.scope().staged_page(Namespace::Incoming, table, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(table, r));
                    targets.extend(page.iter().map(|r| (table, NormalStage::row_key(table, r))));
                }
            }
            for kind in [GraveKind::Note, GraveKind::Card, GraveKind::Deck] {
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx
                        .scope()
                        .staged_page(Namespace::Incoming, Table::Graves, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(Table::Graves, r));
                    for image in page {
                        let grave = Grave::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                        if grave.kind != kind {
                            continue;
                        }
                        let table = grave_table(kind);
                        targets.push((table, RowKey::Id(grave.oid)));
                        ctx.scope().restore(table, &RowKey::Id(grave.oid), None)?;
                        ctx.scope().add_grave(grave.oid, kind, usn)?;
                    }
                }
            }
            for table in [Table::Notetypes, Table::Decks, Table::DeckConfig] {
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx.scope().staged_page(Namespace::Incoming, table, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(table, r));
                    for image in page {
                        let RowKey::Id(id) = NormalStage::row_key(table, &image) else {
                            return Err(EngineError::Internal {
                                message: "object stage key".into(),
                            });
                        };
                        if table == Table::Decks && unsent_grave(ctx.scope(), id, GraveKind::Deck)? {
                            outcome.skipped.push((table, id));
                            continue;
                        }
                        match table {
                            Table::Notetypes => {
                                let mut nt = ctx.scope().staged_notetype(NotetypeId(id))?.ok_or_else(|| {
                                    EngineError::Internal {
                                        message: "staged note type missing".into(),
                                    }
                                })?;
                                if ctx.scope().notetype(nt.id)?.is_some_and(|local| local.mtime > nt.mtime) {
                                    continue;
                                }
                                stage::unique(ctx.scope(), table, id, &mut nt.name, &mut nt.mtime, &mut nt.usn, usn)?;
                                ctx.scope().put_notetype(&nt)?;
                            }
                            Table::Decks => {
                                let mut row = Deck::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                                if ctx.scope().deck(row.id)?.is_some_and(|local| local.mtime > row.mtime) {
                                    continue;
                                }
                                stage::unique(
                                    ctx.scope(),
                                    table,
                                    id,
                                    &mut row.name,
                                    &mut row.mtime,
                                    &mut row.usn,
                                    usn,
                                )?;
                                ctx.scope().put(&row)?;
                            }
                            Table::DeckConfig => {
                                let row = DeckConfig::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                                if ctx
                                    .scope()
                                    .deck_config(row.id)?
                                    .is_some_and(|local| local.mtime > row.mtime)
                                {
                                    continue;
                                }
                                ctx.scope().put(&row)?;
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            if meta.config {
                // Include removals and newly introduced keys. A post-S touch protects
                // the key even if the live value became equal again.
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx.scope().row_page(Table::Config, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(Table::Config, r));
                    for image in page {
                        let key = NormalStage::row_key(Table::Config, &image);
                        if ctx.untouched_since(Table::Config, &key, hold.snapshot)
                            && ctx
                                .scope()
                                .staged_image(Namespace::Incoming, Table::Config, &key)?
                                .is_none()
                        {
                            targets.push((Table::Config, key.clone()));
                            ctx.scope().restore(Table::Config, &key, None)?;
                        }
                    }
                }
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx
                        .scope()
                        .staged_page(Namespace::Incoming, Table::Config, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(Table::Config, r));
                    for mut image in page {
                        let key = NormalStage::row_key(Table::Config, &image);
                        if ctx.untouched_since(Table::Config, &key, hold.snapshot) {
                            image[1] = meguri_storage::Cell::Integer(usn.0.into());
                            image[2] = meguri_storage::Cell::Integer(ctx.now().0.div_euclid(1000));
                            ctx.scope().restore(Table::Config, &key, Some(&image))?;
                        }
                    }
                }
            }
            if let Some(created) = meta.created {
                let mut col = ctx.scope().col()?;
                col.created = created;
                ctx.scope().put_col(&col)?;
            }
            let mut after = None;
            loop {
                ctx.check_cancel()?;
                let page = ctx
                    .scope()
                    .staged_page(Namespace::Incoming, Table::Tags, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                after = page.last().map(|r| NormalStage::row_key(Table::Tags, r));
                for image in page {
                    let row = Tag::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                    stage::register_tag(ctx.scope(), &row.name, usn)?;
                }
            }
            // Replay against the actual live base after graves and object
            // merges, before choosing any note payload. A post-S touch alone
            // does not change eligibility. The executor rolls back this fault.
            validate_live_notes(ctx)?;
            for table in [Table::Cards, Table::Notes, Table::Revlog] {
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx.scope().staged_page(Namespace::Incoming, table, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(table, r));
                    let mut accepted = Vec::new();
                    for image in page {
                        let RowKey::Id(id) = NormalStage::row_key(table, &image) else {
                            return Err(EngineError::Internal {
                                message: "chunk stage key".into(),
                            });
                        };
                        let kind = match table {
                            Table::Cards => Some(GraveKind::Card),
                            Table::Notes => Some(GraveKind::Note),
                            _ => None,
                        };
                        if let Some(kind) = kind
                            && unsent_grave(ctx.scope(), id, kind)?
                        {
                            outcome.skipped.push((table, id));
                            continue;
                        }
                        let keep = match table {
                            Table::Cards | Table::Notes => {
                                let maximum = ctx.scope().staged_received_max(table, id)?;
                                ctx.scope()
                                    .sync_row_state(table, id)?
                                    .is_none_or(|(usn, mtime)| stage::accepts(usn, mtime, maximum))
                            }
                            Table::Revlog => ctx.scope().revlog_entry(RevlogId(id))?.is_none(),
                            _ => unreachable!(),
                        };
                        if keep {
                            if table == Table::Notes {
                                let mut note = Note::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                                let nt = ctx.scope().notetype(note.notetype_id)?;
                                stage::validate_note(&mut note, nt).map_err(EngineError::SyncNote)?;
                                accepted.push(note.to_cells());
                            } else {
                                accepted.push(image);
                            }
                        }
                    }
                    ctx.scope().merge_sync_rows(table, &accepted)?;
                    #[cfg(feature = "test-hooks")]
                    super::test_hooks::reach(super::test_hooks::Point::Merged {
                        table,
                        rows: accepted.len(),
                    });
                    ctx.check_cancel()?;
                }
            }
            // Protect R even if history was cleared or the held step was undone.
            for table in Table::ALL.into_iter().filter(|t| t.usn_column().is_some()) {
                let mut after = None;
                loop {
                    ctx.check_cancel()?;
                    let page = ctx.scope().staged_page(Namespace::Sent, table, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|r| NormalStage::row_key(table, r));
                    let mut keys = Vec::new();
                    for sent in page {
                        let key = NormalStage::row_key(table, &sent);
                        if hold.run.touches(table, &key) || !ctx.untouched_since(table, &key, hold.snapshot) {
                            continue;
                        }
                        if let Some(live) = ctx.scope().row_image(table, &key)? {
                            let column = table.usn_column().unwrap();
                            if (table == Table::Config && live[column].as_i64() != Some(0))
                                || live[column].as_i64() == Some(-1)
                            {
                                keys.push(key);
                            }
                        }
                    }
                    ctx.scope()
                        .mark_usn(table, &keys, if table == Table::Config { Usn(0) } else { usn })?;
                }
            }
            let pending = ctx.scope().sync_has_pending()? || post_mod;
            let mut col = ctx.scope().col()?;
            col.last_sync = time;
            col.modified =
                EpochMillis(
                    time.0
                        .checked_add(i64::from(pending))
                        .ok_or_else(|| EngineError::Internal {
                            message: "finish time overflow".into(),
                        })?,
                );
            col.usn = Usn(usn.0.checked_add(1).ok_or_else(|| EngineError::Internal {
                message: "server usn overflow".into(),
            })?);
            ctx.scope().put_col(&col)?;
            targets.sort_unstable();
            targets.dedup();
            ctx.report_sync_merge(SyncMerge {
                snapshot: hold.snapshot,
                staged: targets,
            })?;
            ctx.report_skipped_incoming(
                outcome
                    .skipped
                    .iter()
                    .filter_map(|&(table, id)| (table == Table::Cards).then_some(CardId(id))),
                outcome
                    .skipped
                    .iter()
                    .filter_map(|&(table, id)| (table == Table::Notes).then_some(NoteId(id))),
                outcome
                    .skipped
                    .iter()
                    .filter_map(|&(table, id)| (table == Table::Decks).then_some(DeckId(id))),
            );
            Ok(outcome)
        })
        .map_err(engine_error)?;
    let mut outcome = result.value;
    outcome.timing = result.timing;
    outcome.write_budget = budget;
    outcome.seq = Some(result.seq);
    Ok(outcome)
}
/// H6's scalar replay has page-bounded RAM even for one very repeated key.
/// Keys are sorted on disk, but faults are selected by global receive order.
fn validate_live_notes(ctx: &mut meguri_engine::OpContext<'_, '_>) -> std::result::Result<(), EngineError> {
    let mut first: Option<(i64, meguri_model::note::SyncNoteFault)> = None;
    let mut after = None;
    loop {
        ctx.check_cancel()?;
        let page = ctx.scope().staged_note_validation_keys(after)?;
        if page.is_empty() {
            break;
        }
        after = page.last().copied();
        for NoteId(id) in page {
            if unsent_grave(ctx.scope(), id, GraveKind::Note)? {
                continue;
            }
            let mut state = ctx.scope().sync_row_state(Table::Notes, id)?;
            let mut ordinal = 0;
            'records: loop {
                ctx.check_cancel()?;
                let records = ctx.scope().staged_note_validation_page(NoteId(id), ordinal)?;
                if records.is_empty() {
                    break;
                }
                for record in records {
                    ordinal = record.ordinal;
                    if state.is_none_or(|(usn, mtime)| stage::accepts(usn, mtime, record.mtime)) {
                        if let Some(fault) = record.fault {
                            if first.is_none_or(|(earlier, _)| record.ordinal < earlier) {
                                first = Some((record.ordinal, fault));
                            }
                            break 'records;
                        }
                        state = Some((record.usn, record.mtime));
                    }
                }
            }
        }
    }
    match first {
        Some((_, fault)) => Err(EngineError::SyncNote(fault)),
        None => Ok(()),
    }
}
fn grave_table(kind: GraveKind) -> Table {
    match kind {
        GraveKind::Card => Table::Cards,
        GraveKind::Note => Table::Notes,
        GraveKind::Deck => Table::Decks,
    }
}
fn unsent_grave(
    scope: &meguri_storage::WriteScope<'_>,
    id: i64,
    kind: GraveKind,
) -> std::result::Result<bool, EngineError> {
    let key = RowKey::Pair(id, kind.stored());
    let pending = scope
        .row_image(Table::Graves, &key)?
        .is_some_and(|r| r[2].as_i64() == Some(-1));
    Ok(pending && scope.staged_image(Namespace::Sent, Table::Graves, &key)?.is_none())
}
