//! `sanityCheck2`, `finish` and `abort` (specs/sync/wire-format.md W12,
//! W13; R1530): the counts tuple both sides compare and the answers that end
//! a normal sync. Which state the client counts is U107's.

use meguri_model::time::EpochMillis;
use serde_json::value::RawValue;

use super::read::{self, At};
use super::write;
use crate::error::JobError;
use crate::transport::Method;

/// The counts tuple (W12.1), every count a `u32`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SanityCounts {
    /// `[new, learning, review]`; Meguri, like Anki, sends zeros.
    pub due: [u32; 3],
    pub cards: u32,
    pub notes: u32,
    pub revlog: u32,
    pub graves: u32,
    /// Rows in `notetypes`.
    pub notetypes: u32,
    pub decks: u32,
    pub deck_configs: u32,
}

impl SanityCounts {
    fn write(&self, out: &mut String) {
        out.push('[');
        write::ints(out, &self.due);
        for count in [
            self.cards,
            self.notes,
            self.revlog,
            self.graves,
            self.notetypes,
            self.decks,
            self.deck_configs,
        ] {
            out.push(',');
            write::int(out, count);
        }
        out.push(']');
    }

    fn read(raw: &RawValue, at: At<'_>) -> read::Result<SanityCounts> {
        let items = read::tuple(raw, 8, 8, at)?;
        let due_at = at.index(0);
        let due = read::tuple(items[0], 3, 3, due_at)?;
        let count = |index: usize| read::strict::<u32>(items[index], at.index(index));
        Ok(SanityCounts {
            due: [
                read::strict::<u32>(due[0], due_at.index(0))?,
                read::strict::<u32>(due[1], due_at.index(1))?,
                read::strict::<u32>(due[2], due_at.index(2))?,
            ],
            cards: count(1)?,
            notes: count(2)?,
            revlog: count(3)?,
            graves: count(4)?,
            notetypes: count(5)?,
            decks: count(6)?,
            deck_configs: count(7)?,
        })
    }
}

/// The `sanityCheck2` request: `{"client":<counts tuple>}`.
pub fn encode_sanity_check(client: &SanityCounts) -> Vec<u8> {
    let mut out = String::from("{\"client\":");
    client.write(&mut out);
    out.push('}');
    out.into_bytes()
}

/// The server's verdict (W12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanityAnswer {
    Ok,
    /// The failed sanity check (`sanity_check_failed`): `c` and `s` when
    /// they are valid counts tuples.
    Bad {
        client: Option<SanityCounts>,
        server: Option<SanityCounts>,
    },
}

/// The `sanityCheck2` answer: `status` exactly `"ok"` or `"bad"`; `c` and
/// `s` count as absent when missing, `null` or not valid counts tuples.
pub fn decode_sanity_answer(body: &[u8]) -> Result<SanityAnswer, JobError> {
    decode(body).map_err(|err| JobError::bad_response(Method::SanityCheck2, err.to_string()))
}

fn decode(body: &[u8]) -> read::Result<SanityAnswer> {
    let at = At::Root;
    let root = read::object(read::body(body)?, at)?;
    let status_at = at.member("status");
    let status = read::string(root.required("status", at)?, status_at)?;
    let counts = |key: &str| -> read::Result<Option<SanityCounts>> {
        Ok(root
            .get(key, at)?
            .and_then(|raw| SanityCounts::read(raw, at.member(key)).ok()))
    };
    let client = counts("c")?;
    let server = counts("s")?;
    match status.as_str() {
        "ok" => Ok(SanityAnswer::Ok),
        "bad" => Ok(SanityAnswer::Bad { client, server }),
        _ => Err(status_at.fail("is not \"ok\" or \"bad\"")),
    }
}

/// The `finish` answer (W13): the server's new modification time, which the
/// client stores as its `mod` and `ls` (U107).
pub fn decode_finish(body: &[u8]) -> Result<EpochMillis, JobError> {
    read::body(body)
        .and_then(|raw| read::strict::<i64>(raw, At::Root))
        .map(EpochMillis)
        .map_err(|err| JobError::bad_response(Method::Finish, err.to_string()))
}
