//! The SQL writer's pieces (`specs/search/query-model.md` Q9): bound
//! parameters, the statement around the predicate, and each term's SQL.

use crate::ast::Term;
use crate::context::{ContextReads, SearchContext};
use crate::error::{ErrorKind, SearchFailure};
use crate::notetypes::SearchNotetypes;

use super::{Rows, SearchMode, history, names, props, state, text};

/// A bound parameter. All user text goes in parameters; numbers and ids may
/// be written into the statement (Q9).
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Integer(i64),
    Real(f64),
    Text(String),
}

/// The parameters of one statement, numbered from 1 in binding order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params(Vec<Param>);

impl Params {
    /// Binds `param` and returns its placeholder (`?3`).
    pub fn bind(&mut self, param: Param) -> String {
        self.0.push(param);
        format!("?{}", self.0.len())
    }

    pub(super) fn into_vec(self) -> Vec<Param> {
        self.0
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    /// Drops the parameters bound after the first `len`.
    pub(super) fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    /// Parameters already bound (a plan's), so that more follow them.
    pub(super) fn from_vec(params: Vec<Param>) -> Params {
        Params(params)
    }
}

/// An id list as one JSON array text, for statements that read it through
/// `json_each` (a resort's ids, a chunk of candidates).
pub fn id_list(ids: &[i64]) -> Param {
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

/// What a term compiles against: the rows the statement tests (aliases `c`
/// for `cards` and `n` for `notes`), the outside values and the note types.
#[derive(Clone, Copy, Debug)]
pub struct TermEnv<'a> {
    pub rows: Rows,
    pub ctx: &'a SearchContext,
    pub notetypes: &'a SearchNotetypes,
    /// A `-` stands directly before the term, which the plan leaves to the
    /// term: only `sc:` takes it (`text-matching.md` T9.4, [`takes_negation`]).
    pub negated: bool,
}

/// One term's predicate.
#[derive(Clone, Debug, PartialEq)]
pub struct TermSql {
    /// An SQL boolean expression over the env's rows.
    pub sql: String,
    /// It calls Meguri's SQL functions per row (Q9: regex, `nc:`, Unicode
    /// folding, `prop:r`, ...), so U85 may stream the search.
    pub slow: bool,
    /// The context values it read (Q8).
    pub reads: ContextReads,
}

/// Q9: the term's SQL. U80 compiles the whole collection, U81 the text terms
/// and U82 the name, id, state, property and history terms. An `Err` is a
/// plan error of E4 step 3: pattern errors ([`is_pattern_error`]) wait for
/// the statement checks of every term.
pub fn compile_term(term: &Term, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    Ok(match term {
        Term::Tag { mode, text } => names::tag(*mode, text, params)?,
        Term::Deck(value) => names::deck(value, env, params)?,
        Term::Preset(value) => names::preset(value, params),
        Term::Notetype(value) => names::notetype(value, params)?,
        Term::Template(template) => names::template(template, params)?,
        Term::Ids { kind, list } => names::ids(*kind, list, env),
        Term::NotetypeId(id) => names::notetype_id(*id),
        Term::Duplicates { notetype, text } => names::duplicates(*notetype, text, env, params),
        // Q2: true for every row.
        Term::WholeCollection => TermSql {
            sql: "1".to_owned(),
            slow: false,
            reads: ContextReads::NONE,
        },
        Term::State(state) => state::state(*state, env, params),
        Term::Flag(flag) => state::flag(*flag),
        Term::Property { property, comparison } => props::property(property, *comparison, env, params)?,
        Term::HasCustomData(key) => props::has_custom_data(key, params),
        Term::Added(days) => history::added(*days, env.ctx),
        Term::Edited(days) => history::edited(*days, env.ctx),
        Term::Introduced(days) => history::introduced(*days, env.ctx),
        Term::Rated { days, button } => history::rated(*days, *button, env.ctx),
        Term::Rescheduled(days) => history::rescheduled(*days, env.ctx),
        Term::Text(value) => text::plain(value, env, params)?,
        Term::NoCombining(value) => text::no_combining(value, env, params)?,
        Term::StripCloze(value) => text::strip_cloze(value, env.negated, env, params)?,
        Term::Word(value) => text::word(value, env, params)?,
        Term::Regex(value) => text::regex(value, env, params)?,
        Term::Field { name, mode, text } => text::field(name, *mode, text, env, params)?,
    })
}

/// The terms that take a `-` standing directly before them themselves
/// (T9.4): the plan compiles them with [`TermEnv::negated`] and no `NOT`.
pub fn takes_negation(term: &Term) -> bool {
    matches!(term, Term::StripCloze(_))
}

/// E4 step 3.2: the errors of the pattern checks (pattern size, then the
/// compilation of an expression). Every other plan error is a statement
/// check, which comes first.
pub fn is_pattern_error(kind: &ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::InvalidRegex { .. }
            | ErrorKind::SearchFailed(SearchFailure::PatternTooLong | SearchFailure::ExpressionTooLarge { .. })
    )
}

/// Q5, Q9: the statement selecting the mode's ids from the rows, with no
/// ordering (U84 adds it).
pub(super) fn statement(mode: SearchMode, rows: Rows, predicate: &str) -> String {
    let select = match (mode, rows) {
        (SearchMode::Cards, Rows::Cards) => "SELECT c.id FROM cards c",
        (SearchMode::Cards, _) => "SELECT c.id FROM cards c JOIN notes n ON n.id = c.nid",
        (SearchMode::Notes, Rows::Notes) => "SELECT n.id FROM notes n",
        (SearchMode::Notes, _) => "SELECT DISTINCT n.id FROM cards c JOIN notes n ON n.id = c.nid",
    };
    format!("{select} WHERE {predicate}")
}
