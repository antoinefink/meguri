//! The SQL writer's pieces (`specs/search/query-model.md` Q9): bound
//! parameters, the statement around the predicate, and each term's SQL.

use crate::ast::{CardState, Term};
use crate::context::{ContextReads, ContextValue, SearchContext};
use crate::error::ErrorKind;

use super::{Rows, SearchMode};

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
}

/// What a term compiles against: the rows the statement tests (aliases `c`
/// for `cards` and `n` for `notes`) and the outside values.
#[derive(Clone, Copy, Debug)]
pub struct TermEnv<'a> {
    pub rows: Rows,
    pub ctx: &'a SearchContext,
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

/// What compiling a term gives.
#[derive(Clone, Debug, PartialEq)]
pub enum TermOutcome {
    Sql(TermSql),
    /// The unit that owns the term's matching (U81, U82) has not landed.
    NotYetSupported,
}

/// Q9: the term's SQL. U80 compiles the whole collection; the text terms are
/// U81's and the name, id, state, property and history terms U82's, which
/// replace their stubs here.
pub fn compile_term(term: &Term, env: &TermEnv<'_>, params: &mut Params) -> Result<TermOutcome, ErrorKind> {
    Ok(match term {
        // Q2: true for every row.
        Term::WholeCollection => TermOutcome::Sql(TermSql {
            sql: "1".to_owned(),
            slow: false,
            reads: ContextReads::NONE,
        }),
        // state-and-flags S1, S3: compare stored due, never type or odue.
        Term::State(CardState::Due) => {
            let today = params.bind(Param::Integer(i64::from(env.ctx.today)));
            // S3 permits saturating the upper cutoff over the whole i64 domain.
            // Compute before SQL so SQLite cannot promote an overflow to REAL.
            let cutoff = env.ctx.now.saturating_add(i64::from(env.ctx.learn_ahead_secs));
            let cutoff = params.bind(Param::Integer(cutoff));
            TermOutcome::Sql(TermSql {
                sql: format!("(c.queue IN (2, 3) AND c.due <= {today}) OR (c.queue IN (1, 4) AND c.due <= {cutoff})"),
                slow: false,
                reads: ContextReads::NONE
                    .with(ContextValue::Now)
                    .with(ContextValue::Today)
                    .with(ContextValue::LearnAhead),
            })
        }
        // S2: this stored predicate performs no unburying or day query.
        Term::State(CardState::Buried) => TermOutcome::Sql(TermSql {
            sql: "c.queue IN (-2, -3)".to_owned(),
            slow: false,
            reads: ContextReads::NONE,
        }),
        _ => TermOutcome::NotYetSupported,
    })
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
