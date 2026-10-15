//! Preconditions and the touch index (`engine/op-model` O7). The executor
//! checks an op's preconditions inside its write transaction, before the
//! op's closure runs; one that fails rolls the op back with `Stale`. Never
//! mtime or `usn`: `Local` stamping has one-second resolution.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use meguri_storage::{ReadRows, RowKey, Table, WriteScope};

use crate::changes::stream::Seq;
use crate::error::EngineError;

/// What must still hold when the write phase runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precondition {
    /// The stored row's digest (`ReadRows::row_digest`) equals `digest`;
    /// `None` means no row has the key. The answer's `stateToken` is such a
    /// digest of the card row (R727).
    RowDigest {
        table: Table,
        key: RowKey,
        digest: Option<[u8; 32]>,
    },
    /// No op with a seq above `since` changed the row.
    Untouched { table: Table, key: RowKey, since: Seq },
    /// No op with a seq above `since` changed any row of the table: the set
    /// precondition of a predicate-targeted op.
    TableUntouched { table: Table, since: Seq },
}

impl Precondition {
    fn stale(&self) -> EngineError {
        let (table, key) = match self {
            Precondition::RowDigest { table, key, .. } | Precondition::Untouched { table, key, .. } => {
                (*table, key.to_string())
            }
            Precondition::TableUntouched { table, .. } => (*table, "*".to_owned()),
        };
        EngineError::Stale { table, key }
    }
}

/// Checks every precondition; the first that fails gives `Stale`.
pub(crate) fn check(
    preconditions: &[Precondition],
    scope: &WriteScope<'_>,
    touched: &TouchIndex,
) -> Result<(), EngineError> {
    for precondition in preconditions {
        let holds = match precondition {
            Precondition::RowDigest { table, key, digest } => scope.row_digest(*table, key)? == *digest,
            Precondition::Untouched { table, key, since } => touched.untouched(*table, key, *since),
            Precondition::TableUntouched { table, since } => touched.table_untouched(*table, *since),
        };
        if !holds {
            return Err(precondition.stale());
        }
    }
    Ok(())
}

/// The most keys the index holds before it starts over (O7).
pub(crate) const TOUCH_INDEX_LIMIT: usize = 1_000_000;

/// For every changed key, the seq of the last op that changed it; per table,
/// the last seq that changed any of its rows. A proof for a snapshot older
/// than a floor fails: `file_floor` is the last seq before this collection
/// file was opened; `key_floor` also moves when the key map starts over.
#[derive(Debug)]
pub(crate) struct TouchIndex {
    file_floor: Seq,
    key_floor: Seq,
    keys: BTreeMap<TouchKey, Seq>,
    tables: HashMap<Table, Seq>,
    limit: usize,
}

impl TouchIndex {
    /// An index that knows nothing before `floor` (the last seq committed
    /// before this collection file was opened).
    pub(crate) fn new(floor: Seq) -> TouchIndex {
        TouchIndex::with_limit(floor, TOUCH_INDEX_LIMIT)
    }

    pub(crate) fn with_limit(floor: Seq, limit: usize) -> TouchIndex {
        TouchIndex {
            file_floor: floor,
            key_floor: floor,
            keys: BTreeMap::new(),
            tables: HashMap::new(),
            limit,
        }
    }

    /// Forgets everything up to `floor`: a new collection file.
    pub(crate) fn reset(&mut self, floor: Seq) {
        self.file_floor = floor;
        self.key_floor = floor;
        self.keys.clear();
        self.tables.clear();
    }

    /// Records the keys a committed op changed.
    pub(crate) fn record<'k>(&mut self, seq: Seq, keys: impl IntoIterator<Item = (Table, &'k RowKey)>, count: usize) {
        if self.keys.len().saturating_add(count) > self.limit {
            // Start over: every proof about earlier ops now fails.
            self.keys.clear();
            self.key_floor = seq;
            for (table, _) in keys {
                self.tables.insert(table, seq);
            }
            return;
        }
        for (table, key) in keys {
            self.tables.insert(table, seq);
            self.keys.insert(
                TouchKey {
                    table,
                    key: key.clone(),
                },
                seq,
            );
        }
    }

    pub(crate) fn untouched(&self, table: Table, key: &RowKey, since: Seq) -> bool {
        if since < self.key_floor {
            return false;
        }
        let probe = TouchKey {
            table,
            key: key.clone(),
        };
        self.keys.get(&probe).is_none_or(|last| *last <= since)
    }

    pub(crate) fn table_untouched(&self, table: Table, since: Seq) -> bool {
        // Per-table seqs are never forgotten within one file.
        since >= self.file_floor && self.tables.get(&table).is_none_or(|last| *last <= since)
    }
}

/// A key as its table compares keys: `tags` by `unicase`, others byte for
/// byte. Also the held-back run's row key (U25).
#[derive(Clone, Debug)]
pub(crate) struct TouchKey {
    table: Table,
    key: RowKey,
}

impl TouchKey {
    pub(crate) fn new(table: Table, key: RowKey) -> TouchKey {
        TouchKey { table, key }
    }
}

impl Ord for TouchKey {
    fn cmp(&self, other: &TouchKey) -> Ordering {
        self.table
            .cmp(&other.table)
            .then_with(|| match (&self.key, &other.key) {
                (RowKey::Text(a), RowKey::Text(b)) if self.table == Table::Tags => {
                    meguri_text::fold::compare_folded(&String::from_utf8_lossy(a), &String::from_utf8_lossy(b))
                }
                (a, b) => a.cmp(b),
            })
    }
}

impl PartialOrd for TouchKey {
    fn partial_cmp(&self, other: &TouchKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TouchKey {
    fn eq(&self, other: &TouchKey) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for TouchKey {}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: i64) -> RowKey {
        RowKey::Id(id)
    }

    #[test]
    fn a_key_is_untouched_until_an_op_after_the_snapshot_changes_it() {
        let mut index = TouchIndex::new(Seq(10));
        index.record(Seq(11), [(Table::Notes, &key(1))], 1);
        assert!(index.untouched(Table::Notes, &key(1), Seq(11)));
        assert!(!index.untouched(Table::Notes, &key(1), Seq(10)));
        assert!(index.untouched(Table::Notes, &key(2), Seq(10)));
        assert!(index.untouched(Table::Cards, &key(1), Seq(10)), "keys are per table");
        assert!(
            !index.untouched(Table::Notes, &key(2), Seq(9)),
            "before the file was opened"
        );
        assert!(!index.table_untouched(Table::Notes, Seq(10)));
        assert!(index.table_untouched(Table::Notes, Seq(11)));
        assert!(index.table_untouched(Table::Decks, Seq(10)));
        assert!(!index.table_untouched(Table::Decks, Seq(9)));
    }

    #[test]
    fn tag_keys_compare_with_unicase() {
        let mut index = TouchIndex::new(Seq(0));
        index.record(Seq(5), [(Table::Tags, &RowKey::text("Straße"))], 1);
        assert!(!index.untouched(Table::Tags, &RowKey::text("STRASSE"), Seq(4)));
        assert!(index.untouched(Table::Tags, &RowKey::text("Strasse2"), Seq(4)));
        assert!(index.untouched(Table::Config, &RowKey::text("STRASSE"), Seq(4)));
    }

    #[test]
    fn past_its_limit_the_index_starts_over_and_older_proofs_fail() {
        let mut index = TouchIndex::with_limit(Seq(0), 3);
        index.record(Seq(1), [(Table::Notes, &key(1)), (Table::Notes, &key(2))], 2);
        assert!(index.untouched(Table::Notes, &key(3), Seq(0)));
        // Two more keys pass the limit of three.
        index.record(Seq(2), [(Table::Cards, &key(7)), (Table::Cards, &key(8))], 2);
        assert!(!index.untouched(Table::Notes, &key(3), Seq(1)), "forgotten");
        assert!(index.untouched(Table::Notes, &key(3), Seq(2)));
        assert!(!index.table_untouched(Table::Cards, Seq(1)), "tables are still known");
        index.record(Seq(3), [(Table::Notes, &key(3))], 1);
        assert!(!index.untouched(Table::Notes, &key(3), Seq(2)));
        index.reset(Seq(3));
        assert!(index.untouched(Table::Notes, &key(3), Seq(3)));
        assert!(!index.untouched(Table::Notes, &key(3), Seq(2)));
    }
}
