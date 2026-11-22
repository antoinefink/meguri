// SPDX-License-Identifier: MIT
//! Disposable, snapshot-bound candidate lists. Never an authority for a match.
//!
//! The caller owns an already established committed collection read snapshot,
//! its SQLite busy/progress limits, and cache invalidation. This module neither
//! opens transactions nor changes storage. Deadlines are cooperative: SQLite
//! stepping itself must be bounded by the caller's connection policy.
use super::{Result, MAX_RECORD_BYTES, MAX_TAGS, generation, read::tag_list, sql_error, unsupported};
use super::super::notes::fields;
use regex_syntax::hir::{ClassUnicode, ClassUnicodeRange};
use rusqlite::Connection;
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::{BuildHasher, Hash, Hasher},
    time::Instant,
};
use unicase::UniCase;

const TAG_VALUE_BYTES: usize = 16 * 1024;
const NOTE_SCALARS: usize = 32 * 1024;
const QUERY_BYTES: usize = 1024;
const FOLD_CACHE_CHARS: usize = 4096;
const SCALAR_BASE: u64 = 0x110000;

#[derive(Clone, Copy)]
struct Capacity { keys: usize, slots: usize }

#[derive(Clone, Copy)]
struct Limits {
    deck: Capacity,
    tag: Capacity,
    text: Capacity,
    card_rows: usize,
    note_rows: usize,
    tag_input_bytes: usize,
    text_input_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            deck: Capacity { keys: 32 * 1024, slots: 1024 * 1024 },
            tag: Capacity { keys: 128 * 1024, slots: 2 * 1024 * 1024 },
            text: Capacity { keys: 256 * 1024, slots: 4 * 1024 * 1024 },
            card_rows: 2_000_000,
            note_rows: 500_000,
            tag_input_bytes: 32 * 1024 * 1024,
            text_input_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Keys are deck IDs, keyed UniCase tag hashes, or injectively encoded grams.
/// No stored tags, field strings, queries or full folded fields survive build.
///
/// On 64-bit targets the default maps reserve at most 851,968 buckets total:
/// about 27 MiB for (u64, Vec<i64>) plus control bytes. They never grow after
/// that reservation. Posting *capacity*, including geometric slack, is capped
/// at 7 Mi slots / 56 MiB across the three facets. A vector reallocation can
/// temporarily retain its old buffer; allow a conservative 32 MiB extra.
/// Fold cache and per-row scratch stay below 1 MiB (tags <=16 KiB, fields
/// <=128 KiB). Thus requested Rust heap stays below about 116 MiB at peak,
/// excluding allocator metadata and the caller-owned SQLite connection/cache.
/// Each failed reservation discards its entire facet. No partial map escapes.
struct Postings {
    map: HashMap<u64, Vec<i64>>,
    limits: Capacity,
    slots: usize,
}

impl Postings {
    fn new(limits: Capacity) -> Option<Self> {
        let mut map = HashMap::new();
        map.try_reserve(limits.keys).ok()?;
        Some(Self { map, limits, slots: 0 })
    }

    // Build visits note IDs in ascending order, including the card scan.
    fn insert(&mut self, key: u64, note: i64) -> bool {
        if self.map.len() == self.limits.keys && !self.map.contains_key(&key) {
            return false;
        }
        let posting = self.map.entry(key).or_default();
        if posting.last() == Some(&note) { return true; }
        debug_assert!(posting.last().is_none_or(|last| *last < note));
        if posting.len() == posting.capacity() {
            let capacity = posting.capacity().saturating_mul(2).max(4);
            let extra = capacity - posting.capacity();
            if extra > self.limits.slots.saturating_sub(self.slots) { return false; }
            let old = posting.capacity();
            if posting.try_reserve_exact(capacity - posting.len()).is_err() { return false; }
            self.slots += posting.capacity() - old;
            if self.slots > self.limits.slots { return false; }
        }
        posting.push(note);
        true
    }

    fn get(&self, key: u64) -> &[i64] {
        self.map.get(&key).map_or(&[], Vec::as_slice)
    }
}

/// `None` facets are unavailable, including when their scan was incomplete.
/// A present facet is complete, so absent keys safely return an empty slice.
pub(crate) struct SearchIndex {
    decks: Option<Postings>,
    tags: Option<Postings>,
    text: Option<Postings>,
    tag_hash: RandomState,
}

impl SearchIndex {
    pub(super) fn build(c: &Connection, deadline: Instant) -> Result<Self> {
        Self::build_with_limits(c, deadline, Limits::default())
    }

    fn build_with_limits(c: &Connection, deadline: Instant, limits: Limits) -> Result<Self> {
        // Prevent an accidental build from mixing independent autocommit reads.
        if c.is_autocommit() { return Err(unsupported()); }
        let mut index = Self {
            decks: Self::build_decks(c, deadline, limits)?,
            tags: None,
            text: None,
            tag_hash: RandomState::new(),
        };
        if Instant::now() >= deadline { return Ok(index); }
        index.tags = Postings::new(limits.tag);
        index.text = Postings::new(limits.text);
        if index.tags.is_none() && index.text.is_none() { return Ok(index); }

        // CASE guards run inside SQLite, before either overflow value can be
        // returned/materialized. octet_length includes bytes after embedded NUL.
        // A wrong storage class is also an unavailable facet, never empty text.
        let mut q = c.prepare(
            "SELECT id,
             CASE WHEN typeof(tags)='text' AND octet_length(tags)<=?1 THEN tags END,
             CASE WHEN typeof(flds)='text' AND octet_length(flds)<=?2 THEN flds END
             FROM notes ORDER BY id LIMIT ?3"
        ).map_err(sql_error)?;
        let mut rows = q.query(rusqlite::params![
            TAG_VALUE_BYTES as i64, MAX_RECORD_BYTES as i64,
            limits.note_rows.saturating_add(1) as i64,
        ]).map_err(sql_error)?;
        let mut fold = FoldCache::new();
        let mut tag_bytes = 0usize;
        let mut text_bytes = 0usize;
        let mut count = 0usize;
        loop {
            if Instant::now() >= deadline {
                index.tags = None;
                index.text = None;
                break;
            }
            let Some(row) = rows.next().map_err(sql_error)? else { break; };
            if count == limits.note_rows {
                index.tags = None;
                index.text = None;
                break;
            }
            count += 1;
            let id: i64 = row.get(0).map_err(sql_error)?;
            if index.tags.is_some() {
                let value: Option<String> = row.get(1).map_err(sql_error)?;
                let complete = value.is_some_and(|value| {
                    tag_bytes = tag_bytes.saturating_add(value.len());
                    if tag_bytes > limits.tag_input_bytes { return false; }
                    let tags = tag_list(&value);
                    if tags.len() > MAX_TAGS { return false; }
                    tags.iter().all(|tag| {
                        Instant::now() < deadline && index.tags.as_mut().unwrap()
                            .insert(tag_key(&index.tag_hash, tag), id)
                    })
                });
                if !complete { index.tags = None; }
            }
            if let Some(postings) = index.text.as_mut() {
                let value: Option<String> = row.get(2).map_err(sql_error)?;
                let complete = value.is_some_and(|value| {
                    text_bytes = text_bytes.saturating_add(value.len());
                    text_bytes <= limits.text_input_bytes &&
                        add_text(postings, &mut fold, id, &value, deadline)
                });
                if !complete { index.text = None; }
            }
            if index.tags.is_none() && index.text.is_none() { break; }
        }
        Ok(index)
    }

    fn build_decks(c: &Connection, deadline: Instant, limits: Limits) -> Result<Option<Postings>> {
        if Instant::now() >= deadline { return Ok(None); }
        // Qualify only this facet with the authority's exact index checks.
        // An unsuitable sibling index must neither produce trusted empty deck
        // postings nor disable independent text/tag facets.
        if generation::prepare_siblings(c).is_err() { return Ok(None); }
        let Some(mut decks) = Postings::new(limits.deck) else { return Ok(None); };
        // The existing nid index orders by nid then rowid/id. Do not DISTINCT
        // or sort the source: those could consume unbounded SQLite temp space.
        // Count orphan rows too, so they cannot evade the source-row budget.
        let mut q = c.prepare(
            "SELECT nid,did,odid,EXISTS(SELECT 1 FROM notes WHERE notes.id=cards.nid)
             FROM cards INDEXED BY ix_cards_nid ORDER BY nid,id LIMIT ?1"
        ).map_err(sql_error)?;
        let mut rows = q.query([limits.card_rows.saturating_add(1) as i64]).map_err(sql_error)?;
        let mut count = 0usize;
        loop {
            if Instant::now() >= deadline { return Ok(None); }
            let Some(row) = rows.next().map_err(sql_error)? else { return Ok(Some(decks)); };
            if count == limits.card_rows { return Ok(None); }
            count += 1;
            if !row.get::<_, bool>(3).map_err(sql_error)? { continue; }
            let note: i64 = row.get(0).map_err(sql_error)?;
            let did: i64 = row.get(1).map_err(sql_error)?;
            let odid: i64 = row.get(2).map_err(sql_error)?;
            let home = if odid != 0 { odid } else { did };
            if !decks.insert(home as u64, note) { return Ok(None); }
        }
    }

    /// Only list lookups and bounded query work; never scans indexed notes.
    /// Always verify all supplied facets against authoritative stored records.
    /// With a deck request, None explicitly asks for the caller's cold deck
    /// path, even if another facet has a usable (or empty) posting.
    pub(crate) fn candidates(
        &self, deck_id: Option<i64>, tag: Option<&str>, text: Option<&str>,
    ) -> Option<&[i64]> {
        if let Some(deck) = deck_id {
            return self.decks.as_ref().map(|decks| decks.get(deck as u64));
        }
        let tags = tag.filter(|tag| !tag.is_empty() && tag.len() <= QUERY_BYTES)
            .and_then(|tag| self.tags.as_ref().map(|tags| tags.get(tag_key(&self.tag_hash, tag))));
        let text = text.and_then(|query| self.text_candidates(query));
        match (tags, text) {
            (Some(a), Some(b)) => Some(if a.len() <= b.len() { a } else { b }),
            (a, b) => a.or(b),
        }
    }

    #[cfg(test)]
    pub(super) fn test_without_decks(mut self) -> Self {
        self.decks = None;
        self
    }

    fn text_candidates(&self, query: &str) -> Option<&[i64]> {
        if query.is_empty() || query.len() > QUERY_BYTES { return None; }
        let postings = self.text.as_ref()?;
        let mut window = GramWindow::default();
        let mut rarest: Option<&[i64]> = None;
        for c in query.chars() {
            for gram in window.push(simple_fold(c)?).into_iter().flatten() {
                let posting = postings.get(gram);
                if posting.is_empty() { return Some(posting); }
                if rarest.is_none_or(|old| posting.len() < old.len()) {
                    rarest = Some(posting);
                }
            }
        }
        rarest
    }
}

fn tag_key(state: &RandomState, tag: &str) -> u64 {
    // UniCase's own Hash is consistent with its equality, including legacy
    // full-fold equivalences. No lowercasing/NFC or stored-byte changes here.
    // A 64-bit collision only admits extra candidates for final verification.
    let mut hash = state.build_hasher();
    UniCase::new(tag).hash(&mut hash);
    hash.finish()
}

fn simple_fold(c: char) -> Option<u32> {
    let mut class = ClassUnicode::new([ClassUnicodeRange::new(c, c)]);
    class.try_case_fold_simple().ok()?;
    class.iter().next().map(|range| range.start() as u32)
}

struct FoldCache { values: HashMap<char, u32> }
impl FoldCache {
    fn new() -> Self {
        let mut values = HashMap::new();
        // Cache failure only disables memoization, not Unicode correctness.
        let _ = values.try_reserve(FOLD_CACHE_CHARS);
        Self { values }
    }
    fn get(&mut self, c: char) -> Option<u32> {
        if let Some(&folded) = self.values.get(&c) { return Some(folded); }
        let folded = simple_fold(c)?;
        if self.values.len() < self.values.capacity().min(FOLD_CACHE_CHARS) {
            self.values.insert(c, folded);
        }
        Some(folded)
    }
}

#[derive(Default)]
struct GramWindow { tail: [u32; 2], len: usize }
impl GramWindow {
    fn push(&mut self, c: u32) -> [Option<u64>; 3] {
        let c = u64::from(c);
        let one = c;
        let two = (self.len >= 1).then(|| SCALAR_BASE + u64::from(self.tail[1]) * SCALAR_BASE + c);
        let three = (self.len >= 2).then(|| SCALAR_BASE + SCALAR_BASE * SCALAR_BASE +
            u64::from(self.tail[0]) * SCALAR_BASE * SCALAR_BASE +
            u64::from(self.tail[1]) * SCALAR_BASE + c);
        self.tail = [self.tail[1], c as u32];
        self.len = (self.len + 1).min(2);
        // Width intervals are disjoint even for leading NUL scalars. Three
        // base-0x110000 digits plus both offsets fit comfortably within u64.
        [Some(one), two, three]
    }
}

fn add_text(
    postings: &mut Postings, fold: &mut FoldCache, id: i64, stored: &str, deadline: Instant,
) -> bool {
    let mut scalars = 0usize;
    for field in fields(stored) {
        let mut window = GramWindow::default(); // No grams across field boundaries.
        for c in field.chars() {
            if scalars == NOTE_SCALARS { return false; }
            if scalars % 128 == 0 && Instant::now() >= deadline { return false; }
            scalars += 1;
            let Some(folded) = fold.get(c) else { return false; };
            for gram in window.push(folded).into_iter().flatten() {
                if !postings.insert(gram, id) { return false; }
            }
        }
    }
    Instant::now() < deadline
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn database() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE notes(id INTEGER PRIMARY KEY,tags TEXT,flds TEXT);
             CREATE TABLE cards(id INTEGER PRIMARY KEY,nid INTEGER,did INTEGER,odid INTEGER,
                ord INTEGER DEFAULT 0,type INTEGER DEFAULT 0,due INTEGER DEFAULT 0,odue INTEGER DEFAULT 0);
             CREATE INDEX ix_cards_nid ON cards(nid);"
        ).unwrap();
        c
    }
    fn note(c: &Connection, id: i64, tags: &str, text: &str) {
        c.execute("INSERT INTO notes VALUES(?1,?2,?3)", rusqlite::params![id,tags,text]).unwrap();
    }
    fn limits() -> Limits {
        let cap = Capacity { keys: 1024, slots: 8192 };
        Limits { deck: cap, tag: cap, text: cap, ..Limits::default() }
    }
    fn build(c: &Connection, limits: Limits) -> SearchIndex {
        let tx = c.unchecked_transaction().unwrap();
        SearchIndex::build_with_limits(&tx, Instant::now() + Duration::from_secs(30), limits).unwrap()
    }
    fn expect(index: &SearchIndex, tag: Option<&str>, text: Option<&str>, ids: &[i64]) {
        assert_eq!(index.candidates(None, tag, text), Some(ids));
    }

    #[test]
    fn original_decks_existing_notes_sorted_and_distinct() {
        let c = database();
        for id in [1,2,3] { note(&c, id, "", ""); }
        c.execute_batch(
            "INSERT INTO cards(id,nid,did,odid) VALUES(1,3,9,7),(2,1,7,0),(3,1,7,0),
             (4,2,8,7),(5,3,6,0),(6,99,7,0),(7,2,8,0);"
        ).unwrap();
        let index = build(&c, limits());
        assert_eq!(index.candidates(Some(7),None,None), Some([1,2,3].as_slice()));
        assert_eq!(index.candidates(Some(8),None,None), Some([2].as_slice()));
        assert_eq!(index.candidates(Some(9),None,None), Some([].as_slice()));
        assert_eq!(index.candidates(Some(6),Some("absent"),Some("absent")), Some([3].as_slice()));
    }

    #[test]
    fn unsuitable_sibling_indexes_disable_only_deck_postings() {
        for replacement in [
            "",
            "CREATE INDEX ix_cards_nid ON cards(did)",
            "CREATE INDEX ix_cards_nid ON cards(nid) WHERE nid=999",
            "CREATE INDEX ix_cards_nid ON notes(id)",
        ] {
            let c=database();
            note(&c,1,"chosen","literal");
            c.execute_batch("INSERT INTO cards(id,nid,did,odid) VALUES(1,1,7,0);
                DROP INDEX ix_cards_nid;").unwrap();
            c.execute_batch(replacement).unwrap();
            let index=build(&c,limits());
            assert_eq!(index.candidates(Some(999),None,None),None);
            assert_eq!(index.candidates(Some(7),Some("absent"),Some("absent")),None);
            expect(&index,Some("chosen"),None,&[1]);
            expect(&index,None,Some("literal"),&[1]);
        }
    }

    #[test]
    fn tags_use_legacy_separators_and_unicase_hash_without_rewriting() {
        let c = database();
        note(&c, 1, " Straße\u{3000}ς K a\tb ", "");
        note(&c, 2, "STRASSE", "");
        let index = build(&c, limits());
        for (tag, ids) in [("strasse", &[1,2][..]), ("Σ", &[1][..]), ("k", &[1][..]),
            ("a\tb", &[1][..]), ("a", &[][..]), ("Straß", &[][..])] {
            expect(&index, Some(tag), None, ids);
        }
        let stored: String = c.query_row("SELECT tags FROM notes WHERE id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(stored, " Straße\u{3000}ς K a\tb ");
    }

    #[test]
    fn unicode_simple_fold_agrees_with_literal_regex_not_naive_case_conversion() {
        let c = database();
        let cases = ["ς", "ſ", "K", "İ", "ı", "ß", "ẞ", "ﬃ", "µ", "σ", "s", "k"];
        for (i, text) in cases.iter().enumerate() { note(&c, i as i64 + 1, "", text); }
        let index = build(&c, limits());
        // Sigma/long-s/Kelvin/micro-sign need equivalence closure. Dotted and
        // dotless I, sharp-s expansion and ligature expansion must not acquire
        // the extra matches of lowercasing, uppercasing or full case folding.
        for query in ["Σ", "S", "K", "i", "İ", "ı", "ß", "SS", "FFI", "Μ"] {
            let regex = regex::RegexBuilder::new(&regex::escape(query))
                .case_insensitive(true).build().unwrap();
            let expected: Vec<i64> = cases.iter().enumerate().filter(|(_, s)| regex.is_match(s))
                .map(|(i, _)| i as i64 + 1).collect();
            // Single-scalar strings make each candidate list exact here.
            expect(&index, None, Some(query), &expected);
        }
    }

    #[test]
    fn short_multibyte_nul_and_literal_queries_preserve_field_boundaries() {
        let c = database();
        for (id, text) in [(1,"é"), (2,"é猫"), (3,"é猫🦀"), (4,"é\0猫"),
            (5,"ab\u{1f}cd"), (6,"a.*[x]"), (7,"\0"), (8,"\0\0"), (9,"\0\0\0")] {
            note(&c, id, "", text);
        }
        let index = build(&c, limits());
        for (query, ids) in [("É", &[1,2,3,4][..]), ("É猫", &[2,3][..]),
            ("É猫🦀", &[3][..]), ("é\0猫", &[4][..]), ("bc", &[][..]),
            (".*[", &[6][..]), ("\0", &[4,7,8,9][..]), ("\0\0", &[8,9][..]),
            ("\0\0\0", &[9][..]), ("missing", &[][..])] {
            expect(&index, None, Some(query), ids);
        }
        assert_eq!(index.candidates(None,None,Some("")), None);
        assert_eq!(index.candidates(None,None,None), None);
    }

    #[test]
    fn chooses_smallest_available_facet_but_deck_requires_membership() {
        let c = database();
        note(&c,1,"common", "rare");
        note(&c,2,"common special", "ordinary");
        note(&c,3,"common", "ordinary");
        let index = build(&c, limits());
        expect(&index, Some("common"), Some("rare"), &[1]);
        expect(&index, Some("special"), Some("ordinary"), &[2]);
        let mut capped = limits();
        capped.deck.keys = 0;
        c.execute_batch("INSERT INTO cards(id,nid,did,odid) VALUES(1,1,7,0)").unwrap();
        let index = build(&c, capped);
        assert_eq!(index.candidates(Some(7),Some("absent"),None), None);
        expect(&index, Some("special"), None, &[2]);
    }

    #[test]
    fn incomplete_facets_never_publish_prefixes_or_false_absence() {
        let c = database();
        note(&c,1,"one", "a");
        note(&c,2,"two", "b");
        c.execute_batch("INSERT INTO cards(id,nid,did,odid) VALUES(1,1,7,0),(2,2,8,0)").unwrap();
        let mut capped = limits();
        capped.tag.keys = 1;
        let index = build(&c, capped);
        assert_eq!(index.candidates(None,Some("one"),None), None);
        assert_eq!(index.candidates(None,Some("two"),None), None);
        expect(&index, Some("two"), Some("b"), &[2]);
        let mut capped = limits();
        capped.text.slots = 4; // First gram fits; second posting cannot grow.
        let index = build(&c, capped);
        assert_eq!(index.candidates(None,None,Some("a")), None);
        expect(&index, Some("two"), Some("b"), &[2]);
        let mut capped = limits();
        capped.card_rows = 1;
        capped.note_rows = 1;
        let index = build(&c, capped);
        assert_eq!(index.candidates(Some(7),None,None), None);
        assert_eq!(index.candidates(None,Some("one"),Some("a")), None);
    }

    #[test]
    fn posting_capacity_counts_growth_and_repeated_grams_do_not_duplicate_notes() {
        let c = database();
        for id in 1..=5 { note(&c,id,"repeat repeat", "aaaa\u{1f}aaaa"); }
        let index = build(&c, limits());
        expect(&index, Some("repeat"), None, &[1,2,3,4,5]);
        expect(&index, None, Some("aaa"), &[1,2,3,4,5]);
        let mut capped = limits();
        capped.tag.slots = 4; // Fifth distinct note needs capacity eight.
        let index = build(&c, capped);
        assert_eq!(index.candidates(None,Some("repeat"),None), None);
        expect(&index, None, Some("aaa"), &[1,2,3,4,5]);
    }

    #[test]
    fn oversized_or_malformed_values_discard_only_their_facet() {
        for malformed_tags in [false,true] {
            let c = database();
            note(&c,1,"ok", "short");
            if malformed_tags {
                c.execute("UPDATE notes SET tags=zeroblob(?1)", [(TAG_VALUE_BYTES + 1) as i64]).unwrap();
            } else {
                // NUL before the oversize suffix must not fool the SQL guard.
                c.execute("UPDATE notes SET flds=?1", [format!("\0{}", "x".repeat(MAX_RECORD_BYTES))]).unwrap();
            }
            let index = build(&c, limits());
            if malformed_tags {
                assert_eq!(index.candidates(None,Some("missing"),None), None);
                expect(&index, None, Some("short"), &[1]);
            } else {
                assert_eq!(index.candidates(None,None,Some("missing")), None);
                expect(&index, Some("ok"), None, &[1]);
            }
        }
        let c = database();
        note(&c,1,"ok", &"a".repeat(NOTE_SCALARS + 1));
        let index = build(&c, limits());
        assert_eq!(index.candidates(None,None,Some("a")), None);
        expect(&index, Some("ok"), None, &[1]);
    }

    #[test]
    fn deadline_and_input_budgets_invalidate_incomplete_facets() {
        let c = database();
        note(&c,1,"tag", "text");
        let tx = c.unchecked_transaction().unwrap();
        let index = SearchIndex::build_with_limits(&tx, Instant::now(), limits()).unwrap();
        assert_eq!(index.candidates(Some(1),None,None), None);
        assert_eq!(index.candidates(None,Some("tag"),Some("text")), None);
        drop(tx);
        for tags in [false,true] {
            let mut capped = limits();
            if tags { capped.tag_input_bytes = 0; } else { capped.text_input_bytes = 0; }
            let index = build(&c, capped);
            assert_eq!(index.candidates(None,
                if tags { Some("tag") } else { None },
                if tags { None } else { Some("text") }), None);
        }
        assert!(SearchIndex::build(&c, Instant::now()).is_err());
    }
}
