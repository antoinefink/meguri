//! Filter counts (`specs/search/filter-tokens.md` T12.4): how many rows of
//! a mode each value's built item finds, read with one grouped statement
//! per kind over one snapshot, never one search per value.
//!
//! [`CountPlan`] sorts the run texts to count into jobs, and the engine
//! runs each job's statement on its snapshot:
//!
//! - a deck item (`deck:` by name, or J3.5's `did:` list) joins the decks
//!   whose stored names its expression matches (`names-and-ids` N3.3,
//!   compared here as the expression does: anchored, each character by
//!   simple case folding, then the end or a level separator), and one read
//!   of every card's decks gives every deck item's count;
//! - a tag item in normal mode without wildcards (`tag:` by name) is the
//!   literal its expression matches (N2.2), and a note type item without
//!   wildcards (`note:` by name, or `mid:`) the note types whose names equal
//!   its own under `unicase` (N4.1): one read of every note's note type and
//!   tags gives every such item's count, with each note's cards in Cards;
//! - every other value shares one statement per row set (Q5) with one count
//!   column per value, each column the value's own plan predicate;
//! - a saved search or a whole query runs its run text as a count-only
//!   search.
//!
//! Each count equals the number of ids the item's own search returns in
//! the mode (`search/query-model` Q4, Q5): the same predicate over the same
//! rows, or the same expression over the same names.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use meguri_text::normalize::{nfc, remove_ascii_controls};

use crate::ast::{IdKind, NodeKind, Query, Term, TextMode};
use crate::context::{ContextReads, SearchContext};
use crate::fold::simple_fold;
use crate::name_index::NameIndex;
use crate::notetypes::SearchNotetypes;
use crate::parser::parse;
use crate::pattern::glob_literal;
use crate::plan::{Param, Params, Plan, Rows, SearchMode, compile_predicate};

use super::{FilterQuery, Token, Value, ValueToken, read_text};

/// The most value columns one count statement holds.
const COLUMNS: usize = 200;

/// The longest deck or tag literal counted by name: a longer one could make
/// its term's case-insensitive expression too large to compile (the search
/// then fails, `errors.md` E3), so it is counted by its own predicate, which
/// fails the same way.
const LONGEST_LITERAL: usize = 1000;

/// One thing a sidebar or suggestion row counts (T12.3 step 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CountItem {
    /// A value of T3, its built item alone.
    Value(Value),
    /// A user saved search, by its stored text: T4's run text of T7's
    /// reading of it (T12.4), so a saved `deck:*` counts what clicking it
    /// shows.
    Saved(String),
    /// A whole query's run text, such as the deck page's Suspended Cards
    /// row (the deck and Card State: Suspended, plan decision 18).
    Query(FilterQuery),
}

impl CountItem {
    /// The run text whose rows the item counts (T4); `None` when it has
    /// none (a value T3 does not offer).
    pub fn run_text(&self, names: &NameIndex) -> Option<String> {
        match self {
            CountItem::Value(value) => {
                value.item()?;
                FilterQuery::new(vec![Token::Value(ValueToken::new(vec![value.clone()], false))], "")
                    .run_text(names)
                    .ok()
            }
            CountItem::Saved(stored) => read_text(stored, names).run_text(names).ok(),
            CountItem::Query(query) => query.run_text(names).ok(),
        }
    }

    /// Whether the item runs alone as a count-only search (T12.4: saved
    /// searches, and whole queries) rather than in a statement it shares.
    pub fn runs_alone(&self) -> bool {
        !matches!(self, CountItem::Value(_))
    }
}

/// The tables a count reads, for the cache (T12.4): a change set that
/// touches one of them drops the count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tables(u8);

impl Tables {
    pub const CARDS: Tables = Tables(1);
    pub const NOTES: Tables = Tables(1 << 1);
    pub const DECKS: Tables = Tables(1 << 2);
    /// Note types with their fields and templates.
    pub const NOTETYPES: Tables = Tables(1 << 3);
    pub const DECK_CONFIG: Tables = Tables(1 << 4);
    pub const REVLOG: Tables = Tables(1 << 5);
    pub const ALL: Tables = Tables(0b11_1111);

    #[must_use]
    pub fn union(self, other: Tables) -> Tables {
        Tables(self.0 | other.0)
    }

    pub fn intersects(self, other: Tables) -> bool {
        self.0 & other.0 != 0
    }

    /// Q5's rows.
    pub fn of_rows(rows: Rows) -> Tables {
        match rows {
            Rows::Cards => Tables::CARDS,
            Rows::Notes => Tables::NOTES,
            Rows::CardsWithNotes => Tables::CARDS.union(Tables::NOTES),
        }
    }

    /// The tables `query`'s terms read besides its rows.
    pub fn of_terms(query: &Query) -> Tables {
        query
            .nodes()
            .iter()
            .filter_map(|node| match &node.kind {
                NodeKind::Term(term) => Some(term_tables(term)),
                _ => None,
            })
            .fold(Tables::default(), Tables::union)
    }
}

fn term_tables(term: &Term) -> Tables {
    use crate::ast::{Property, TemplateRef};
    match term {
        Term::WholeCollection | Term::Ids { .. } | Term::NotetypeId(_) => Tables::default(),
        Term::Deck(_) => Tables::DECKS,
        Term::Preset(_) => Tables::DECKS.union(Tables::DECK_CONFIG),
        Term::Notetype(_) | Term::Template(TemplateRef::Name(_)) => Tables::NOTETYPES,
        Term::Template(TemplateRef::Ordinal(_)) => Tables::CARDS,
        Term::Tag { .. } | Term::Edited(_) | Term::Duplicates { .. } => Tables::NOTES,
        Term::Text(_)
        | Term::Field { .. }
        | Term::Regex(_)
        | Term::NoCombining(_)
        | Term::StripCloze(_)
        | Term::Word(_) => Tables::NOTES.union(Tables::NOTETYPES),
        Term::Introduced(_) | Term::Rated { .. } | Term::Rescheduled(_) => Tables::CARDS.union(Tables::REVLOG),
        Term::Property {
            property: Property::Rated { .. } | Property::Rescheduled(_),
            ..
        } => Tables::CARDS.union(Tables::REVLOG),
        Term::State(_) | Term::Flag(_) | Term::Added(_) | Term::Property { .. } | Term::HasCustomData(_) => {
            Tables::CARDS
        }
    }
}

/// What a count depends on: the tables it read and the context values its
/// statement read (Q8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountReads {
    pub tables: Tables,
    pub context: ContextReads,
}

/// One run text to count, at its place in the caller's list.
#[derive(Clone, Debug)]
pub struct CountText<'a> {
    pub text: &'a str,
    pub alone: bool,
}

/// The jobs that count a list of run texts in one mode.
#[derive(Debug)]
pub struct CountPlan {
    pub mode: SearchMode,
    pub jobs: Vec<CountJob>,
    /// The texts no job counts (they did not parse or compile), by index:
    /// their count is unknown.
    pub uncounted: Vec<usize>,
}

/// One statement's work.
#[derive(Debug)]
pub enum CountJob {
    Decks(DeckCounts),
    /// Tag and note type items: one read of every note.
    Notes(NoteCounts),
    /// Notes mode, card terms only: each card's note and one column per
    /// text, gathered per note here.
    NoteColumns(NoteColumns),
    /// One count column per text, in `texts` order.
    Columns {
        sql: String,
        params: Vec<Param>,
        texts: Vec<usize>,
        reads: Vec<CountReads>,
    },
    /// A count-only search: one row, one column.
    Search {
        sql: String,
        params: Vec<Param>,
        text: usize,
        reads: CountReads,
    },
}

impl CountPlan {
    /// Sorts `texts` into jobs for `mode`, compiled against the snapshot's
    /// context and note types.
    pub fn new(mode: SearchMode, texts: &[CountText<'_>], ctx: &SearchContext, notetypes: &SearchNotetypes) -> CountPlan {
        let mut decks = DeckCounts::new(mode);
        let mut notes = NoteCounts::new(mode);
        let mut shared: Vec<(usize, Query)> = Vec::new();
        let mut jobs = Vec::new();
        // Count-only searches can be slow (text over every note): they come
        // last, so a newer call cancelling one keeps the grouped counts.
        let mut searches = Vec::new();
        let mut uncounted = Vec::new();
        for (index, item) in texts.iter().enumerate() {
            let Ok(query) = parse(item.text) else {
                uncounted.push(index);
                continue;
            };
            if let Some(key) = deck_key(&query) {
                decks.add(index, key);
            } else if let Some(literal) = tag_literal(&query) {
                notes.add_tag(index, &literal);
            } else if let Some(key) = notetype_key(&query) {
                notes.add_notetype(index, key);
            } else if item.alone {
                match search_job(index, &query, mode, ctx, notetypes) {
                    Some(job) => searches.push(job),
                    None => uncounted.push(index),
                }
            } else {
                shared.push((index, query));
            }
        }
        // One statement per row set, at most COLUMNS columns each.
        let mut by_rows: Vec<((Rows, bool), Vec<(usize, Query)>)> = Vec::new();
        for (index, query) in shared {
            let scope = crate::plan::query_scope(&query);
            // Notes mode with card terms only: the cards are read alone and
            // matched to the notes read for the other items.
            // A `nid:` term reads the note's id from the note in those rows.
            let reads_note_ids = query
                .nodes()
                .iter()
                .any(|node| matches!(node.kind, NodeKind::Term(Term::Ids { kind: IdKind::Notes, .. })));
            let key = (
                Rows::for_query(mode, scope),
                mode == SearchMode::Notes && scope == crate::ast::Scope::Card && !reads_note_ids,
            );
            match by_rows.iter_mut().find(|(kept, _)| *kept == key) {
                Some((_, list)) => list.push((index, query)),
                None => by_rows.push((key, vec![(index, query)])),
            }
        }
        for ((rows, cards_of_notes), list) in by_rows {
            for chunk in list.chunks(COLUMNS) {
                let (job, failed) = columns_job(mode, rows, cards_of_notes, chunk, ctx, notetypes);
                uncounted.extend(failed);
                jobs.extend(job);
            }
        }
        if !decks.is_empty() {
            jobs.insert(0, CountJob::Decks(decks));
        }
        if !notes.is_empty() {
            jobs.insert(0, CountJob::Notes(notes));
        }
        jobs.extend(searches);
        CountPlan { mode, jobs, uncounted }
    }
}

/// The count-only search of one text (T12.4).
fn search_job(
    index: usize,
    query: &Query,
    mode: SearchMode,
    ctx: &SearchContext,
    notetypes: &SearchNotetypes,
) -> Option<CountJob> {
    let plan = Plan::build(query, mode, ctx, notetypes).ok()?;
    Some(CountJob::Search {
        sql: format!("SELECT count(*) FROM ({})", plan.sql()),
        params: plan.params().to_vec(),
        text: index,
        reads: CountReads {
            tables: Tables::of_rows(plan.rows()).union(Tables::of_terms(query)),
            context: plan.reads(),
        },
    })
}

/// One statement counting each query of `chunk` over `rows` (all of them
/// test those rows in `mode`); the indexes whose query did not compile.
fn columns_job(
    mode: SearchMode,
    rows: Rows,
    cards_of_notes: bool,
    chunk: &[(usize, Query)],
    ctx: &SearchContext,
    notetypes: &SearchNotetypes,
) -> (Option<CountJob>, Vec<usize>) {
    let mut params = Params::default();
    let mut predicates = Vec::new();
    let mut texts = Vec::new();
    let mut reads = Vec::new();
    let mut failed = Vec::new();
    for (index, query) in chunk {
        match compile_predicate(query, mode, ctx, notetypes, &mut params) {
            Ok(predicate) => {
                debug_assert_eq!(predicate.rows, rows);
                predicates.push(predicate.sql);
                texts.push(*index);
                reads.push(CountReads {
                    tables: Tables::of_rows(rows).union(Tables::of_terms(query)),
                    context: predicate.reads,
                });
            }
            Err(_) => failed.push(*index),
        }
    }
    if predicates.is_empty() {
        return (None, failed);
    }
    if cards_of_notes {
        let columns: Vec<String> = predicates
            .iter()
            .map(|p| format!("CASE WHEN ({p}) THEN 1 ELSE 0 END"))
            .collect();
        return (
            Some(CountJob::NoteColumns(NoteColumns {
                sql: format!("SELECT c.nid, {} FROM cards c", columns.join(", ")),
                params: params.into_vec(),
                texts,
                reads,
                bits: Vec::new(),
            })),
            failed,
        );
    }
    let sql = match (mode, rows) {
        (SearchMode::Notes, Rows::Cards | Rows::CardsWithNotes) => {
            // A note counts once when any of its cards makes the item true
            // (Q4): the most of each column over the note's pairs.
            let inner: Vec<String> = predicates
                .iter()
                .enumerate()
                .map(|(k, p)| format!("max(CASE WHEN ({p}) THEN 1 ELSE 0 END) AS m{k}"))
                .collect();
            let outer: Vec<String> = (0..predicates.len())
                .map(|k| format!("count(CASE WHEN m{k} = 1 THEN 1 END)"))
                .collect();
            format!(
                "SELECT {} FROM (SELECT {} FROM cards c JOIN notes n ON n.id = c.nid GROUP BY c.nid)",
                outer.join(", "),
                inner.join(", ")
            )
        }
        _ => {
            let from = match rows {
                Rows::Cards => "cards c",
                Rows::Notes => "notes n",
                Rows::CardsWithNotes => "cards c JOIN notes n ON n.id = c.nid",
            };
            let columns: Vec<String> = predicates
                .iter()
                .map(|p| format!("count(CASE WHEN ({p}) THEN 1 END)"))
                .collect();
            format!("SELECT {} FROM {from}", columns.join(", "))
        }
    };
    (
        Some(CountJob::Columns {
            sql,
            params: params.into_vec(),
            texts,
            reads,
        }),
        failed,
    )
}

/// The query's only item, when it is one term.
fn single_term(query: &Query) -> Option<&Term> {
    match &query.node(query.root()).kind {
        NodeKind::Term(term) => Some(term),
        _ => None,
    }
}

/// Which decks a deck item names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DeckKey {
    /// N3.3's expression without wildcards: the stored name it matches,
    /// levels joined by U+001F, folded.
    Name(String),
    /// N3.5's list: these ids, whether or not a deck has them.
    Ids(Vec<i64>),
}

/// A `deck:` term with no wildcard and no special value, or a `did:` list.
fn deck_key(query: &Query) -> Option<DeckKey> {
    match single_term(query)? {
        Term::Deck(value) => {
            let value = nfc(value);
            if matches!(value.as_ref(), "*" | "filtered" | "current") {
                return None;
            }
            let levels = deck_levels_literal(&value)?;
            (levels.chars().count() <= LONGEST_LITERAL).then(|| DeckKey::Name(simple_fold(&levels)))
        }
        // A digit other than ASCII fails the search's plan (E3): counted as
        // the search is, by its own job.
        Term::Ids {
            kind: IdKind::Decks,
            list,
        } if list.chars().all(|c| c == ',' || c.is_ascii_digit()) => Some(DeckKey::Ids(
            list.split(',').filter_map(|run| run.parse::<i64>().ok()).collect(),
        )),
        _ => None,
    }
}

/// `plan::names::deck_levels` for a value without wildcards: the text its
/// expression matches between `^` and `($|\x1f)`. The expression's own
/// characters (escapes included) and the text's are split, normalized and
/// trimmed alike: a level separator, White_Space and `:` are never escaped
/// there.
fn deck_levels_literal(value: &str) -> Option<String> {
    let literal = glob_literal(value)?;
    let levels: Vec<String> = literal
        .split("::")
        .map(|piece| {
            let piece = nfc(piece);
            let piece = remove_ascii_controls(&piece);
            let piece = piece.trim_matches(|c: char| c.is_whitespace() || c == ':');
            if piece.is_empty() { "blank".to_owned() } else { piece.to_owned() }
        })
        .collect();
    Some(levels.join("\u{1f}"))
}

/// A `tag:` term in normal mode whose expression (N2.2 step 4) holds no
/// wildcard: the literal it matches after a space and before `::` or a
/// space, folded.
fn tag_literal(query: &Query) -> Option<String> {
    let Term::Tag {
        mode: TextMode::Normal,
        text,
    } = single_term(query)?
    else {
        return None;
    };
    let value = nfc(text);
    if matches!(value.as_ref(), "none" | "*") || value.contains(' ') {
        return None;
    }
    let literal = glob_literal(&value)?;
    (!literal.is_empty() && literal.chars().count() <= LONGEST_LITERAL).then(|| simple_fold(&literal))
}

/// Which note types a note type item names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NotetypeKey {
    /// N4.1 without wildcards: names equal to this one under unicase.
    Name(String),
    /// `mid:`.
    Id(i64),
}

/// A `note:` term with no wildcard (N4.1: unicase equality with its exact
/// text, read in the statement), or a `mid:` term.
fn notetype_key(query: &Query) -> Option<NotetypeKey> {
    match single_term(query)? {
        Term::Notetype(value) => {
            let value = nfc(value);
            if value.is_empty() || crate::pattern::has_wildcards(&value) {
                return None;
            }
            Some(NotetypeKey::Name(crate::pattern::exact_text(&value).into_owned()))
        }
        Term::NotetypeId(id) => Some(NotetypeKey::Id(id.0)),
        _ => None,
    }
}

/// A hasher for row ids: one multiply, as ids are already spread out.
#[derive(Default)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*byte)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }
    fn write_i64(&mut self, value: i64) {
        self.0 = (value as u64 ^ self.0).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type IdMap<V> = HashMap<i64, V, BuildHasherDefault<IdHasher>>;

/// The notes that exist (Q5: Notes mode reads a card only with its note),
/// each with a dense index for per-note arrays.
#[derive(Debug, Default)]
pub struct NoteIndex {
    ids: IdMap<u32>,
}

impl NoteIndex {
    pub fn insert(&mut self, nid: i64) {
        let next = u32::try_from(self.ids.len()).unwrap_or(u32::MAX);
        self.ids.entry(nid).or_insert(next);
    }

    pub fn get(&self, nid: i64) -> Option<u32> {
        self.ids.get(&nid).copied()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// The deck items of a plan: the cards grouped by `did` and `odid` (Cards),
/// or every card-and-note pair's note and decks (Notes).
#[derive(Debug)]
pub struct DeckCounts {
    mode: SearchMode,
    names: HashMap<String, Vec<usize>>,
    ids: HashMap<i64, Vec<usize>>,
    texts: Vec<usize>,
    /// After [`DeckCounts::decks`]: the texts each deck id is in.
    members: HashMap<i64, Vec<usize>>,
    /// Each distinct (`did`, `odid`) pair read, with its cards.
    pairs: HashMap<(i64, i64), (u32, u64)>,
    /// Notes mode: each card's note (its dense index) and pair.
    cards: Vec<(u32, u32)>,
}

impl DeckCounts {
    fn new(mode: SearchMode) -> DeckCounts {
        DeckCounts {
            mode,
            names: HashMap::new(),
            ids: HashMap::new(),
            texts: Vec::new(),
            members: HashMap::new(),
            pairs: HashMap::new(),
            cards: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    fn add(&mut self, index: usize, key: DeckKey) {
        self.texts.push(index);
        match key {
            DeckKey::Name(name) => self.names.entry(name).or_default().push(index),
            DeckKey::Ids(ids) => {
                for id in ids {
                    let texts = self.ids.entry(id).or_default();
                    if !texts.contains(&index) {
                        texts.push(index);
                    }
                }
            }
        }
    }

    pub fn texts(&self) -> &[usize] {
        &self.texts
    }

    /// The decks of the snapshot, each its id and stored name (none when
    /// it is not text, which no expression matches): which items each deck
    /// is in.
    pub fn decks<'a>(&mut self, decks: impl IntoIterator<Item = (i64, Option<&'a str>)>) {
        let mut members: HashMap<i64, Vec<usize>> = self.ids.clone();
        for (id, name) in decks {
            let Some(name) = name else { continue };
            let folded = simple_fold(name);
            let mut ends: Vec<usize> = folded.match_indices('\u{1f}').map(|(at, _)| at).collect();
            ends.push(folded.len());
            for end in ends {
                if let Some(texts) = self.names.get(&folded[..end]) {
                    let entry = members.entry(id).or_default();
                    for text in texts {
                        if !entry.contains(text) {
                            entry.push(*text);
                        }
                    }
                }
            }
        }
        self.members = members;
    }

    /// The statement: every card's decks (Cards), or every card-and-note
    /// pair's note and decks (Notes). Grouped here, not by SQLite's sorter.
    pub fn sql(&self) -> &'static str {
        match self.mode {
            SearchMode::Cards => "SELECT c.did, c.odid FROM cards c",
            // Matched to the notes that exist as it is read.
            SearchMode::Notes => "SELECT c.nid, c.did, c.odid FROM cards c",
        }
    }

    /// One row of [`DeckCounts::sql`]; in Notes mode `notes` are the notes
    /// that exist (Q5: a card counts only with its note).
    pub fn row(&mut self, row: &[i64], notes: &NoteIndex) {
        let (pair, note) = match self.mode {
            SearchMode::Cards => ((row[0], row[1]), None),
            SearchMode::Notes => match notes.get(row[0]) {
                Some(note) => ((row[1], row[2]), Some(note)),
                None => return,
            },
        };
        let next = u32::try_from(self.pairs.len()).unwrap_or(u32::MAX);
        let entry = self.pairs.entry(pair).or_insert((next, 0));
        entry.1 += 1;
        if let Some(note) = note {
            self.cards.push((note, entry.0));
        }
    }

    /// Every item's count.
    pub fn finish(mut self) -> Vec<(usize, u64)> {
        // N3.3: a card is in an item's decks through `did`, or through
        // `odid` when it is in a filtered deck.
        let mut texts_of: Vec<Vec<usize>> = vec![Vec::new(); self.pairs.len()];
        let mut cards_of: Vec<u64> = vec![0; self.pairs.len()];
        for ((did, odid), (id, cards)) in &self.pairs {
            let found = &mut texts_of[*id as usize];
            for deck in [Some(*did), (*odid != 0).then_some(*odid)].into_iter().flatten() {
                for text in self.members.get(&deck).into_iter().flatten() {
                    if !found.contains(text) {
                        found.push(*text);
                    }
                }
            }
            cards_of[*id as usize] = *cards;
        }
        let mut counts: HashMap<usize, u64> = HashMap::new();
        match self.mode {
            SearchMode::Cards => {
                for (found, cards) in texts_of.iter().zip(&cards_of) {
                    for text in found {
                        *counts.entry(*text).or_default() += cards;
                    }
                }
            }
            SearchMode::Notes => {
                // A note counts once per item, whichever of its cards is in
                // it: the cards by note, then each item once per note.
                self.cards.sort_unstable();
                self.cards.dedup();
                let mut stamp: HashMap<usize, u32> = HashMap::new();
                for (note, pair) in &self.cards {
                    for text in &texts_of[*pair as usize] {
                        if stamp.insert(*text, *note) != Some(*note) {
                            *counts.entry(*text).or_default() += 1;
                        }
                    }
                }
            }
        }
        self.texts
            .iter()
            .map(|text| (*text, counts.get(text).copied().unwrap_or(0)))
            .collect()
    }
}

/// The tag and note type items of a plan: one read of every note's note
/// type and tags, with each note's cards in Cards (Q5: every card whose note
/// exists, with it).
#[derive(Debug)]
pub struct NoteCounts {
    mode: SearchMode,
    literals: HashMap<String, Vec<usize>>,
    names: Vec<(String, usize)>,
    ids: Vec<(i64, usize)>,
    texts: Vec<usize>,
    counts: HashMap<usize, u64>,
    /// After [`NoteCounts::notetypes`]: the items each note type id is in.
    members: HashMap<i64, Vec<usize>>,
    /// Cards: each note's cards.
    cards: IdMap<u64>,
    /// Reused for each tag's folding.
    buffer: String,
    found: Vec<usize>,
    /// The items each tag as stored matches, with a space after it and
    /// without: the same tags recur on many notes.
    pieces: HashMap<String, usize>,
    matched: Vec<[Vec<usize>; 2]>,
    /// Every note read.
    read: NoteIndex,
}

impl NoteCounts {
    fn new(mode: SearchMode) -> NoteCounts {
        NoteCounts {
            mode,
            literals: HashMap::new(),
            names: Vec::new(),
            ids: Vec::new(),
            texts: Vec::new(),
            counts: HashMap::new(),
            members: HashMap::new(),
            cards: IdMap::default(),
            buffer: String::new(),
            found: Vec::new(),
            pieces: HashMap::new(),
            matched: Vec::new(),
            read: NoteIndex::default(),
        }
    }

    fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    fn add_tag(&mut self, index: usize, literal: &str) {
        self.texts.push(index);
        self.literals.entry(literal.to_owned()).or_default().push(index);
    }

    fn add_notetype(&mut self, index: usize, key: NotetypeKey) {
        self.texts.push(index);
        match key {
            NotetypeKey::Name(name) => self.names.push((name, index)),
            NotetypeKey::Id(id) => self.ids.push((id, index)),
        }
    }

    pub fn texts(&self) -> &[usize] {
        &self.texts
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    /// The note types of the snapshot, each its id and name (none when not
    /// text): which items each note type id is in. Names compare as the
    /// `unicase` collation does.
    pub fn notetypes<'a>(&mut self, notetypes: impl IntoIterator<Item = (i64, Option<&'a str>)>) {
        let mut members: HashMap<i64, Vec<usize>> = HashMap::new();
        for (id, text) in &self.ids {
            members.entry(*id).or_default().push(*text);
        }
        if !self.names.is_empty() {
            for (id, name) in notetypes {
                let Some(name) = name else { continue };
                for (wanted, text) in &self.names {
                    if meguri_text::fold::compare_folded(name, wanted).is_eq() {
                        let entry = members.entry(id).or_default();
                        if !entry.contains(text) {
                            entry.push(*text);
                        }
                    }
                }
            }
        }
        self.members = members;
    }

    /// Cards: the statement giving each note id's cards, read before the
    /// notes.
    pub fn cards_sql(&self) -> Option<&'static str> {
        match self.mode {
            SearchMode::Cards => Some("SELECT c.nid, count(*) FROM cards c GROUP BY c.nid"),
            SearchMode::Notes => None,
        }
    }

    /// One row of [`NoteCounts::cards_sql`].
    pub fn cards_row(&mut self, row: &[i64]) {
        self.cards.insert(row[0], u64::try_from(row[1]).unwrap_or(0));
    }

    /// One note: its note type, and N2.2 step 4's expression `.* L(::| ).*`
    /// on its tags for each tag item's literal L (L after a space, then
    /// `::` or a space).
    pub fn note(&mut self, nid: i64, mid: Option<i64>, tags: Option<&str>) {
        self.read.insert(nid);
        let weight = match self.mode {
            SearchMode::Notes => 1,
            SearchMode::Cards => match self.cards.get(&nid) {
                Some(cards) => *cards,
                None => return,
            },
        };
        let mut found = std::mem::take(&mut self.found);
        found.clear();
        if let Some(texts) = mid.and_then(|mid| self.members.get(&mid)) {
            found.extend(texts.iter().copied());
        }
        if let Some(tags) = tags
            && !self.literals.is_empty()
        {
            let mut pieces = tags.split(' ').peekable();
            // The first piece has no space before it.
            pieces.next();
            while let Some(piece) = pieces.next() {
                let closed = usize::from(pieces.peek().is_some());
                if piece.is_empty() {
                    continue;
                }
                let at = match self.pieces.get(piece) {
                    Some(at) => *at,
                    None => {
                        let matched = [self.matches(piece, false), self.matches(piece, true)];
                        self.matched.push(matched);
                        self.pieces.insert(piece.to_owned(), self.matched.len() - 1);
                        self.matched.len() - 1
                    }
                };
                for text in &self.matched[at][closed] {
                    if !found.contains(text) {
                        found.push(*text);
                    }
                }
            }
        }
        for text in &found {
            *self.counts.entry(*text).or_default() += weight;
        }
        self.found = found;
    }

    /// The notes read so far: every note, once the read is done.
    pub fn take_notes(&mut self) -> NoteIndex {
        std::mem::take(&mut self.read)
    }

    /// The tag items one tag as stored matches: each literal that it
    /// equals before a `::` it holds (overlapping), or as a whole when a
    /// space follows it (`closed`).
    fn matches(&mut self, piece: &str, closed: bool) -> Vec<usize> {
        crate::fold::fold_into(piece, &mut self.buffer);
        let folded = self.buffer.as_str();
        let bytes = folded.as_bytes();
        let ends = (1..bytes.len().saturating_sub(1))
            .filter(|&at| bytes[at] == b':' && bytes[at + 1] == b':')
            .chain(closed.then_some(folded.len()));
        let mut found = Vec::new();
        for end in ends {
            for text in self.literals.get(&folded[..end]).into_iter().flatten() {
                if !found.contains(text) {
                    found.push(*text);
                }
            }
        }
        found
    }

    /// Every item's count.
    pub fn finish(self) -> Vec<(usize, u64)> {
        self.texts
            .iter()
            .map(|text| (*text, self.counts.get(text).copied().unwrap_or(0)))
            .collect()
    }
}

/// Notes mode, card terms only (Q4: a note counts when one of its cards
/// makes the item true): each card's note and one 0-or-1 column per text,
/// read from the cards alone; the notes that exist come from the caller.
#[derive(Debug)]
pub struct NoteColumns {
    pub sql: String,
    pub params: Vec<Param>,
    texts: Vec<usize>,
    reads: Vec<CountReads>,
    /// Per note (its dense index): the columns any of its cards set, as
    /// bits, `words` a note.
    bits: Vec<u64>,
}

impl NoteColumns {
    pub fn texts(&self) -> &[usize] {
        &self.texts
    }

    /// One row of [`NoteColumns::sql`]: the card's note, then the columns;
    /// a card whose note is not in `notes` counts for nothing.
    pub fn row(&mut self, row: &[i64], notes: &NoteIndex) {
        let words = self.texts.len().div_ceil(64);
        if self.bits.is_empty() {
            self.bits = vec![0; notes.len() * words];
        }
        let Some(note) = notes.get(row[0]) else { return };
        let bits = &mut self.bits[note as usize * words..(note as usize + 1) * words];
        for (k, value) in row[1..].iter().enumerate() {
            if *value == 1 {
                bits[k / 64] |= 1 << (k % 64);
            }
        }
    }

    /// Every item's count.
    pub fn finish(self) -> Vec<(usize, u64)> {
        let words = self.texts.len().div_ceil(64);
        let mut counts = vec![0u64; self.texts.len()];
        for bits in self.bits.chunks(words.max(1)) {
            for (k, count) in counts.iter_mut().enumerate() {
                if bits[k / 64] & (1 << (k % 64)) != 0 {
                    *count += 1;
                }
            }
        }
        self.texts.iter().copied().zip(counts).collect()
    }
}

impl CountJob {
    /// Whether the job needs the set of notes that exist (Notes mode).
    pub fn needs_notes(&self) -> bool {
        match self {
            CountJob::Decks(decks) => decks.mode == SearchMode::Notes,
            CountJob::NoteColumns(_) => true,
            _ => false,
        }
    }

    /// What each count of the job depends on, by text index.
    pub fn reads(&self) -> Vec<(usize, CountReads)> {
        match self {
            CountJob::Decks(decks) => {
                let tables = Tables::of_rows(match decks.mode {
                    SearchMode::Cards => Rows::Cards,
                    SearchMode::Notes => Rows::CardsWithNotes,
                })
                .union(Tables::DECKS);
                decks
                    .texts
                    .iter()
                    .map(|text| {
                        (
                            *text,
                            CountReads {
                                tables,
                                context: ContextReads::NONE,
                            },
                        )
                    })
                    .collect()
            }
            CountJob::Notes(notes) => {
                let rows = Tables::of_rows(match notes.mode {
                    SearchMode::Cards => Rows::CardsWithNotes,
                    SearchMode::Notes => Rows::Notes,
                });
                // A note type by name reads the note types' names too.
                let by_name: HashSet<usize> = notes.names.iter().map(|(_, text)| *text).collect();
                notes
                    .texts
                    .iter()
                    .map(|text| {
                        let tables = if by_name.contains(text) { rows.union(Tables::NOTETYPES) } else { rows };
                        (
                            *text,
                            CountReads {
                                tables,
                                context: ContextReads::NONE,
                            },
                        )
                    })
                    .collect()
            }
            CountJob::Columns { texts, reads, .. } => texts.iter().copied().zip(reads.iter().copied()).collect(),
            CountJob::NoteColumns(columns) => columns.texts.iter().copied().zip(columns.reads.iter().copied()).collect(),
            CountJob::Search { text, reads, .. } => vec![(*text, *reads)],
        }
    }
}
