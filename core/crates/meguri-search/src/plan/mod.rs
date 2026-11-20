//! Compiling a query into a plan for cards mode or notes mode
//! (`specs/search/query-model.md` Q4, Q5, Q9): one SQL statement over the
//! rows the query's scope needs, the slow terms, the context values read,
//! and the run errors that need no database (`errors.md` E3).

mod names;
pub mod sql;

use std::ops::Range;

use crate::ast::{NodeId, NodeKind, Query, Scope, Term};
use crate::context::{ContextReads, SearchContext};
use crate::error::{ErrorKind, SearchError, SearchFailure};

pub use sql::{Param, Params, TermEnv, TermOutcome, TermSql};

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
    params: Vec<Param>,
    slow: Vec<NodeId>,
    reads: ContextReads,
}

/// Why no plan was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// A run error of E3 found before anything runs.
    Search(SearchError),
    /// The query holds a term whose matching unit (U81, U82) has not landed:
    /// the first such term, as parsed, and its range. Removed with the last
    /// stub.
    NotYetSupported { term: Term, range: Range<usize> },
}

impl From<SearchError> for BuildError {
    fn from(error: SearchError) -> BuildError {
        BuildError::Search(error)
    }
}

impl Plan {
    /// Q9: compiles `query` for `mode`. A negated operator word, or an id
    /// list holding a digit other than ASCII `0`-`9`, is `search_failed`
    /// (E3), whatever else the query holds.
    pub fn build(query: &Query, mode: SearchMode, ctx: &SearchContext) -> Result<Plan, BuildError> {
        build_with(query, mode, ctx, &mut sql::compile_term)
    }

    /// Test-only (KTD18): [`Plan::build`] with `compile` in place of
    /// [`sql::compile_term`], so that tests can run plans of terms whose
    /// matching (U81, U82) is still a stub.
    #[cfg(feature = "test-hooks")]
    pub fn build_with(
        query: &Query,
        mode: SearchMode,
        ctx: &SearchContext,
        compile: &mut dyn FnMut(&Term, &TermEnv<'_>, &mut Params) -> Result<TermOutcome, ErrorKind>,
    ) -> Result<Plan, BuildError> {
        build_with(query, mode, ctx, compile)
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
}

/// The signature of a term compiler: [`sql::compile_term`] in the product.
type Compile<'a> = dyn FnMut(&Term, &TermEnv<'_>, &mut Params) -> Result<TermOutcome, ErrorKind> + 'a;

enum Piece {
    Node(NodeId),
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
    compile: &mut Compile<'_>,
) -> Result<Plan, BuildError> {
    let rows = Rows::for_query(mode, query_scope(query));
    let env = TermEnv { rows, ctx };
    let mut params = Params::default();
    let mut predicate = String::new();
    let mut slow = Vec::new();
    let mut reads = ContextReads::NONE;
    let mut not_yet = None;
    // E4 step 3: the statement checks of every term in reading order, then
    // the pattern checks. A pattern error waits until every term passed its
    // statement checks.
    let mut pattern_error = None;
    // Left to right, so the first error the source names is the one raised.
    let mut stack = vec![Piece::Node(query.root())];
    while let Some(piece) = stack.pop() {
        let id = match piece {
            Piece::Text(text) => {
                predicate.push_str(text);
                continue;
            }
            Piece::Node(id) => id,
        };
        let node = query.node(id);
        match &node.kind {
            NodeKind::And(items) => joined(&mut stack, items, " AND "),
            NodeKind::Or(items) => joined(&mut stack, items, " OR "),
            NodeKind::Group(inside) => stack.push(Piece::Node(*inside)),
            NodeKind::Not(item) => {
                stack.push(Piece::Node(*item));
                stack.push(Piece::Text("NOT "));
            }
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
                let outcome = match compile(term, &env, &mut params) {
                    Ok(outcome) => outcome,
                    Err(kind) if sql::is_pattern_error(&kind) => {
                        pattern_error.get_or_insert_with(|| SearchError::new(kind, node.span.clone()));
                        TermOutcome::Sql(TermSql {
                            sql: "0".to_owned(),
                            slow: false,
                            reads: ContextReads::NONE,
                        })
                    }
                    Err(kind) => return Err(SearchError::new(kind, node.span.clone()).into()),
                };
                match outcome {
                    TermOutcome::Sql(term_sql) => {
                        predicate.push('(');
                        predicate.push_str(&term_sql.sql);
                        predicate.push(')');
                        if term_sql.slow {
                            slow.push(id);
                        }
                        reads = reads.union(term_sql.reads);
                    }
                    TermOutcome::NotYetSupported => {
                        not_yet.get_or_insert_with(|| (term.clone(), node.span.clone()));
                        predicate.push_str("(0)");
                    }
                }
            }
        }
    }
    if let Some(error) = pattern_error {
        return Err(error.into());
    }
    if let Some((term, range)) = not_yet {
        return Err(BuildError::NotYetSupported { term, range });
    }
    Ok(Plan {
        mode,
        rows,
        sql: sql::statement(mode, rows, &predicate),
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
