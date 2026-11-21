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
//!   of the cards grouped by deck gives every deck item's count;
//! - a tag item in normal mode without wildcards (`tag:` by name) is the
//!   literal its expression matches (N2.2): one read of every note's tags
//!   gives every tag item's count, with the cards of each note in Cards;
//! - every other value shares one statement per row set (Q5) with one count
//!   column per value, each column the value's own plan predicate;
//! - a saved search or a whole query runs its run text as a count-only
//!   search.
//!
//! Each count equals the number of ids the item's own search returns in
//! the mode (`search/query-model` Q4, Q5): the same predicate over the same
//! rows, or the same expression over the same names.

use std::collections::HashMap;

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
    Tags(TagCounts),
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
        let mut tags = TagCounts::new(mode);
        let mut shared: Vec<(usize, Query)> = Vec::new();
        let mut jobs = Vec::new();
        let mut uncounted = Vec::new();
        for (index, item) in texts.iter().enumerate() {
            let Ok(query) = parse(item.text) else {
                uncounted.push(index);
                continue;
            };
            if let Some(key) = deck_key(&query) {
                decks.add(index, key);
            } else if let Some(literal) = tag_literal(&query) {
                tags.add(index, &literal);
            } else if item.alone {
                match search_job(index, &query, mode, ctx, notetypes) {
                    Some(job) => jobs.push(job),
                    None => uncounted.push(index),
                }
            } else {
                shared.push((index, query));
            }
        }
        // One statement per row set, at most COLUMNS columns each.
        let mut by_rows: Vec<(Rows, Vec<(usize, Query)>)> = Vec::new();
        for (index, query) in shared {
            let rows = Rows::for_query(mode, crate::plan::query_scope(&query));
            match by_rows.iter_mut().find(|(kept, _)| *kept == rows) {
                Some((_, list)) => list.push((index, query)),
                None => by_rows.push((rows, vec![(index, query)])),
            }
        }
        for (rows, list) in by_rows {
            for chunk in list.chunks(COLUMNS) {
                let (job, failed) = columns_job(mode, rows, chunk, ctx, notetypes);
                uncounted.extend(failed);
                jobs.extend(job);
            }
        }
        if !decks.is_empty() {
            jobs.insert(0, CountJob::Decks(decks));
        }
        if !tags.is_empty() {
            jobs.insert(0, CountJob::Tags(tags));
        }
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
            Some(DeckKey::Name(simple_fold(&deck_levels_literal(&value)?)))
        }
        Term::Ids {
            kind: IdKind::Decks,
            list,
        } => Some(DeckKey::Ids(
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
    (!literal.is_empty()).then(|| simple_fold(&literal))
}

/// The deck items of a plan: the cards grouped by `did` and `odid` (Cards),
/// or every card-and-note pair's note and decks (Notes).
#[derive(Debug)]
pub struct DeckCounts {
    mode: SearchMode,
    names: HashMap<String, Vec<usize>>,
    ids: HashMap<i64, Vec<usize>>,
    texts: Vec<usize>,
    /// Per text index: its count so far.
    counts: HashMap<usize, u64>,
    /// After [`DeckCounts::decks`]: the texts each deck id is in.
    members: HashMap<i64, Vec<usize>>,
    /// Notes mode: the note being read and the texts it is in so far.
    note: Option<i64>,
    seen: Vec<usize>,
}

impl DeckCounts {
    fn new(mode: SearchMode) -> DeckCounts {
        DeckCounts {
            mode,
            names: HashMap::new(),
            ids: HashMap::new(),
            texts: Vec::new(),
            counts: HashMap::new(),
            members: HashMap::new(),
            note: None,
            seen: Vec::new(),
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

    /// The decks of the snapshot, each its id and stored name: which items
    /// each deck is in.
    pub fn decks<'a>(&mut self, decks: impl IntoIterator<Item = (i64, &'a str)>) {
        let mut members: HashMap<i64, Vec<usize>> = self.ids.clone();
        for (id, name) in decks {
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

    /// The grouped statement.
    pub fn sql(&self) -> &'static str {
        match self.mode {
            SearchMode::Cards => "SELECT c.did, c.odid, count(*) FROM cards c GROUP BY c.did, c.odid",
            SearchMode::Notes => {
                "SELECT c.nid, c.did, c.odid FROM cards c JOIN notes n ON n.id = c.nid \
                 GROUP BY c.nid, c.did, c.odid ORDER BY c.nid"
            }
        }
    }

    /// One row of [`DeckCounts::sql`].
    pub fn row(&mut self, row: &[i64]) {
        match self.mode {
            SearchMode::Cards => {
                let (did, odid, count) = (row[0], row[1], u64::try_from(row[2]).unwrap_or(0));
                // N3.3: through `did`, or through `odid` in a filtered deck.
                let mut found = self.members.get(&did).cloned().unwrap_or_default();
                if odid != 0 {
                    for text in self.members.get(&odid).into_iter().flatten() {
                        if !found.contains(text) {
                            found.push(*text);
                        }
                    }
                }
                for text in found {
                    *self.counts.entry(text).or_default() += count;
                }
            }
            SearchMode::Notes => {
                let (nid, did, odid) = (row[0], row[1], row[2]);
                if self.note != Some(nid) {
                    self.end_note();
                    self.note = Some(nid);
                }
                for deck in [Some(did), (odid != 0).then_some(odid)].into_iter().flatten() {
                    for text in self.members.get(&deck).into_iter().flatten() {
                        if !self.seen.contains(text) {
                            self.seen.push(*text);
                        }
                    }
                }
            }
        }
    }

    fn end_note(&mut self) {
        for text in self.seen.drain(..) {
            *self.counts.entry(text).or_default() += 1;
        }
    }

    /// Every item's count.
    pub fn finish(mut self) -> Vec<(usize, u64)> {
        self.end_note();
        self.texts
            .iter()
            .map(|text| (*text, self.counts.get(text).copied().unwrap_or(0)))
            .collect()
    }
}

/// The tag items of a plan: every note's tags (and in Cards its cards).
#[derive(Debug)]
pub struct TagCounts {
    mode: SearchMode,
    literals: HashMap<String, Vec<usize>>,
    texts: Vec<usize>,
    counts: HashMap<usize, u64>,
    /// Cards: each note's cards.
    cards: HashMap<i64, u64>,
}

impl TagCounts {
    fn new(mode: SearchMode) -> TagCounts {
        TagCounts {
            mode,
            literals: HashMap::new(),
            texts: Vec::new(),
            counts: HashMap::new(),
            cards: HashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    fn add(&mut self, index: usize, literal: &str) {
        self.texts.push(index);
        self.literals.entry(literal.to_owned()).or_default().push(index);
    }

    pub fn texts(&self) -> &[usize] {
        &self.texts
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    /// Cards (Q5: every card whose note exists, with it): the statement
    /// giving each note id's cards, read before the notes.
    pub fn cards_sql(&self) -> Option<&'static str> {
        match self.mode {
            SearchMode::Cards => Some("SELECT c.nid, count(*) FROM cards c GROUP BY c.nid"),
            SearchMode::Notes => None,
        }
    }

    /// One row of [`TagCounts::cards_sql`].
    pub fn cards_row(&mut self, row: &[i64]) {
        self.cards.insert(row[0], u64::try_from(row[1]).unwrap_or(0));
    }

    /// One note: N2.2 step 4's expression `.* L(::| ).*` on its tags, for
    /// each item's literal L: L after a space, then `::` or a space.
    pub fn note(&mut self, nid: i64, tags: Option<&str>) {
        let Some(tags) = tags else { return };
        let weight = match self.mode {
            SearchMode::Notes => 1,
            SearchMode::Cards => match self.cards.get(&nid) {
                Some(cards) => *cards,
                None => return,
            },
        };
        let mut found: Vec<usize> = Vec::new();
        let mut pieces = tags.split(' ').peekable();
        // The first piece has no space before it.
        pieces.next();
        while let Some(piece) = pieces.next() {
            let closed = pieces.peek().is_some();
            if piece.is_empty() {
                continue;
            }
            let folded = simple_fold(piece);
            // Every end of a run that `::` follows (overlapping), and the
            // whole piece when a space follows it.
            let mut ends: Vec<usize> = Vec::new();
            let bytes = folded.as_bytes();
            for at in 0..bytes.len().saturating_sub(1) {
                if bytes[at] == b':' && bytes[at + 1] == b':' && at > 0 {
                    ends.push(at);
                }
            }
            if closed {
                ends.push(folded.len());
            }
            for end in ends {
                if let Some(texts) = self.literals.get(&folded[..end]) {
                    for text in texts {
                        if !found.contains(text) {
                            found.push(*text);
                        }
                    }
                }
            }
        }
        for text in found {
            *self.counts.entry(text).or_default() += weight;
        }
    }

    /// Every item's count.
    pub fn finish(self) -> Vec<(usize, u64)> {
        self.texts
            .iter()
            .map(|text| (*text, self.counts.get(text).copied().unwrap_or(0)))
            .collect()
    }
}

impl CountJob {
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
            CountJob::Tags(tags) => {
                let tables = Tables::of_rows(match tags.mode {
                    SearchMode::Cards => Rows::CardsWithNotes,
                    SearchMode::Notes => Rows::Notes,
                });
                tags.texts
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
            CountJob::Columns { texts, reads, .. } => texts.iter().copied().zip(reads.iter().copied()).collect(),
            CountJob::Search { text, reads, .. } => vec![(*text, *reads)],
        }
    }
}
