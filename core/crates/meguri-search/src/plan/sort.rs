//! Sorted searches (`specs/browser/sorting.md` S1 to S10): the statement
//! that lists a search's ids, or a given id list's, in a column's order.
//!
//! Every key is computed inside the one statement: ranks are window
//! functions over the deck, note type and template rows (S3), per-note
//! aggregates are correlated subqueries over the note's cards (S5), and
//! nothing is created in the collection (S10, R2016). A final id key in the
//! sort's direction makes the order total (S8), so reversing an ascending
//! result equals the descending one.
//!
//! Field columns `fld:<name>` (`browser/workspace` V4.2) are recognised here
//! and sort in both modes; their key is U261's ([`field_keys`]).

use crate::config::{Column, Refusal, SortDirection};
use crate::context::{ContextReads, ContextValue, SearchContext};

use super::{Param, Plan, Rows, SearchMode, props};

/// The prefix of a field column key (`browser/workspace` V3).
pub const FIELD_COLUMN_PREFIX: &str = "fld:";

/// The column a sort request names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SortColumn {
    /// One of the 19 columns, sortable in the sort's mode.
    Column(Column),
    /// A field column `fld:<name>`: sortable in both modes whatever the name
    /// (S1, changed 2026-11-20).
    Field(String),
}

impl SortColumn {
    /// S1: the column `key` names in `mode`. A key outside the 19 that is
    /// not a field column key, or a column that does not sort in the mode,
    /// is `unsortable_column` (K5.2's kind).
    pub fn resolve(key: &str, mode: SearchMode) -> Result<SortColumn, Refusal> {
        if let Some(name) = key.strip_prefix(FIELD_COLUMN_PREFIX) {
            return Ok(SortColumn::Field(name.to_owned()));
        }
        match Column::from_key(key) {
            Some(column) if column.sortable(mode) => Ok(SortColumn::Column(column)),
            _ => Err(Refusal::UnsortableColumn),
        }
    }
}

/// A sort request: a mode, a column and a direction (S1). The core never
/// chooses the direction.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sort {
    mode: SearchMode,
    column: SortColumn,
    direction: SortDirection,
}

impl Sort {
    /// Refuses an unsortable key before anything runs (S1).
    pub fn new(mode: SearchMode, key: &str, direction: SortDirection) -> Result<Sort, Refusal> {
        Ok(Sort {
            mode,
            column: SortColumn::resolve(key, mode)?,
            direction,
        })
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    pub fn column(&self) -> &SortColumn {
        &self.column
    }

    pub fn direction(&self) -> SortDirection {
        self.direction
    }

    /// The same sort in the other direction (R1142).
    #[must_use]
    pub fn reversed(&self) -> Sort {
        Sort {
            direction: self.direction.reversed(),
            ..self.clone()
        }
    }
}

/// Why no sorted statement was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SortError {
    /// The plan was built for the other mode.
    ModeMismatch,
    /// The field column sort key (`browser/workspace` V4.2) is U261's and
    /// has not landed; the key is accepted, not refused (S1).
    FieldColumnNotYetSupported { name: String },
}

/// A sorted statement: the ids in order, one column.
#[derive(Clone, Debug, PartialEq)]
pub struct SortedQuery {
    mode: SearchMode,
    sql: String,
    runs_sql: String,
    params: Vec<Param>,
    reads: ContextReads,
    slow: bool,
}

impl SortedQuery {
    /// The search's matching rows sorted (S9: in cards mode a sort that reads
    /// note columns tests only cards whose note exists).
    pub fn for_plan(plan: &Plan, sort: &Sort, ctx: &SearchContext) -> Result<SortedQuery, SortError> {
        if plan.mode() != sort.mode {
            return Err(SortError::ModeMismatch);
        }
        let keys = keys(sort, ctx)?;
        let filter = match (sort.mode, plan.rows()) {
            (SearchMode::Notes, Rows::CardsWithNotes | Rows::Cards) => format!(
                "n.id IN (SELECT c.nid FROM cards c JOIN notes n ON n.id = c.nid WHERE {})",
                plan.predicate
            ),
            _ => plan.predicate.clone(),
        };
        let joined = keys.needs_note || plan.rows() == Rows::CardsWithNotes;
        Ok(SortedQuery::new(
            sort,
            &keys,
            &filter,
            joined,
            plan.params().to_vec(),
            plan.reads().union(keys.reads),
            plan.is_slow() || keys.slow,
        ))
    }

    /// The ids of parameter `?1`, [`SortedQuery::ids_param`], sorted without
    /// running a predicate (a resort, `browser/result-sets` H8): ids that
    /// name no row are left out, and in cards mode a sort that reads note
    /// columns leaves out cards whose note is missing (S9).
    pub fn for_ids(sort: &Sort, ctx: &SearchContext) -> Result<SortedQuery, SortError> {
        let keys = keys(sort, ctx)?;
        let filter = match sort.mode {
            SearchMode::Cards => "c.id IN (SELECT value FROM json_each(?1))",
            SearchMode::Notes => "n.id IN (SELECT value FROM json_each(?1))",
        };
        Ok(SortedQuery::new(
            sort,
            &keys,
            filter,
            keys.needs_note,
            Vec::new(),
            keys.reads,
            keys.slow,
        ))
    }

    /// The `?1` of [`SortedQuery::for_ids`]: the ids as a JSON array.
    pub fn ids_param(ids: &[i64]) -> Param {
        let mut text = String::with_capacity(ids.len() * 14 + 2);
        text.push('[');
        for (k, id) in ids.iter().enumerate() {
            if k > 0 {
                text.push(',');
            }
            text.push_str(&id.to_string());
        }
        text.push(']');
        Param::Text(text)
    }

    fn new(
        sort: &Sort,
        keys: &Keys,
        filter: &str,
        joined: bool,
        params: Vec<Param>,
        reads: ContextReads,
        slow: bool,
    ) -> SortedQuery {
        let (id, from) = match sort.mode {
            SearchMode::Cards if joined => ("c.id", "cards c JOIN notes n ON n.id = c.nid"),
            SearchMode::Cards => ("c.id", "cards c"),
            SearchMode::Notes => ("n.id", "notes n"),
        };
        let with = if keys.ctes.is_empty() {
            String::new()
        } else {
            format!("WITH {} ", keys.ctes.join(", "))
        };
        let joins: String = keys.joins.iter().map(|join| format!(" {join}")).collect();
        let dir = if sort.direction.descending() { " DESC" } else { "" };
        let order = keys
            .keys
            .iter()
            .map(String::as_str)
            .chain([id])
            .map(|key| format!("{key}{dir}"))
            .collect::<Vec<_>>()
            .join(", ");
        let body = format!("FROM {from}{joins} WHERE {filter} ORDER BY {order}");
        let mut run_keys = keys.keys.clone();
        if keys.id_after_aggregate {
            run_keys.push(format!("CASE WHEN {} IS NULL THEN NULL ELSE n.id END", keys.keys[0]));
        }
        SortedQuery {
            mode: sort.mode,
            sql: format!("{with}SELECT {id} {body}"),
            runs_sql: format!(
                "{with}SELECT {id}, DENSE_RANK() OVER (ORDER BY {}) {body}",
                run_keys.join(", ")
            ),
            params,
            reads,
            slow,
        }
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    /// One statement selecting the ids, each once, in the sort's order.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The same rows in the same order, each with a second column that is
    /// equal exactly for consecutive rows equal on every key of S4 or S5,
    /// the final id key aside: the equal-key runs of S8
    /// (`specs/conformance/browser-ops.md` B2).
    pub fn runs_sql(&self) -> &str {
        &self.runs_sql
    }

    /// The statement's parameters, `?1` first.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The context values the search and the keys read (Q8, S10).
    pub fn reads(&self) -> ContextReads {
        self.reads
    }

    /// A term or a key calls Meguri's SQL functions per row.
    pub fn is_slow(&self) -> bool {
        self.slow
    }
}

/// The keys of one column in one mode, ascending, without the final key.
#[derive(Default)]
struct Keys {
    ctes: Vec<&'static str>,
    joins: Vec<String>,
    keys: Vec<String>,
    /// Cards mode: the keys read the note's row (S9).
    needs_note: bool,
    /// Notes mode: an aggregate followed by Anki's note id key, which is the
    /// final key; a NULL aggregate is the single key NULL (S5).
    id_after_aggregate: bool,
    reads: ContextReads,
    slow: bool,
}

/// S3: deck, note type and template ranks.
const DECK_RANK: &str = "meguri_deck_rank(id, r) AS (SELECT id, ROW_NUMBER() OVER (ORDER BY name COLLATE unicase) FROM decks)";
const NOTETYPE_RANK: &str =
    "meguri_notetype_rank(id, r) AS (SELECT id, ROW_NUMBER() OVER (ORDER BY name COLLATE unicase) FROM notetypes)";
const TEMPLATE_RANK: &str = "meguri_template_rank(ntid, ord, r) AS (SELECT ntid, ord, ROW_NUMBER() OVER (ORDER BY name COLLATE unicase, ntid, ord) FROM templates)";

/// S6: E of card alias `k`, from its stored `due` and `type`.
fn due_key(k: &str, ctx: &SearchContext) -> String {
    format!(
        "(CASE WHEN {k}.due > 1000000000 OR {k}.type = 0 THEN {k}.due ELSE ({k}.due - {}) * 86400 + ({}) END)",
        ctx.today, ctx.now
    )
}

/// S5.1: P of the first card (lowest id) of the note with id `note`.
fn first_position(note: &str) -> String {
    format!(
        "(SELECT COALESCE(meguri_original_position(k.data), CASE WHEN k.type = 0 THEN k.due ELSE 0 END) \
         FROM cards k WHERE k.nid = {note} ORDER BY k.id LIMIT 1)"
    )
}

/// S5: an aggregate over the note's cards `k`.
fn over_cards(expr: &str, condition: &str) -> String {
    format!("(SELECT {expr} FROM cards k WHERE k.nid = n.id{condition})")
}

fn keys(sort: &Sort, ctx: &SearchContext) -> Result<Keys, SortError> {
    match &sort.column {
        SortColumn::Field(name) => field_keys(name, sort.mode),
        SortColumn::Column(column) => Ok(match sort.mode {
            SearchMode::Cards => card_keys(*column, ctx),
            SearchMode::Notes => note_keys(*column, ctx),
        }),
    }
}

/// `browser/workspace` V4.2, U261: the field column's key goes here.
fn field_keys(name: &str, _mode: SearchMode) -> Result<Keys, SortError> {
    Err(SortError::FieldColumnNotYetSupported { name: name.to_owned() })
}

fn day_reads() -> ContextReads {
    ContextReads::NONE.with(ContextValue::Today).with(ContextValue::Now)
}

/// S4.
fn card_keys(column: Column, ctx: &SearchContext) -> Keys {
    let plain = |keys: &[&str]| Keys {
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        ..Keys::default()
    };
    let note = |keys: &[&str]| Keys {
        needs_note: true,
        ..plain(keys)
    };
    let per_row = |key: String| Keys {
        keys: vec![key],
        slow: true,
        ..Keys::default()
    };
    match column {
        Column::SortField => note(&["n.sfld COLLATE NOCASE", "c.ord"]),
        Column::CardType => Keys {
            ctes: vec![TEMPLATE_RANK],
            joins: vec![
                "LEFT JOIN meguri_template_rank t1 ON t1.ntid = n.mid AND t1.ord = c.ord".to_owned(),
                "LEFT JOIN meguri_template_rank t0 ON t0.ntid = n.mid AND t0.ord = 0".to_owned(),
            ],
            ..note(&["COALESCE(t1.r, t0.r)", "c.ord"])
        },
        Column::Deck => Keys {
            ctes: vec![DECK_RANK],
            joins: vec!["LEFT JOIN meguri_deck_rank dr ON dr.id = c.did".to_owned()],
            ..plain(&["dr.r"])
        },
        Column::Due => Keys {
            reads: day_reads(),
            ..plain(&[due_key("c", ctx).as_str()])
        },
        Column::Ease => plain(&["(c.type = 0)", "c.factor"]),
        Column::Interval => plain(&["c.ivl"]),
        Column::Reviews => plain(&["c.reps"]),
        Column::Lapses => plain(&["c.lapses"]),
        Column::Created => note(&["n.id", "c.ord"]),
        Column::CardModified => plain(&["c.mod"]),
        Column::NoteModified => note(&["n.mod", "c.ord"]),
        Column::Notetype => Keys {
            ctes: vec![NOTETYPE_RANK],
            joins: vec!["LEFT JOIN meguri_notetype_rank nr ON nr.id = n.mid".to_owned()],
            ..note(&["nr.r"])
        },
        Column::Tags => note(&["n.tags COLLATE BINARY"]),
        Column::Position => Keys {
            slow: true,
            ..plain(&[first_position("c.nid").as_str(), "c.nid"])
        },
        Column::Stability => per_row("meguri_card_f32(c.data, 's')".to_owned()),
        Column::Difficulty => per_row("meguri_card_f32(c.data, 'd')".to_owned()),
        Column::Retrievability => Keys {
            reads: day_reads(),
            ..per_row(props::retrievability(ctx.today, ctx.now))
        },
        // Refused by `Sort::new` (S1).
        Column::Question | Column::Answer => unreachable!("{column:?} never sorts"),
    }
}

/// S5.
fn note_keys(column: Column, ctx: &SearchContext) -> Keys {
    let plain = |keys: &[&str]| Keys {
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        ..Keys::default()
    };
    let aggregate = |key: String| Keys {
        keys: vec![key],
        id_after_aggregate: true,
        ..Keys::default()
    };
    match column {
        Column::SortField => plain(&["n.sfld COLLATE NOCASE"]),
        Column::CardType => aggregate(format!("NULLIF({}, 0)", over_cards("COUNT(*)", ""))),
        Column::Deck => {
            let existing = "FROM cards k JOIN decks d ON d.id = k.did WHERE k.nid = n.id";
            Keys {
                ctes: vec![DECK_RANK],
                joins: vec![format!(
                    "LEFT JOIN meguri_deck_rank dr ON dr.id = (SELECT k.did {existing} ORDER BY k.id LIMIT 1)"
                )],
                keys: vec![
                    format!("NULLIF((SELECT COUNT(DISTINCT k.did) {existing}), 0)"),
                    "dr.r".to_owned(),
                ],
                id_after_aggregate: true,
                ..Keys::default()
            }
        }
        Column::Due => Keys {
            reads: day_reads(),
            ..aggregate(over_cards(
                &format!("MIN({})", due_key("k", ctx)),
                " AND k.odid = 0 AND k.type != 0 AND k.queue > 0",
            ))
        },
        Column::Ease => aggregate(over_cards("AVG(k.factor)", " AND k.type != 0")),
        Column::Interval => aggregate(over_cards("AVG(k.ivl)", " AND k.type IN (2, 3)")),
        Column::Reviews => aggregate(over_cards("SUM(k.reps)", "")),
        Column::Lapses => aggregate(over_cards("SUM(k.lapses)", "")),
        Column::CardModified => aggregate(over_cards("MAX(k.mod)", "")),
        Column::Created => plain(&["n.id"]),
        Column::NoteModified => plain(&["n.mod"]),
        Column::Notetype => Keys {
            ctes: vec![NOTETYPE_RANK],
            joins: vec!["LEFT JOIN meguri_notetype_rank nr ON nr.id = n.mid".to_owned()],
            ..plain(&["nr.r"])
        },
        Column::Tags => plain(&["n.tags COLLATE BINARY"]),
        Column::Position => Keys {
            slow: true,
            ..aggregate(first_position("n.id"))
        },
        // Refused by `Sort::new` (S1).
        Column::Question | Column::Answer | Column::Stability | Column::Difficulty | Column::Retrievability => {
            unreachable!("{column:?} does not sort notes")
        }
    }
}
