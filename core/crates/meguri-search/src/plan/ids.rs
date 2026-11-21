//! The id statements of `browser/result-sets` (U85): what a result handle
//! reads besides its search's own statement. Each takes the ids it needs as
//! one JSON array bound as `?1` ([`super::sql::id_list`]) and returns ids
//! only, so the core can map, resolve and set aside rows without reading
//! whole records.

/// Cards among `?1` whose note row is missing, each with its `nid` (H5, H8:
/// the cards a note-column sort sets aside).
pub const LONE_CARDS_AMONG: &str = "SELECT c.id, c.nid FROM cards c WHERE c.id IN (SELECT value FROM json_each(?1)) \
     AND NOT EXISTS (SELECT 1 FROM notes k WHERE k.id = c.nid)";

/// Every card of the notes in `?1`, as card id and note id, ascending card
/// id (H11, H12).
pub const CARDS_OF_NOTES: &str =
    "SELECT c.id, c.nid FROM cards c WHERE c.nid IN (SELECT value FROM json_each(?1)) ORDER BY c.id";

/// The cards of `?1` that exist, with their `nid` (H12: the notes of cards).
pub const NOTES_OF_CARDS: &str = "SELECT c.id, c.nid FROM cards c WHERE c.id IN (SELECT value FROM json_each(?1))";

/// The notes of `?1` that exist (H11: a card-scope toggle keeps only notes
/// whose row exists).
pub const EXISTING_NOTES: &str = "SELECT n.id FROM notes n WHERE n.id IN (SELECT value FROM json_each(?1))";

/// One row, 1 when some card has no note row (Q6).
pub const ANY_LONE_CARD: &str =
    "SELECT EXISTS (SELECT 1 FROM cards c WHERE NOT EXISTS (SELECT 1 FROM notes k WHERE k.id = c.nid))";

/// One row, 1 when some note has no card (Q6).
pub const ANY_LONE_NOTE: &str =
    "SELECT EXISTS (SELECT 1 FROM notes n WHERE NOT EXISTS (SELECT 1 FROM cards k WHERE k.nid = n.id))";

/// The cards of `?1` that exist, each with its home deck (`odid` for a card
/// in a filtered deck, else `did`) and its stored queue (Browse's selection
/// counts, `browser/workspace` V7 and V8.1's "shown when").
pub const HOME_DECKS_AND_QUEUES: &str = "SELECT c.id, CASE WHEN c.odid != 0 THEN c.odid ELSE c.did END, c.queue FROM cards c \
     WHERE c.id IN (SELECT value FROM json_each(?1))";

/// The notes of `?1` that exist, each with its note type (V8.1).
pub const NOTETYPES_OF_NOTES: &str = "SELECT n.id, n.mid FROM notes n WHERE n.id IN (SELECT value FROM json_each(?1))";
