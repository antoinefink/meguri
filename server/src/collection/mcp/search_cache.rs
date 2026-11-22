// SPDX-License-Identifier: MIT
//! Optional candidate acceleration, never search authority or admission.
//!
//! Call only inside the validated account's publication barrier, with a
//! committed read transaction on `current` and its server-selected pathname.
//! A hit still needs authoritative matching, paging limits and final authority
//! revalidation. Every miss/failure uses normal search in that same transaction.
use super::search_index::SearchIndex;
use crate::{collection::schema, storage::{DataRoot, collection::ReadPin}};
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

const RETAINED: usize = 2;
const FRESH_FOR: Duration = Duration::from_secs(60);
// Includes snapshot setup and time spent awaiting a Tokio blocking thread.
const BUILD_FOR: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp { usn: i64, modified: i64 }

fn stamp(c: &Connection) -> Option<Stamp> {
    // i64 extraction rejects text, blobs, NULL and real values, rather than
    // coercing a malformed stamp into an apparently fresh cache key.
    c.query_row("SELECT usn,mod FROM col WHERE id=1", [], |row| {
        Ok(Stamp { usn: row.get(0)?, modified: row.get(1)? })
    }).ok()
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    account: String,
    epoch: i64,
    path: PathBuf,
    stamp: Stamp,
}

struct Entry {
    key: Key,
    // None is a failed-build cooldown for this exact unchanged snapshot.
    index: Option<Arc<SearchIndex>>,
    expires: Instant,
}

struct Flight {
    // Identity rather than a wrapping counter; obsolete completions cannot
    // consume another reservation or overwrite its result.
    token: Arc<()>,
    key: Key,
    deadline: Instant,
    publish: bool,
}

#[derive(Default)]
struct State {
    // Least recently used first; at most one entry per account.
    entries: Vec<Entry>,
    flight: Option<Flight>,
}

/// Share one instance across the service. There is one reserved blocking job,
/// no pending-key queue, and at most two retained immutable account snapshots.
/// Entries hold no database handles, read pins, permits, barriers or DataRoot.
#[derive(Default)]
pub(crate) struct SearchCache {
    state: Mutex<State>,
    #[cfg(test)]
    probe: Mutex<Option<Arc<dyn Fn(BuildBoundary) + Send + Sync>>>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildBoundary { Scanning, Completed }


impl SearchCache {
    pub(crate) fn get_or_schedule(
        self: &Arc<Self>, current: &Connection, account: &str, epoch: i64,
        root: &Arc<DataRoot>, path: &Path,
    ) -> Option<Arc<SearchIndex>> {
        // Never accept a stamp from a sequence of independent autocommit reads.
        let version = (!current.is_autocommit()).then(|| stamp(current)).flatten();
        let Some(version) = version else {
            self.invalidate(account);
            return None;
        };
        let key = Key { account: account.to_owned(), epoch, path: path.to_owned(), stamp: version };
        // Plain synchronous unit callers can use an existing hit, but cannot
        // silently acquire a thread or create a runtime for a cold cache.
        let runtime = Handle::try_current().ok();
        let (hit, reservation) = self.lookup_or_reserve(&key, runtime.is_some(), Instant::now());
        if hit.is_some() { return hit; }
        let reservation = reservation?;
        let runtime = runtime?;

        // This small setup runs while the normal request still owns publication.
        // BEGIN alone does not establish a read snapshot: the stamp read does.
        // A reset may rename the entire directory despite the private read pin;
        // the worker must use this open transaction and NEVER reopen key.path.
        let snapshot = Snapshot::open(root, path, version, reservation.deadline)?;
        #[cfg(test)]
        let probe = self.probe.lock().ok().and_then(|p| p.clone());
        runtime.spawn_blocking(move || {
            #[cfg(test)]
            if let Some(probe) = &probe { probe(BuildBoundary::Scanning); }
            let index = if Instant::now() < reservation.deadline {
                SearchIndex::build(&snapshot.connection, reservation.deadline).ok()
            } else {
                None
            };
            // Close the reader before releasing its pin/root and before making
            // the result visible. Connection drop rolls back the read transaction.
            drop(snapshot);
            reservation.finish(index.map(Arc::new));
            #[cfg(test)]
            if let Some(probe) = &probe { probe(BuildBoundary::Completed); }
        });
        None
    }

    /// Optional eager eviction after reset/replacement/revocation. Key matching
    /// already prevents reuse across epochs, generation paths and write stamps.
    /// A running obsolete job still owns the single slot until it actually ends.
    pub(crate) fn invalidate(&self, account: &str) {
        let mut retired = Vec::new();
        if let Ok(mut state) = self.state.lock() {
            let mut i = 0;
            while i < state.entries.len() {
                if state.entries[i].key.account == account {
                    retired.push(state.entries.remove(i));
                } else { i += 1; }
            }
            if let Some(flight) = state.flight.as_mut() {
                if flight.key.account == account { flight.publish = false; }
            }
        }
        // A last Arc may free a large index; never do that under the mutex.
        drop(retired);
    }

    fn lookup_or_reserve(
        self: &Arc<Self>, key: &Key, can_schedule: bool, now: Instant,
    ) -> (Option<Arc<SearchIndex>>, Option<Reservation>) {
        let mut retired = Vec::new();
        let Ok(mut state) = self.state.lock() else { return (None, None); };
        let mut i = 0;
        while i < state.entries.len() {
            let entry = &state.entries[i];
            if now >= entry.expires || (entry.key.account == key.account && entry.key != *key) {
                retired.push(state.entries.remove(i));
            } else { i += 1; }
        }
        if let Some(flight) = state.flight.as_mut() {
            if flight.key.account == key.account && flight.key != *key {
                // Irreversible for this job, even if a later old read asks for
                // its key again. No obsolete result can become fresh again.
                flight.publish = false;
            }
        }
        let found = state.entries.iter().position(|entry| entry.key == *key);
        let hit = found.and_then(|i| {
            let entry = state.entries.remove(i);
            let index = entry.index.clone();
            state.entries.push(entry);
            index
        });
        let reservation = if found.is_none() && can_schedule && state.flight.is_none() {
            let token = Arc::new(());
            let deadline = now + BUILD_FOR;
            state.flight = Some(Flight { token: Arc::clone(&token), key: key.clone(), deadline, publish: true });
            Some(Reservation { cache: Arc::clone(self), token, deadline, finished: false })
        } else { None };
        drop(state);
        drop(retired);
        (hit, reservation)
    }

    fn complete(&self, token: &Arc<()>, mut index: Option<Arc<SearchIndex>>, now: Instant) {
        let mut retired = Vec::new();
        if let Ok(mut state) = self.state.lock() {
            let owns_slot = state.flight.as_ref().is_some_and(|flight| Arc::ptr_eq(&flight.token, token));
            if owns_slot {
                if let Some(flight) = state.flight.take() {
                    if flight.publish {
                        // Late or failed jobs are also retained, without data,
                        // to bound repeated attempts at an unchanged stamp.
                        let accepted = if now < flight.deadline { index.take() } else { None };
                        let mut i = 0;
                        while i < state.entries.len() {
                            let entry = &state.entries[i];
                            if now >= entry.expires || entry.key.account == flight.key.account {
                                retired.push(state.entries.remove(i));
                            } else { i += 1; }
                        }
                        if state.entries.len() == RETAINED { retired.push(state.entries.remove(0)); }
                        state.entries.push(Entry { key: flight.key, index: accepted, expires: now + FRESH_FOR });
                    }
                }
            }
        }
        drop(retired);
        drop(index);
    }

    #[cfg(test)]
    pub(crate) fn test_probe(&self, probe: Arc<dyn Fn(BuildBoundary) + Send + Sync>) {
        *self.probe.lock().unwrap() = Some(probe);
    }

    /// Test-hook adapters can inspect completion without retaining a Service in
    /// the worker or introducing a production wait/notification queue. A hit is
    /// inspected with get_or_schedule under the usual authority/transaction.
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn test_is_building(&self) -> Option<bool> {
        self.state.lock().ok().map(|state| state.flight.is_some())
    }
}

// RAII releases the reservation on snapshot failure, panic, or a queued Tokio
// closure being discarded at shutdown. No Service or request guard is captured.
struct Reservation {
    cache: Arc<SearchCache>,
    token: Arc<()>,
    deadline: Instant,
    finished: bool,
}
impl Reservation {
    fn finish(mut self, index: Option<Arc<SearchIndex>>) {
        self.cache.complete(&self.token, index, Instant::now());
        self.finished = true;
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.finished { self.cache.complete(&self.token, None, Instant::now()); }
    }
}

// Field order is deliberate: the SQLite reader closes before the pin and root.
struct Snapshot {
    connection: Connection,
    _pin: ReadPin,
    _root: Arc<DataRoot>,
}
impl Snapshot {
    fn open(root: &Arc<DataRoot>, path: &Path, expected: Stamp, deadline: Instant) -> Option<Self> {
        if Instant::now() >= deadline { return None; }
        let pin = root.pin_collection(path).ok()?;
        let mut connection = schema::open_mcp(path, false).ok()?;
        install_deadline(&mut connection, deadline).ok()?;
        connection.execute_batch("BEGIN DEFERRED").ok()?;
        if stamp(&connection)? != expected || Instant::now() >= deadline { return None; }
        Some(Self { connection, _pin: pin, _root: Arc::clone(root) })
    }
}

fn install_deadline(connection: &mut Connection, deadline: Instant) -> rusqlite::Result<()> {
    connection.progress_handler(1_000, Some(move || Instant::now() >= deadline))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(account: &str) -> Key {
        Key { account: account.into(), epoch: 1, path: PathBuf::from("initial/collection"),
            stamp: Stamp { usn: 2, modified: 3 } }
    }
    fn index() -> Arc<SearchIndex> {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE notes(id INTEGER PRIMARY KEY,tags TEXT,flds TEXT);
            CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,odid INTEGER,
                ord INTEGER DEFAULT 0,type INTEGER DEFAULT 0,due INTEGER DEFAULT 0,odue INTEGER DEFAULT 0);
            CREATE INDEX ix_cards_nid ON cards(nid); BEGIN DEFERRED;").unwrap();
        Arc::new(SearchIndex::build(&c, Instant::now() + BUILD_FOR).unwrap())
    }
    fn reserve(cache: &Arc<SearchCache>, key: &Key, now: Instant) -> Reservation {
        let (hit, reservation) = cache.lookup_or_reserve(key, true, now);
        assert!(hit.is_none());
        reservation.unwrap()
    }
    fn publish(cache: &Arc<SearchCache>, key: &Key, now: Instant, index: &Arc<SearchIndex>) {
        let mut reservation = reserve(cache, key, now);
        cache.complete(&reservation.token, Some(Arc::clone(index)), now);
        reservation.finished = true;
    }

    #[test]
    fn one_reservation_no_queue_and_drop_releases_slot() {
        let cache = Arc::new(SearchCache::default());
        let now = Instant::now();
        let first = reserve(&cache, &key("a"), now);
        let (hit, pending) = cache.lookup_or_reserve(&key("b"), true, now);
        assert!(hit.is_none() && pending.is_none());
        assert_eq!(cache.test_is_building(), Some(true));
        drop(first);
        assert_eq!(cache.test_is_building(), Some(false));
        assert!(cache.lookup_or_reserve(&key("a"), true, now).1.is_none(), "failed job must cool down");
        let retry_at = cache.state.lock().unwrap().entries[0].expires;
        assert!(cache.lookup_or_reserve(&key("a"), true, retry_at).1.is_some());
        drop(reserve(&cache, &key("b"), now));
    }

    #[test]
    fn changed_epoch_path_or_stamp_evicts_and_obsoletes_inflight() {
        let index = index();
        for dimension in 0..4 {
            let cache = Arc::new(SearchCache::default());
            let now = Instant::now();
            let original = key("a");
            publish(&cache, &original, now, &index);
            let mut changed = original.clone();
            match dimension {
                0 => changed.epoch += 1,
                1 => changed.path = PathBuf::from("replacement/collection"),
                2 => changed.stamp.usn += 1,
                _ => changed.stamp.modified += 1,
            }
            let running = reserve(&cache, &changed, now);
            assert!(cache.state.lock().unwrap().entries.is_empty());
            // Observing another publication permanently invalidates this job.
            assert!(cache.lookup_or_reserve(&original, true, now).1.is_none());
            running.finish(Some(Arc::clone(&index)));
            assert!(cache.lookup_or_reserve(&changed, false, now).0.is_none());
        }
    }

    #[test]
    fn two_accounts_lru_expiry_and_external_snapshot_ownership() {
        let cache = Arc::new(SearchCache::default());
        let now = Instant::now();
        let index = index();
        publish(&cache, &key("a"), now, &index);
        publish(&cache, &key("b"), now, &index);
        let held = cache.lookup_or_reserve(&key("a"), false, now).0.unwrap();
        publish(&cache, &key("c"), now, &index);
        assert!(cache.lookup_or_reserve(&key("b"), false, now).0.is_none());
        assert_eq!(cache.state.lock().unwrap().entries.len(), RETAINED);
        assert!(cache.lookup_or_reserve(&key("a"), false, now + FRESH_FOR).0.is_none());
        assert!(cache.state.lock().unwrap().entries.is_empty());
        assert!(Arc::ptr_eq(&held, &index));
    }

    #[test]
    fn invalidation_and_late_completion_cannot_take_a_new_slot() {
        let cache = Arc::new(SearchCache::default());
        let now = Instant::now();
        let index = index();
        let old = reserve(&cache, &key("a"), now);
        let old_token = Arc::clone(&old.token);
        cache.invalidate("a");
        old.finish(Some(Arc::clone(&index)));
        assert!(cache.state.lock().unwrap().entries.is_empty());
        let new = reserve(&cache, &key("b"), now);
        cache.complete(&old_token, Some(Arc::clone(&index)), now);
        assert_eq!(cache.test_is_building(), Some(true));
        new.finish(Some(index));
        assert!(cache.lookup_or_reserve(&key("b"), false, now).0.is_some());
    }

    #[test]
    fn overdue_build_and_malformed_stamp_are_unavailable() {
        let cache = Arc::new(SearchCache::default());
        let now = Instant::now();
        let mut reservation = reserve(&cache, &key("a"), now);
        cache.complete(&reservation.token, Some(index()), now + BUILD_FOR);
        reservation.finished = true;
        assert!(cache.lookup_or_reserve(&key("a"), false, now).0.is_none());
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE col(id INTEGER PRIMARY KEY,usn,mod);
            INSERT INTO col VALUES(1,2,3);").unwrap();
        assert!(stamp(&c) == Some(Stamp { usn: 2, modified: 3 }));
        for sql in ["UPDATE col SET usn='2'", "UPDATE col SET usn=2.0",
            "UPDATE col SET usn=NULL", "UPDATE col SET usn=2,mod=x'33'"] {
            c.execute_batch(sql).unwrap();
            assert!(stamp(&c).is_none());
        }
    }
}
