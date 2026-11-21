//! Compiling a query into a plan for cards mode or notes mode
//! (`specs/search/query-model.md` Q4, Q5, Q9): one SQL statement over the
//! rows the query's scope needs, the slow terms, the context values read,
//! and the run errors that need no database (`errors.md` E3).

mod history;
pub mod ids;
pub(crate) mod names;
mod props;
pub mod sort;
pub mod sql;
mod state;
mod text;

use std::ops::Range;

use crate::ast::{NodeId, NodeKind, Query, Scope, Term};
use crate::context::{ContextReads, SearchContext};
use crate::error::{ErrorKind, SearchError, SearchFailure};
use crate::notetypes::SearchNotetypes;

pub use sql::{Param, Params, TermEnv, TermSql};

/// What a search returns (Q4, R1080).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchMode {
    /// Matching cards: the query is true for the card with its note.
    Cards,
    /// Notes with at least one card that makes the whole query true.
    Notes,
}

/// The rows a statement tests (Q5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rows {
    /// Every card alone (alias `c`); its note need not exist.
    Cards,
    /// Every note alone (alias `n`); it need not have cards.
    Notes,
    /// Every card with its existing note (`c` joined to `n`).
    CardsWithNotes,
}

impl Rows {
    /// Q5's table.
    pub fn for_query(mode: SearchMode, scope: Scope) -> Rows {
        match (mode, scope) {
            (SearchMode::Cards, Scope::Card | Scope::Either) => Rows::Cards,
            (SearchMode::Notes, Scope::Note | Scope::Either) => Rows::Notes,
            _ => Rows::CardsWithNotes,
        }
    }
}

/// A compiled search for one mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    mode: SearchMode,
    rows: Rows,
    sql: String,
    /// The WHERE expression of `sql`, over the aliases of `rows`, for the
    /// sorted statements of [`sort`].
    predicate: String,
    params: Vec<Param>,
    slow: Vec<NodeId>,
    reads: ContextReads,
}

/// Why no plan was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// A run error of E3 found before anything runs.
    Search(SearchError),
}

impl From<SearchError> for BuildError {
    fn from(error: SearchError) -> BuildError {
        BuildError::Search(error)
    }
}

impl Plan {
    /// Q9: compiles `query` for `mode` against the collection's note types
    /// (`text-matching.md` T15: read on the transaction the statement runs
    /// on, or build the plan again when they changed). Plan errors follow
    /// `errors.md` E4 step 3: the statement checks of every term (a negated
    /// operator word, a digit other than ASCII `0`-`9` in an id list, ...)
    /// in reading order, then the pattern checks (every pattern checked and
    /// every expression compiled).
    pub fn build(
        query: &Query,
        mode: SearchMode,
        ctx: &SearchContext,
        notetypes: &SearchNotetypes,
    ) -> Result<Plan, BuildError> {
        build_with(query, mode, ctx, notetypes, &mut sql::compile_term)
    }

    /// Test-only (KTD18): [`Plan::build`] with `compile` in place of
    /// [`sql::compile_term`], so that tests can run plans on tables that
    /// hold only the columns a stand-in compiler reads.
    #[cfg(feature = "test-hooks")]
    pub fn build_with(
        query: &Query,
        mode: SearchMode,
        ctx: &SearchContext,
        compile: &mut dyn FnMut(&Term, &TermEnv<'_>, &mut Params) -> Result<TermSql, ErrorKind>,
    ) -> Result<Plan, BuildError> {
        build_with(query, mode, ctx, &SearchNotetypes::default(), compile)
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    pub fn rows(&self) -> Rows {
        self.rows
    }

    /// One statement selecting the matching ids, each once, in no order.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The statement's parameters, `?1` first.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The terms that call Meguri's SQL functions per row.
    pub fn slow_terms(&self) -> &[NodeId] {
        &self.slow
    }

    pub fn is_slow(&self) -> bool {
        !self.slow.is_empty()
    }

    /// The context values the plan read (Q8).
    pub fn reads(&self) -> ContextReads {
        self.reads
    }

    /// The plan's statement over its rows, restricted to the ids of the
    /// JSON array bound as parameter `list` (`?n`, after the plan's own):
    /// Q10 step 4's final test of a chunk of candidates. The ids are the
    /// mode's (card ids in cards mode, note ids in notes mode); the result
    /// is the matching ones, each once, in no order.
    pub fn restricted_sql(&self, list: &str) -> String {
        let id = match self.mode {
            SearchMode::Cards => "c.id",
            SearchMode::Notes => "n.id",
        };
        sql::statement(
            self.mode,
            self.rows,
            &format!("{id} IN (SELECT value FROM json_each({list})) AND ({})", self.predicate),
        )
    }

    /// `browser/result-sets` H5, H11: the lone rows (Q6) the query matches,
    /// tested on those rows alone. Cards mode with card scope: each card
    /// without a note row, with its `nid` (two columns). Notes mode with note
    /// or either scope: each note without a card (one column). `None` for the
    /// other plans, whose rows hold no lone row.
    pub fn lone_rows_sql(&self) -> Option<String> {
        match (self.mode, self.rows) {
            (SearchMode::Cards, Rows::Cards) => Some(format!(
                "SELECT c.id, c.nid FROM cards c WHERE NOT EXISTS (SELECT 1 FROM notes k WHERE k.id = c.nid) AND ({})",
                self.predicate
            )),
            (SearchMode::Notes, Rows::Notes) => Some(format!(
                "SELECT n.id FROM notes n WHERE NOT EXISTS (SELECT 1 FROM cards k WHERE k.nid = n.id) AND ({})",
                self.predicate
            )),
            _ => None,
        }
    }

    /// `browser/result-sets` H11's match data: in notes mode with card or
    /// card-and-note scope, every card-and-note pair that makes the query
    /// true, as card id and note id. `None` for the other plans.
    pub fn pairs_sql(&self) -> Option<String> {
        match (self.mode, self.rows) {
            (SearchMode::Notes, Rows::Cards | Rows::CardsWithNotes) => Some(format!(
                "SELECT c.id, c.nid FROM cards c JOIN notes n ON n.id = c.nid WHERE {}",
                self.predicate
            )),
            _ => None,
        }
    }
}

/// The signature of a term compiler: [`sql::compile_term`] in the product.
type Compile<'a> = dyn FnMut(&Term, &TermEnv<'_>, &mut Params) -> Result<TermSql, ErrorKind> + 'a;

enum Piece {
    Node(NodeId),
    /// A term that takes the `-` before it itself ([`sql::takes_negation`]).
    Negated(NodeId),
    Text(&'static str),
}

/// Q5: the scope of the whole query, through AND, OR, NOT and groups alike.
pub fn query_scope(query: &Query) -> Scope {
    query
        .nodes()
        .iter()
        .filter_map(|node| match &node.kind {
            NodeKind::Term(term) => Some(term.scope()),
            _ => None,
        })
        .fold(Scope::Either, Scope::combine)
}

/// E7's `{term}`: the term as it stands in the search, quotes included and
/// without a negating `-` (the term node's span); a built query writes the
/// term's canonical text.
fn term_text(query: &Query, span: &Range<usize>, term: &Term) -> String {
    query
        .source_text(span)
        .unwrap_or_else(|| crate::canonical::term_text(term))
}

/// E3: the key of an id list as typed, then `:`.
fn typed_key(query: &Query, span: &Range<usize>, term: &Term) -> String {
    let typed = query.source_text(span).and_then(|text| {
        let text = text.strip_prefix('"').unwrap_or(&text);
        text.split_once(':').map(|(key, _)| key.to_owned())
    });
    let key = match (typed, term) {
        (Some(key), _) => key,
        (None, Term::Ids { kind, .. }) => kind.key().to_owned(),
        (None, _) => String::new(),
    };
    format!("{key}:")
}

fn build_with(
    query: &Query,
    mode: SearchMode,
    ctx: &SearchContext,
    notetypes: &SearchNotetypes,
    compile: &mut Compile<'_>,
) -> Result<Plan, BuildError> {
    let rows = Rows::for_query(mode, query_scope(query));
    let env = TermEnv {
        rows,
        ctx,
        notetypes,
        negated: false,
    };
    let mut params = Params::default();
    let mut predicate = String::new();
    let mut slow = Vec::new();
    let mut reads = ContextReads::NONE;
    // E4 step 3: the statement checks of every term in reading order, then
    // the pattern checks. A pattern error waits until every term passed its
    // statement checks.
    let mut pattern_error = None;
    // Left to right, so the first error the source names is the one raised.
    let mut stack = vec![Piece::Node(query.root())];
    while let Some(piece) = stack.pop() {
        let (id, negated) = match piece {
            Piece::Text(text) => {
                predicate.push_str(text);
                continue;
            }
            Piece::Node(id) => (id, false),
            Piece::Negated(id) => (id, true),
        };
        let node = query.node(id);
        match &node.kind {
            NodeKind::And(items) => joined(&mut stack, items, " AND "),
            NodeKind::Or(items) => joined(&mut stack, items, " OR "),
            NodeKind::Group(inside) => stack.push(Piece::Node(*inside)),
            NodeKind::Not(item) => match &query.node(*item).kind {
                NodeKind::Term(term) if sql::takes_negation(term) => stack.push(Piece::Negated(*item)),
                _ => {
                    stack.push(Piece::Node(*item));
                    stack.push(Piece::Text("NOT "));
                }
            },
            NodeKind::NegatedOperator(_) => {
                let kind = ErrorKind::SearchFailed(SearchFailure::NegatedOperator);
                return Err(SearchError::new(kind, node.span.clone()).into());
            }
            NodeKind::Term(term) => {
                if let Term::Ids { list, .. } = term
                    && list.chars().any(|c| c != ',' && !c.is_ascii_digit())
                {
                    let context = typed_key(query, &node.span, term);
                    let kind = ErrorKind::SearchFailed(SearchFailure::IdList { context });
                    return Err(SearchError::new(kind, node.span.clone()).into());
                }
                let term_env = TermEnv { negated, ..env };
                let term_sql = match compile(term, &term_env, &mut params) {
                    Ok(term_sql) => term_sql,
                    Err(kind) if sql::is_pattern_error(&kind) => {
                        pattern_error.get_or_insert_with(|| SearchError::new(kind, node.span.clone()));
                        TermSql {
                            sql: "0".to_owned(),
                            slow: false,
                            reads: ContextReads::NONE,
                        }
                    }
                    Err(ErrorKind::SearchFailed(SearchFailure::NumberOutOfRange { .. })) => {
                        let term = term_text(query, &node.span, term);
                        let kind = ErrorKind::SearchFailed(SearchFailure::NumberOutOfRange { term });
                        return Err(SearchError::new(kind, node.span.clone()).into());
                    }
                    Err(kind) => return Err(SearchError::new(kind, node.span.clone()).into()),
                };
                predicate.push('(');
                predicate.push_str(&term_sql.sql);
                predicate.push(')');
                if term_sql.slow {
                    slow.push(id);
                }
                reads = reads.union(term_sql.reads);
            }
        }
    }
    if let Some(error) = pattern_error {
        return Err(error.into());
    }
    Ok(Plan {
        mode,
        rows,
        sql: sql::statement(mode, rows, &predicate),
        predicate,
        params: params.into_vec(),
        slow,
        reads,
    })
}

/// Pushes `( item joiner item ... )` so the pieces pop in order.
fn joined(stack: &mut Vec<Piece>, items: &[NodeId], joiner: &'static str) {
    stack.push(Piece::Text(")"));
    for (k, item) in items.iter().enumerate().rev() {
        stack.push(Piece::Node(*item));
        if k > 0 {
            stack.push(Piece::Text(joiner));
        }
    }
    stack.push(Piece::Text("("));
}

#[cfg(test)]
mod tests;
