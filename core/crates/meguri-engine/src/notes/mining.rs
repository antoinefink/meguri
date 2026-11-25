//! Mining's generation-bound, idempotent note and attachment transaction.
use super::{AddScreenNote, Added};
use crate::{Collection, EngineError, OpResult, OpSource};
use meguri_model::ids::NoteId;
use meguri_storage::{ConfigValue, ReadRows, media::StagedOp};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, mpsc};

const RECEIPTS: &str = "meguriMiningReceipt";
#[derive(Serialize, Deserialize)]
struct Receipt {
    key: String,
    digest: String,
    at: i64,
    id: i64,
}

#[derive(Debug)]
pub struct MinedNote {
    pub note: Added,
    pub written: bool,
}

/// The bounded receipt set is written in the same collection transaction as
/// the note. Only hashes and identities are retained; a crash or a lost HTTP
/// response cannot cause a second add. Only a live note can satisfy a receipt.
pub fn add(
    collection: &Collection,
    input: AddScreenNote,
    key: String,
    digest: String,
    stage: Option<StagedOp>,
) -> Result<OpResult<MinedNote>, EngineError> {
    if input.expected_generation != collection.generation() {
        return Err(EngineError::StaleGeneration {
            planned: input.expected_generation,
            current: collection.generation(),
        });
    }
    if key.len() != 64
        || digest.len() != 64
        || !key.bytes().chain(digest.bytes()).all(|b| b.is_ascii_hexdigit())
    {
        return Err(super::invalid("invalid mining receipt hash"));
    }
    let (replay, replayed) = mpsc::sync_channel(1);
    let spec = super::op_spec(crate::undo::names::ADD_NOTE, OpSource::LocalApi, 1);
    let result = collection.run(spec, move |ctx| {
        let now = ctx.scope().now().whole_secs().0;
        let receipt_key = format!("{RECEIPTS}{key}");
        if let Some(value) = ctx.scope().config_value(&receipt_key)? {
            let receipt: Receipt = serde_json::from_str(&value.compact())
                .map_err(|_| super::invalid("mining receipt cannot be read"))?;
            if now.saturating_sub(receipt.at) <= 600 && ctx.scope().note(NoteId(receipt.id))?.is_some() {
                if receipt.digest != digest {
                    return Err(super::invalid("Conflicting retry: the same mining delivery has different content."));
                }
                // Keep receipt and live-note checks on the serialized writer.
                // Cancel this read-only attempt so it rolls back before any
                // sequence, undo, sync or media commit effects. Deliver success
                // only after run() confirms that rollback completed.
                replay
                    .send((NoteId(receipt.id), ctx.last_seq, ctx.generation()))
                    .map_err(|_| EngineError::internal("mining replay receiver closed"))?;
                return Err(EngineError::Cancelled);
            }
        }
        // Independent keyed receipts merge through ordinary sync without a
        // whole-array last-writer update discarding another delivery's guard.
        let keys = ctx.scope().config_keys()?;
        let mut expired = Vec::new();
        let mut active = 0usize;
        for name in keys.into_iter().filter(|name| name.starts_with(RECEIPTS)) {
            if let Some(value) = ctx.scope().config_value(&name)? {
                let receipt: Receipt = serde_json::from_str(&value.compact())
                    .map_err(|_| super::invalid("mining receipt cannot be read"))?;
                if now.saturating_sub(receipt.at) > 600 || ctx.scope().note(NoteId(receipt.id))?.is_none() {
                    expired.push(name);
                } else {
                    active += 1;
                }
            }
        }
        // Refuse pressure instead of evicting still-live duplicate protection.
        if active >= 1024 {
            return Err(super::invalid("Mining retry history is full. Try again in ten minutes."));
        }
        let (scope, random) = ctx.scope_and_random();
        let note = super::add::add_screen_in(scope, random, input, false)?;
        if note.cards.is_empty() {
            return Err(super::invalid("The selected note type did not generate a card."));
        }
        let receipt = Receipt {
            key,
            digest,
            at: now,
            id: note.id.0,
        };
        let encoded = serde_json::to_vec(&receipt)
            .map_err(|_| super::invalid("could not encode mining receipt"))?;
        let value = ConfigValue::parse(&encoded).ok_or_else(|| super::invalid("invalid mining receipt"))?;
        for name in expired {
            ctx.scope().remove_config(&name)?;
        }
        ctx.scope().set_config(&receipt_key, &value)?;
        if let Some(stage) = stage {
            ctx.commit_media(stage, &[note.id])?;
        }
        Ok(MinedNote { note, written: true })
    });
    match result {
        Err(EngineError::Cancelled) => {
            let (id, seq, generation) = replayed.try_recv().map_err(|_| EngineError::Cancelled)?;
            Ok(OpResult {
                value: MinedNote {
                    note: Added { id, cards: vec![] },
                    written: false,
                },
                seq,
                generation,
                changes: Arc::new(meguri_model::changes::ChangeSet::empty(OpSource::LocalApi)),
                col_mod: None,
                undo: None,
                cloud_detached_run: None,
                past_cancel_point: false,
                timing: Default::default(),
            })
        }
        other => other,
    }
}
