//! The schema-11 JSON form of note types, decks and presets
//! (`specs/model/schema11-json.md`), converted from and to the schema-18
//! records. Output (record to object) never fails; input (object to record)
//! refuses a whole object with [`ModelError::InvalidJson`] (S1).
//!
//! Users: the schema upgrade (U22), the schema-11 writer (U33), normal sync
//! (U107) and legacy packages (U120, U123).

pub mod col;
pub mod deck;
pub mod deck_config;
pub mod notetype;

pub use deck::{deck_from_json, deck_to_json};
pub use deck_config::{deck_config_from_json, deck_config_to_json};
pub use notetype::{notetype_from_json, notetype_to_json};

use serde_json::value::RawValue;

use crate::error::{ModelError, Schema11Item};
use crate::json::{self, Member, ObjectWriter};

type Result<T> = std::result::Result<T, ModelError>;

/// One object being read (S2): its members with repeated keys reduced (N9),
/// which of them a key of the object's tables has read, and where it is, for
/// errors.
pub(crate) struct ObjectReader<'a> {
    members: Vec<Member<'a>>,
    read: Vec<bool>,
    item: Schema11Item,
    path: String,
}

impl<'a> ObjectReader<'a> {
    /// `path` names the object inside its record (`flds[1]`), empty at the
    /// top. A value that is not an object refuses.
    pub(crate) fn new(raw: &'a RawValue, item: Schema11Item, path: &str) -> Result<ObjectReader<'a>> {
        let members = json::object(raw).ok_or_else(|| ModelError::invalid_json(item, path))?;
        Ok(ObjectReader {
            read: vec![false; members.len()],
            members,
            item,
            path: path.to_owned(),
        })
    }

    pub(crate) fn refuse(&self, key: &str) -> ModelError {
        let at = if self.path.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path)
        };
        ModelError::invalid_json(self.item, at)
    }

    /// The member `key`, which counts as read.
    pub(crate) fn get(&mut self, key: &str) -> Option<&'a RawValue> {
        let index = self.members.iter().position(|(k, _)| k == key)?;
        self.read[index] = true;
        Some(self.members[index].1)
    }

    /// A strict reader: a value that does not fit refuses; a missing key
    /// takes `missing`, or refuses when that is `None`.
    pub(crate) fn strict<T>(
        &mut self,
        key: &str,
        reader: impl Fn(&'a RawValue) -> Option<T>,
        missing: Option<T>,
    ) -> Result<T> {
        match self.get(key) {
            Some(raw) => reader(raw).ok_or_else(|| self.refuse(key)),
            None => missing.ok_or_else(|| self.refuse(key)),
        }
    }

    /// A lenient reader (N6 default on invalid): a value that does not fit
    /// gives the type's default; a missing key takes `missing`, or refuses
    /// when that is `None`.
    pub(crate) fn lenient<T: Default>(
        &mut self,
        key: &str,
        reader: impl Fn(&'a RawValue) -> Option<T>,
        missing: Option<T>,
    ) -> Result<T> {
        match self.get(key) {
            Some(raw) => Ok(reader(raw).unwrap_or_default()),
            None => missing.ok_or_else(|| self.refuse(key)),
        }
    }

    /// A key that is dropped without being kept in `other`.
    pub(crate) fn discard(&mut self, key: &str) {
        self.get(key);
    }

    /// The members no key read, in input order (S3 input rule 1).
    pub(crate) fn unread(self) -> Vec<Member<'a>> {
        self.members
            .into_iter()
            .zip(self.read)
            .filter_map(|(member, read)| (!read).then_some(member))
            .collect()
    }
}

/// A reader that also takes `null`, as `None`.
pub(crate) fn nullable<'a, T>(
    reader: impl Fn(&'a RawValue) -> Option<T>,
) -> impl Fn(&'a RawValue) -> Option<Option<T>> {
    move |raw| {
        if json::is_null(raw) {
            Some(None)
        } else {
            reader(raw).map(Some)
        }
    }
}

/// S3 input rule 2: a compact object of the unread members, or no bytes.
pub(crate) fn other_from_members(members: &[Member<'_>]) -> Vec<u8> {
    if members.is_empty() {
        return Vec::new();
    }
    let mut out = ObjectWriter::new();
    for (key, raw) in members {
        out.raw(key, raw.get());
    }
    out.finish().into_bytes()
}

/// S3 output rule 1: the members of `other` when it holds a JSON object, with
/// repeated keys reduced (N9); none otherwise.
pub(crate) fn other_members(other: &[u8]) -> Vec<Member<'_>> {
    std::str::from_utf8(other)
        .ok()
        .and_then(json::parse_value)
        .and_then(json::object)
        .unwrap_or_default()
}

/// S3 output rules 2 to 4: appends the members of `other` that are neither
/// reserved nor written by the object.
pub(crate) fn write_other(out: &mut ObjectWriter, members: &[Member<'_>], reserved: &[&str], written: &[&str]) {
    for (key, raw) in members {
        if !reserved.contains(&key.as_str()) && !written.contains(&key.as_str()) {
            out.raw(key, raw.get());
        }
    }
}

/// "Truncate" (S2): toward zero; negative values and NaN give 0, values above
/// the target range its maximum. Rust's float-to-integer `as` does exactly
/// that.
pub(crate) fn truncate_u32(value: f32) -> u32 {
    value as u32
}

pub(crate) fn truncate_u16(value: f32) -> u16 {
    value as u16
}
