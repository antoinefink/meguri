//! Card flags and flag names (`specs/tags/flags-and-marked.md` F1 to F4):
//! the flag is the low three bits of `cards.flags`, every other bit belongs
//! to other clients; flag names live in the synced config key `flagLabels`.

use std::fmt;

use meguri_model::ids::CardId;
use meguri_storage::{ConfigValue, ReadRows, StampMode, StorageError, WriteScope};

use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpResult, OpSpec};
use crate::tags::{RunOptions, op_spec};
use crate::undo::entry::StepKind;
use crate::undo::names::{RENAME_FLAG, SET_FLAG};

/// The bits of `cards.flags` that hold the flag (F1).
pub const FLAG_BITS: u8 = 0b111;

/// The highest flag (F1).
pub const MAX_FLAG: u8 = 7;

/// The config key of the flag names (F4).
pub const LABELS_KEY: &str = "flagLabels";

/// The default names of flags 1 to 7 (F1): Anki's English names. Other
/// languages use Meguri's own translations, in the app.
pub const DEFAULT_NAMES: [&str; 7] = ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"];

/// The flag of a `cards.flags` value: its low three bits (F1).
pub fn flag_of(flags: u8) -> u8 {
    flags & FLAG_BITS
}

/// `flags` with its low three bits replaced by `flag`, the others kept (F2).
pub fn with_flag(flags: u8, flag: u8) -> u8 {
    (flags & !FLAG_BITS) | (flag & FLAG_BITS)
}

/// F2 step 1: a flag above 7 is refused, refusal kind `invalid_flag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFlag;

impl InvalidFlag {
    /// The refusal kind (`specs/conformance/tag-ops.md` G8).
    pub const KIND: &'static str = "invalid_flag";
}

impl fmt::Display for InvalidFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid_flag: a flag is 0 to 7")
    }
}

/// F2 step 1.
pub fn check_flag(value: u64) -> Result<u8, InvalidFlag> {
    u8::try_from(value)
        .ok()
        .filter(|flag| *flag <= MAX_FLAG)
        .ok_or(InvalidFlag)
}

fn invalid(error: impl fmt::Display) -> EngineError {
    EngineError::InvalidArgument {
        message: error.to_string(),
    }
}

/// F2 steps 2 and 3 within `scope`: each listed card whose flag differs gets
/// `flag`, its other bits kept, and is written; cards that no longer exist
/// are skipped. The scope must stamp in `KeepMtime` mode: a flag change
/// gives the card `usn` -1 and keeps its `mod` (F2). Returns how many cards
/// were written.
pub fn set_flag_in(scope: &mut WriteScope<'_>, cards: &[CardId], flag: u8) -> Result<usize, StorageError> {
    if scope.mode() != StampMode::KeepMtime {
        return Err(StorageError::Mode {
            detail: format!(
                "a flag change keeps the card's mod; a {:?} scope stamps it",
                scope.mode()
            ),
        });
    }
    let mut written = 0;
    for id in cards {
        let Some(mut card) = scope.card(*id)? else {
            continue;
        };
        if flag_of(card.flags) == flag {
            continue;
        }
        card.flags = with_flag(card.flags, flag);
        if scope.put(&card)?.changed() {
            written += 1;
        }
    }
    Ok(written)
}

/// The spec of a flag write: "Set Flag", `KeepMtime` stamping.
fn set_flag_spec(cards: usize, options: &RunOptions) -> OpSpec {
    op_spec("set_flag", SET_FLAG, cards, options).stamp(StampMode::KeepMtime)
}

/// F2: writes `flag` to the cards, as one "Set Flag" step; a flag above 7 is
/// refused (`InvalidArgument` carrying `invalid_flag`). The value is the
/// number of cards written.
pub fn set_flag(
    collection: &Collection,
    cards: &[CardId],
    flag: u64,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let flag = check_flag(flag).map_err(invalid)?;
    let cards = cards.to_vec();
    collection.run(set_flag_spec(cards.len(), options), move |ctx| {
        Ok(set_flag_in(ctx.scope(), &cards, flag)?)
    })
}

/// F3: the review menu's and Browse's toggle. When the current card's flag
/// is `chosen`, every target card loses its flag; otherwise each gets
/// `chosen`. The current card is read in the write phase. Browse's "No
/// flag" is [`set_flag`] with 0.
pub fn toggle_flag(
    collection: &Collection,
    current: CardId,
    targets: &[CardId],
    chosen: u8,
    options: &RunOptions,
) -> Result<OpResult<usize>, EngineError> {
    let chosen = check_flag(u64::from(chosen)).map_err(invalid)?;
    let targets = targets.to_vec();
    collection.run(set_flag_spec(targets.len(), options), move |ctx| {
        let has = ctx.scope().card(current)?.map(|card| flag_of(card.flags));
        let flag = if has == Some(chosen) { 0 } else { chosen };
        Ok(set_flag_in(ctx.scope(), &targets, flag)?)
    })
}

/// F4 Reading: the shown name of flag `flag` (1 to 7) under the stored
/// `flagLabels` value: its member `"N"` when that is a non-empty string,
/// else the default name. A missing key or a value that is not an object
/// reads as `{}`.
pub fn shown_name(labels: Option<&ConfigValue>, flag: u8) -> String {
    let custom = labels
        .and_then(|labels| labels.get(&flag.to_string()))
        .and_then(ConfigValue::as_str)
        .filter(|name| !name.is_empty());
    match custom {
        Some(name) => name.to_owned(),
        None => DEFAULT_NAMES
            .get(usize::from(flag).wrapping_sub(1))
            .map_or_else(String::new, |name| (*name).to_owned()),
    }
}

/// The shown names of flags 1 to 7 (F4, R900).
pub fn flag_names(reader: &(impl ReadRows + ?Sized)) -> Result<[String; 7], StorageError> {
    let labels = reader.config_value(LABELS_KEY)?;
    Ok(std::array::from_fn(|i| {
        shown_name(labels.as_ref(), u8::try_from(i + 1).unwrap_or(MAX_FLAG))
    }))
}

fn check_named_flag(flag: u8) -> Result<(), EngineError> {
    if (1..=MAX_FLAG).contains(&flag) {
        Ok(())
    } else {
        Err(invalid(format!("flag {flag} has no name: names are for flags 1 to 7")))
    }
}

/// F4 Restoring the default name within `scope`: removes member `"N"` when
/// it exists, keeping the others (the key stays, `{}` when none is left).
/// Returns whether the key was written.
pub fn restore_flag_name_in(scope: &mut WriteScope<'_>, flag: u8) -> Result<bool, StorageError> {
    let Some(ConfigValue::Object(members)) = scope.config_value(LABELS_KEY)? else {
        return Ok(false);
    };
    let name = flag.to_string();
    if !members.iter().any(|(member, _)| *member == name) {
        return Ok(false);
    }
    let kept = members.into_iter().filter(|(member, _)| *member != name).collect();
    Ok(scope.set_config(LABELS_KEY, &ConfigValue::Object(kept))?.changed())
}

/// F4 Renaming within `scope`: every `"` removed; an empty result restores
/// the default name; a result equal to the shown name writes nothing;
/// otherwise member `"N"` is set, every other member kept (a stored value
/// that is not an object is replaced). Returns whether the key was written.
pub fn rename_flag_in(scope: &mut WriteScope<'_>, flag: u8, text: &str) -> Result<bool, StorageError> {
    let name: String = text.chars().filter(|&c| c != '"').collect();
    if name.is_empty() {
        return restore_flag_name_in(scope, flag);
    }
    let stored = scope.config_value(LABELS_KEY)?;
    if shown_name(stored.as_ref(), flag) == name {
        return Ok(false);
    }
    let base = stored.unwrap_or(ConfigValue::Object(Vec::new()));
    let value = base.with_member(&flag.to_string(), ConfigValue::String(name));
    Ok(scope.set_config(LABELS_KEY, &value)?.changed())
}

fn rename_flag_spec() -> OpSpec {
    OpSpec::named("rename_flag", meguri_model::changes::OpSource::UserUndoable).undo_step(RENAME_FLAG, StepKind::Other)
}

/// F4: renames flag `flag` (1 to 7) from Browse's sidebar. A write is one
/// "Rename Flag" step. The value is whether the key was written.
pub fn rename_flag(collection: &Collection, flag: u8, text: &str) -> Result<OpResult<bool>, EngineError> {
    check_named_flag(flag)?;
    let text = text.to_owned();
    collection.run(rename_flag_spec(), move |ctx| {
        Ok(rename_flag_in(ctx.scope(), flag, &text)?)
    })
}

/// F4: the sidebar's Restore Default Name. A write is one "Rename Flag"
/// step. The value is whether the key was written.
pub fn restore_flag_name(collection: &Collection, flag: u8) -> Result<OpResult<bool>, EngineError> {
    check_named_flag(flag)?;
    collection.run(rename_flag_spec(), move |ctx| {
        Ok(restore_flag_name_in(ctx.scope(), flag)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_the_low_three_bits() {
        assert_eq!(with_flag(0b1000_0011, 5), 0b1000_0101);
        assert_eq!(with_flag(0b1000_0101, 0), 0b1000_0000);
        assert_eq!(flag_of(0b1111_1010), 2);
        assert_eq!(check_flag(7), Ok(7));
        assert_eq!(check_flag(8), Err(InvalidFlag));
        assert_eq!(check_flag(u64::MAX), Err(InvalidFlag));
    }

    #[test]
    fn shown_names_read_non_empty_string_members() {
        let labels = ConfigValue::parse(br#"{"1":"Hard","2":"","3":5,"9":"x"}"#).unwrap();
        assert_eq!(shown_name(Some(&labels), 1), "Hard");
        assert_eq!(shown_name(Some(&labels), 2), "Orange");
        assert_eq!(shown_name(Some(&labels), 3), "Green");
        assert_eq!(shown_name(Some(&ConfigValue::Integer(3)), 7), "Purple");
        assert_eq!(shown_name(None, 6), "Turquoise");
    }
}
