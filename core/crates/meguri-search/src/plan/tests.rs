//! The plan's mode and row machinery (query-model Q4, Q5, Q9) on in-memory
//! card and note tables. The product's term compilers need a schema-18
//! collection, so a test compiler stands in for `is:suspended`
//! (`c.queue = -1`) and for plain text (`n.flds = ?`); what is tested is how
//! the plan combines them, never the stand-ins themselves.

use rusqlite::{Connection, params_from_iter};

use super::*;
use crate::ast::CardState;
use crate::context::FoldMode;
use crate::parser::parse;

fn context() -> SearchContext {
    SearchContext {
        now: 1_894_701_600,
        today: 100,
        next_day_at: 1_894_766_400,
        learn_ahead_secs: SearchContext::DEFAULT_LEARN_AHEAD_SECS,
        current_deck: meguri_model::ids::DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

/// The stand-in compiler: suspended state, plain text equal to the note's
/// only field, the whole collection through the product compiler.
fn stand_in(term: &Term, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    Ok(match term {
        Term::State(CardState::Suspended) => TermSql {
            sql: "c.queue = -1".to_owned(),
            slow: false,
            reads: ContextReads::NONE,
        },
        Term::Text(text) => TermSql {
            sql: format!("n.flds = {}", params.bind(Param::Text(text.clone()))),
            slow: true,
            reads: ContextReads::NONE.with(crate::context::ContextValue::NormalizeNoteText),
        },
        // A slow term that takes the `-` before it (T9.4).
        Term::StripCloze(text) => {
            let bound = params.bind(Param::Text(text.clone()));
            TermSql {
                sql: if env.negated {
                    format!("n.flds != {bound}")
                } else {
                    format!("n.flds = {bound}")
                },
                slow: true,
                reads: ContextReads::NONE,
            }
        }
        other => return sql::compile_term(other, env, params),
    })
}

fn plan(text: &str, mode: SearchMode) -> Plan {
    let query = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    build_with(&query, mode, &context(), &SearchNotetypes::default(), &mut stand_in, false)
        .unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn relaxed(text: &str, mode: SearchMode) -> Plan {
    let query = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    build_with(&query, mode, &context(), &SearchNotetypes::default(), &mut stand_in, true)
        .unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

/// Notes 1 (`dog`, cards 10 and 11, 11 suspended) and 2 (`cat`, card 20);
/// card 30's note 3 is gone, and note 4 (`dog`) has no cards.
fn collection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, flds TEXT NOT NULL);
         CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL, queue INTEGER NOT NULL);
         INSERT INTO notes VALUES (1, 'dog'), (2, 'cat'), (4, 'dog');
         INSERT INTO cards VALUES (10, 1, 0), (11, 1, -1), (20, 2, 0), (30, 3, -1);",
    )
    .unwrap();
    conn
}

fn ids(plan: &Plan) -> Vec<i64> {
    let conn = collection();
    let mut statement = conn
        .prepare(plan.sql())
        .unwrap_or_else(|e| panic!("{}: {e}", plan.sql()));
    let values = plan.params().iter().map(|p| match p {
        Param::Integer(v) => rusqlite::types::Value::Integer(*v),
        Param::Real(v) => rusqlite::types::Value::Real(*v),
        Param::Text(v) => rusqlite::types::Value::Text(v.clone()),
    });
    let mut out: Vec<i64> = statement
        .query_map(params_from_iter(values), |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    out.sort_unstable();
    out
}

// R1080, Q4: one suspended card in a two-card note.
#[test]
fn cards_mode_returns_the_card_and_notes_mode_its_note() {
    assert_eq!(ids(&plan("is:suspended", SearchMode::Cards)), [11, 30]);
    assert_eq!(ids(&plan("is:suspended", SearchMode::Notes)), [1]);
}

// Q4: notes mode tests the whole query on one card at a time.
#[test]
fn a_negated_card_term_returns_a_note_with_any_card_left_out() {
    assert_eq!(ids(&plan("-is:suspended", SearchMode::Cards)), [10, 20]);
    assert_eq!(ids(&plan("-is:suspended", SearchMode::Notes)), [1, 2]);
    assert_eq!(ids(&plan("is:suspended or -is:suspended", SearchMode::Notes)), [1, 2]);
}

// Q5, Q6: a mixed query tests only card-and-note pairs, in both modes.
#[test]
fn mixed_scopes_join_cards_to_their_notes() {
    let cards = plan("is:suspended or cat", SearchMode::Cards);
    assert_eq!(cards.rows(), Rows::CardsWithNotes);
    assert_eq!(ids(&cards), [11, 20]);
    let notes = plan("is:suspended or cat", SearchMode::Notes);
    assert_eq!(notes.rows(), Rows::CardsWithNotes);
    assert_eq!(ids(&notes), [1, 2]);
    assert_eq!(ids(&plan("is:suspended dog", SearchMode::Cards)), [11]);
}

// Q5, Q6: a note term alone skips cards without a note in cards mode, and
// returns notes without cards in notes mode.
#[test]
fn a_note_term_reads_notes_alone_in_notes_mode() {
    let notes = plan("dog", SearchMode::Notes);
    assert_eq!(notes.rows(), Rows::Notes);
    assert_eq!(ids(&notes), [1, 4]);
    assert_eq!(ids(&plan("-dog", SearchMode::Notes)), [2]);
    assert_eq!(ids(&plan("dog", SearchMode::Cards)), [10, 11]);
}

// Q2, Q6: the whole collection includes damaged rows.
#[test]
fn the_whole_collection_reads_one_table() {
    let cards = plan("", SearchMode::Cards);
    assert_eq!(
        (cards.rows(), cards.sql()),
        (Rows::Cards, "SELECT c.id FROM cards c WHERE (1)")
    );
    assert_eq!(ids(&cards), [10, 11, 20, 30]);
    let notes = plan("  ", SearchMode::Notes);
    assert_eq!(
        (notes.rows(), notes.sql()),
        (Rows::Notes, "SELECT n.id FROM notes n WHERE (1)")
    );
    assert_eq!(ids(&notes), [1, 2, 4]);
}

// Q7, Q9: AND, OR, NOT and groups keep their SQL forms.
#[test]
fn operators_compile_in_source_order_with_bound_text() {
    let plan = plan("dog (cat or -is:suspended)", SearchMode::Cards);
    assert_eq!(
        plan.sql(),
        "SELECT c.id FROM cards c JOIN notes n ON n.id = c.nid WHERE ((n.flds = ?1) AND ((n.flds = ?2) OR NOT (c.queue = -1)))"
    );
    assert_eq!(plan.params(), [Param::Text("dog".into()), Param::Text("cat".into())]);
    assert_eq!(ids(&plan), [10]);
}

// Q9: slow terms and context reads are recorded per plan.
#[test]
fn slow_terms_and_context_reads_are_recorded() {
    let query = parse("is:suspended dog").unwrap();
    let plan = build_with(&query, SearchMode::Cards, &context(), &SearchNotetypes::default(), &mut stand_in, false).unwrap();
    assert_eq!(plan.slow_terms().len(), 1);
    assert!(matches!(
        query.node(plan.slow_terms()[0]).kind,
        NodeKind::Term(Term::Text(_))
    ));
    assert_eq!(
        plan.reads().iter().collect::<Vec<_>>(),
        [crate::context::ContextValue::NormalizeNoteText]
    );
    let fast = build_with(
        &parse("is:suspended").unwrap(),
        SearchMode::Cards,
        &context(),
        &SearchNotetypes::default(),
        &mut stand_in,
        false,
    )
    .unwrap();
    assert!(!fast.is_slow() && fast.reads().is_empty());
}

// E3: search_failed for a statement check, wherever it stands.
#[test]
fn statement_checks_fail_wherever_they_stand() {
    let query = parse("a -and b").unwrap();
    let error = Plan::build(&query, SearchMode::Cards, &context(), &SearchNotetypes::default()).unwrap_err();
    assert_eq!(
        error,
        BuildError::Search(SearchError::new(
            ErrorKind::SearchFailed(SearchFailure::NegatedOperator),
            2..6
        ))
    );
    let query = parse("a  NID:1,\u{661}").unwrap();
    let error = Plan::build(&query, SearchMode::Notes, &context(), &SearchNotetypes::default()).unwrap_err();
    let key = "NID:".to_owned();
    assert_eq!(
        error,
        BuildError::Search(SearchError::new(
            ErrorKind::SearchFailed(SearchFailure::IdList { context: key }),
            3..10
        ))
    );
}

// Q10 steps 1 to 3: slow terms become TRUE under an even number of
// negations and FALSE under an odd one, their parameters go, the rows are
// the original query's, and the relaxed result holds the original one.
#[test]
fn the_relaxed_query_replaces_slow_terms_by_negation_parity_q10() {
    let cases = [
        ("dog (cat or -is:suspended)", "((1) AND ((1) OR NOT (c.queue = -1)))"),
        ("-(dog is:suspended)", "NOT ((0) AND (c.queue = -1))"),
        ("-dog or is:suspended", "(NOT (0) OR (c.queue = -1))"),
        ("-(-dog)", "NOT NOT (1)"),
        ("-sc:dog", "(1)"),
        ("-(-sc:dog is:suspended)", "NOT ((0) AND (c.queue = -1))"),
    ];
    for (text, predicate) in cases {
        let original = plan(text, SearchMode::Cards);
        let loose = relaxed(text, SearchMode::Cards);
        assert!(loose.sql().ends_with(&format!("WHERE {predicate}")), "{text}: {}", loose.sql());
        assert!(loose.params().is_empty() && !loose.is_slow(), "{text}");
        assert_eq!(loose.rows(), original.rows(), "{text}");
        let all = ids(&loose);
        assert!(ids(&original).iter().all(|id| all.contains(id)), "{text}");
    }
    // A slow term keeps the rows its scope needs: cards with their note.
    assert_eq!(relaxed("dog", SearchMode::Cards).rows(), Rows::CardsWithNotes);
    assert_eq!(ids(&relaxed("dog", SearchMode::Cards)), [10, 11, 20]);
}

// Q10 step 4: the plan restricted to a chunk of candidates.
#[test]
fn the_restricted_statement_tests_only_the_listed_ids_q10() {
    let conn = collection();
    let original = plan("dog", SearchMode::Cards);
    let sql = original.restricted_sql("?2");
    let mut statement = conn.prepare(&sql).unwrap();
    let mut found: Vec<i64> = statement
        .query_map(params_from_iter(["dog", "[11, 20, 30]"]), |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    found.sort_unstable();
    assert_eq!(found, [11]);
    let notes = plan("is:suspended", SearchMode::Notes);
    let sql = notes.restricted_pairs_sql("?1").unwrap();
    let mut statement = conn.prepare(&sql).unwrap();
    let pairs: Vec<(i64, i64)> = statement
        .query_map(params_from_iter(["[1, 2, 3]"]), |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(pairs, [(11, 1)]);
}
