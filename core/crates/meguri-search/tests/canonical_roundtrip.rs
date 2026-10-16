//! Canonical search text (`specs/search/canonical-form.md`): F8's examples,
//! and the R1077 round trip over 10,000 generated searches, whose plans run
//! on a fixture collection with stand-in term compilers. The committed
//! oracle goldens (`conformance/goldens/search/`) are checked through the
//! testkit's `canonical_search` adapter in T0.

use std::collections::BTreeSet;

use meguri_model::ids::DeckId;
use meguri_search::ast::{NodeId, NodeKind, Property, Query};
use meguri_search::error::ErrorKind;
use meguri_search::plan::sql::compile_term;
use meguri_search::plan::{BuildError, Param, Params, Plan, Rows, SearchMode, TermEnv, TermOutcome, TermSql};
use meguri_search::{ContextReads, FoldMode, Scope, SearchContext, Term, canonical_text, parse};
use rusqlite::{Connection, params_from_iter};

fn canonical(text: &str) -> String {
    canonical_text(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

// The examples of F2 to F6 and F8, from the spec's text and tables.
#[test]
fn the_spec_examples_write_anki_bytes() {
    let cases = [
        // F8.
        ("dog cat or  fish", "dog cat OR fish"),
        ("", "deck:*"),
        ("   ", "deck:*"),
        ("\t\n", "deck:*"),
        ("a\tb", "a b"),
        ("a\u{A0}b", "a b"),
        ("a\u{3000}b", "a b"),
        ("\"cedar\" \"p q r\"", "cedar \"p q r\""),
        ("stem:\"oak\"", "stem:oak"),
        ("key:\"value with spaces\"", "\"key:value with spaces\""),
        ("key: \"value\"", "key: value"),
        ("a\\( \\)b", "\"a(\" \")b\""),
        ("\\-bloom", "\"-bloom\""),
        ("\"-bloom\"", "\"-bloom\""),
        ("--a", "-\"-a\""),
        ("q\\_\\:\\*", "q\\_\\:\\*"),
        ("deck:a\\:b", "deck:a:b"),
        ("deck:a\\-b", "deck:a-b"),
        ("deck:a\\(b", "\"deck:a(b\""),
        ("\"Or\"  \"aND\"", "\"Or\" \"aND\""),
        ("\"k l m\"no", "\"k l m\" no"),
        ("tag:re\\:x", "tag:re\\:x"),
        ("f:re\\:x", "f:re\\:x"),
        ("re:\\(", "\"re:\\(\""),
        ("card:0", "card:1"),
        ("card:+1", "card:1"),
        ("card:65536", "card:65536"),
        ("rated:0", "rated:1"),
        ("rated:2:+3", "rated:2:3"),
        ("resched:0", "resched:0"),
        ("prop:ease>3.0", "prop:ease>3"),
        ("prop:s>1.0", "prop:s>1"),
        ("prop:pos=+3", "prop:pos=3"),
        ("\"prop:cds:tip=one two\"", "\"prop:cds:tip=one two\""),
        ("dupe:1,a\\\\b", "dupe:1,a\\\\b"),
        ("-(a or b)", "-(a OR b)"),
        ("(a b)", "a b"),
        ("((a b))", "(a b)"),
        // F2.
        ("(a)", "a"),
        ("-(a b)", "-(a b)"),
        // F3.
        ("7 and 8  or 9", "7 8 OR 9"),
        ("a(b)", "a (b)"),
        ("\"x\"\"y\"", "x y"),
        ("a -and b", "a -  b"),
        ("-or", "- OR "),
        // F4.
        ("\"-\"", "-"),
        ("\"-a\"", "\"-a\""),
        ("\"a-b\"", "a-b"),
        ("\"(\"", "\"(\""),
        ("\"AnD\"", "\"AnD\""),
        // F5.
        ("DECK:x", "deck:x"),
        ("card:01", "card:1"),
        ("flag:007", "flag:7"),
        ("added:0", "added:1"),
        ("nid:007", "nid:007"),
        // F6.
        ("mid:+5", "mid:5"),
        ("prop:due=-0", "prop:due=0"),
        ("prop:ivl>=+6", "prop:ivl>=6"),
        ("dupe:+1,a", "dupe:1,a"),
    ];
    for (text, expected) in cases {
        assert_eq!(canonical(text), expected, "{text:?}");
    }
}

// F6's table: each operand as typed and as written. F6 is the same for every
// float property (F5), so the rows go through `prop:d<`.
#[test]
fn float_operands_are_written_as_f6_says() {
    let rows: [(&[&str], &str); 14] = [
        (&["1.0", "1.", "1E0"], "1"),
        (&["2.50"], "2.5"),
        (&["1e3", "1E3"], "1000"),
        (&[".5", "+.5"], "0.5"),
        (&["0.1"], "0.1"),
        (&["0.3333333333"], "0.33333334"),
        (&["1e-10"], "0.0000000001"),
        (&["16777217"], "16777216"),
        (&["123456789"], "123456790"),
        (&["1e39", "1e400", "infinity", "inf"], "inf"),
        (&["-inf"], "-inf"),
        (&["nan", "NaN"], "NaN"),
        (&["1e-50"], "0"),
        (&["-1e-50", "-0"], "-0"),
    ];
    for (typed, written) in rows {
        for operand in typed {
            assert_eq!(
                canonical(&format!("prop:d<{operand}")),
                format!("prop:d<{written}"),
                "{operand:?}"
            );
        }
    }
}

// F3's joiners, F4's quoting and F5's `prop:cds:` row, on Meguri's own
// inputs: expected texts follow the rules' wording.
#[test]
fn joiners_and_quotes_follow_the_rules() {
    let cases = [
        // F3: explicit `and` is one space, `or` is ` OR `.
        ("pear and fig or lime", "pear fig OR lime"),
        // F4: a quoted text keeps its quotes only when it needs them.
        ("\"kiwi\" \"green tea\"", "kiwi \"green tea\""),
        // F4: operator words in any letter case are quoted.
        ("\"anD\" plum \"OR\"", "\"anD\" plum \"OR\""),
        // G3, F3: a quote ends a term, and the writer puts a space after it.
        ("\"red fox\"den", "\"red fox\" den"),
        // F5: a `prop:cds:` value holding a space is quoted whole.
        ("\"prop:cds:hint=two words\"", "\"prop:cds:hint=two words\""),
    ];
    for (text, expected) in cases {
        assert_eq!(canonical(text), expected, "{text:?}");
    }
}

// F7's first three shapes: their canonical text does not read back the same.
#[test]
fn the_spec_exceptions_keep_anki_bytes() {
    assert_eq!(canonical("-and"), "- ");
    assert_eq!(canonical(&canonical("-and")), "-");
    assert_eq!(canonical("-OR"), "- OR ");
    assert_eq!(canonical_text("- OR ").unwrap_err().code(), "misplaced_or");
    assert_eq!(canonical("preset:a\\\"b"), "preset:a\\\\\"b");
    assert_eq!(canonical_text("preset:a\\\\\"b").unwrap_err().code(), "unclosed_quote");
    assert_eq!(canonical("has\\-cd:x"), "has-cd:x");
    assert!(matches!(term_of("has-cd:x"), Term::HasCustomData(_)));
}

// F7's fourth shape: F2.2 drops one level of brackets per writing, so the
// canonical text of a search that is one group holding only a group is not
// canonical again. It still means the same; the round trip checks that.
#[test]
fn a_group_holding_a_group_loses_one_level_per_writing() {
    assert_eq!(canonical("((a b))"), "(a b)");
    assert_eq!(canonical("(a b)"), "a b");
    assert_eq!(canonical("(((x)))"), "((x))");
    assert_eq!(canonical("((x))"), "(x)");
    assert_eq!(canonical("(x)"), "x");
    // Only the whole search loses a level: a negated group, or a group among
    // other items, is written as it is.
    assert_eq!(canonical("-((a))"), "-((a))");
    assert_eq!(canonical("((a)) b"), "((a)) b");
}

fn term_of(text: &str) -> Term {
    let query = parse(text).unwrap();
    match &query.node(query.root()).kind {
        NodeKind::Term(term) => term.clone(),
        other => panic!("{other:?}"),
    }
}

/// SplitMix64, so the generated cases are the same on every run.
struct Seq(u64);

impl Seq {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    /// The key in a random ASCII letter case (R1031).
    fn key(&mut self, key: &str) -> String {
        key.chars()
            .map(|c| if self.below(3) == 0 { c.to_ascii_uppercase() } else { c })
            .collect()
    }
}

const WORDS: [&str; 22] = [
    "dog", "a", "é", "日本", "x*y", "a_b", "3\\:30", "\\-x", "\\\"q", "a\\\\b", "\\(", "\\)", "-", "not", "a-b", "\\*",
    "\\_", "\u{1F}", "andy", "Or1", "w\\:x", "\\\\",
];
const QUOTED: [&str; 12] = [
    "a b", "and", "OR", "-a", "(x)", "a\\\"b", "x:y z", "a\\:b c", "日 本", "-", "a\\\\ b", "\\(",
];
const VALUES: [&str; 14] = [
    "a", "a*b", "a\\_b", "x\\:y", "a\\-b", "\\\"q", "re:x", "nc:y", "", "a\\(b", "-a", "x:y", "a\\\\b", "RE:z",
];
const FLOATS: [&str; 16] = [
    "1.0",
    "2.50",
    ".5",
    "+.5",
    "1e3",
    "nan",
    "NaN",
    "inf",
    "-inf",
    "-0",
    "0.3333333333",
    "1e-50",
    "16777217",
    "3.4e38",
    "1E0",
    "7",
];
const STATES: [&str; 8] = [
    "new",
    "review",
    "learn",
    "due",
    "buried",
    "buried-manually",
    "buried-sibling",
    "suspended",
];
const COMPARISONS: [&str; 6] = ["<", ">", "<=", ">=", "=", "!="];

fn term(seq: &mut Seq) -> String {
    match seq.below(30) {
        0..=4 => seq.pick(&WORDS).to_owned(),
        5 | 6 => format!("\"{}\"", seq.pick(&QUOTED)),
        7 => format!("{}:{}", seq.key("deck"), seq.pick(&VALUES)),
        8 => format!("{}:\"{}\"", seq.key("note"), seq.pick(&QUOTED)),
        9 => {
            let prefix = seq.pick(&["", "re:", "nc:", "re\\:", "NC:"]);
            format!("{}:{prefix}{}", seq.key("tag"), seq.pick(&VALUES))
        }
        10 => {
            let value = match seq.below(4) {
                0 => seq.below(70_000).to_string(),
                1 => format!("+{}", seq.below(10)),
                2 => format!("0{}", seq.below(10)),
                _ => seq.pick(&VALUES).to_owned(),
            };
            format!("{}:{value}", seq.key("card"))
        }
        11 => format!("{}:{}", seq.key("flag"), seq.pick(&["0", "7", "+3", "007"])),
        12 => format!("{}:{}", seq.key("is"), seq.pick(&STATES)),
        13 => {
            let key = seq.pick(&["added", "edited", "introduced", "resched"]);
            format!("{}:{}", seq.key(key), seq.pick(&["0", "1", "+5", "007", "4294967295"]))
        }
        14 => {
            let button = seq.pick(&["", ":1", ":+4", ":04"]);
            format!("{}:{}{button}", seq.key("rated"), seq.pick(&["0", "3", "+2"]))
        }
        15 => {
            let key = seq.pick(&["did", "nid", "cid"]);
            format!(
                "{}:{}",
                seq.key(key),
                seq.pick(&["1", "007", "1,2,3", "12345678901234567890"])
            )
        }
        16 => format!("{}:{}", seq.key("mid"), seq.pick(&["+5", "-5", "007", "-0"])),
        17 => {
            let key = seq.pick(&["re", "nc", "sc", "w", "has-cd"]);
            format!("{}:{}", seq.key(key), seq.pick(&VALUES))
        }
        18 => format!(
            "{}:{}",
            seq.key("re"),
            seq.pick(&["\\d", "a\\x", "\\\\", "te\\\"st", "^x$"])
        ),
        19 => {
            let text = seq.pick(&["a", "a\\\\b", "a\\\"b", "a\\x", "", "a,b", "\\\\\\\""]);
            format!("{}:{}{text}", seq.key("dupe"), seq.pick(&["1,", "+1,", "-5,", "-0,"]))
        }
        20 => format!(
            "{}:{}",
            seq.key("preset"),
            seq.pick(&["Evening", "a\\x", "a\\-b", "a\\:b"])
        ),
        21 => {
            let property = seq.pick(&["ivl", "reps", "lapses", "pos"]);
            let operand = seq.pick(&["3", "+5", "007", "4294967295"]);
            format!("prop:{property}{}{operand}", seq.pick(&COMPARISONS))
        }
        22 => {
            let property = seq.pick(&["ease", "s", "d", "r", "cdn:k1", "cdn:ABC"]);
            format!("prop:{property}{}{}", seq.pick(&COMPARISONS), seq.pick(&FLOATS))
        }
        23 => {
            let property = seq.pick(&["due", "rated", "resched"]);
            let operand = match property {
                "rated" => seq.pick(&["-1", "-0", "0", "+0", "-1:1", "0:+4"]),
                _ => seq.pick(&["-1", "-0", "0", "+0", "-2147483648"]),
            };
            format!("prop:{property}{}{operand}", seq.pick(&COMPARISONS))
        }
        24 => format!(
            "prop:cds:k{}{}",
            seq.pick(&COMPARISONS),
            seq.pick(&["", "x", "a\\:b", "a\\x"])
        ),
        25 | 26 => {
            let field = seq.pick(&[
                "f", "Front", "f*", "*", "_*", "de\\:ck", "f\\(x\\)", "rid", "a\\\"b", "and",
            ]);
            let prefix = seq.pick(&["", "", "re:", "nc:", "re\\:", "nc\\:"]);
            format!("{field}:{prefix}{}", seq.pick(&VALUES))
        }
        27 => format!("\"my field:{}\"", seq.pick(&QUOTED)),
        28 => format!("{}:\"{}\"", seq.pick(&["key", "deck", "w", "f"]), seq.pick(&QUOTED)),
        _ => format!(
            "{}:{}",
            seq.pick(&["deck", "tag", "note"]),
            seq.pick(&["*", "a::b", "current", "none"])
        ),
    }
}

/// A generated sequence of items, `depth` levels of groups at most.
fn sequence(seq: &mut Seq, depth: u32) -> String {
    let items = 1 + seq.below(4);
    let mut out = String::new();
    for k in 0..items {
        if k > 0 {
            out.push_str(seq.pick(&[" ", " ", "  ", "\t", " and ", " or ", " AND ", " Or ", "\u{3000}", "\n"]));
        }
        let negated = seq.below(5) == 0;
        if negated {
            out.push('-');
        }
        if depth > 0 && seq.below(4) == 0 {
            out.push('(');
            out.push_str(&sequence(seq, depth - 1));
            out.push(')');
        } else {
            out.push_str(&term(seq));
        }
    }
    out
}

/// F7's first three shapes, excluded from the round trip.
fn is_exception(query: &Query) -> bool {
    query.nodes().iter().any(|node| match &node.kind {
        NodeKind::NegatedOperator(_) => true,
        NodeKind::Term(Term::Preset(text)) => text.contains("\\\""),
        NodeKind::Term(Term::Property {
            property: Property::CustomString { value, .. },
            ..
        }) => value.contains("\\\""),
        NodeKind::Term(Term::Field { name, .. }) => name.eq_ignore_ascii_case("has-cd"),
        _ => false,
    })
}

/// F7's fourth shape (`a_group_holding_a_group_loses_one_level_per_writing`):
/// the whole search is a group whose only item is a group. Its canonical text
/// means the same, but is not canonical again.
fn is_group_of_group(query: &Query) -> bool {
    match query.node(query.root()).kind {
        NodeKind::Group(inside) => matches!(query.node(inside).kind, NodeKind::Group(_)),
        _ => false,
    }
}

/// The node a meaning comparison starts from: one root group less (F2.2
/// drops it, and a group is transparent, Q1).
fn meaning_root(query: &Query) -> NodeId {
    match query.node(query.root()).kind {
        NodeKind::Group(inside) => inside,
        _ => query.root(),
    }
}

/// The same query, spans aside.
fn same_meaning(a: &Query, a_root: NodeId, b: &Query, b_root: NodeId) -> bool {
    let mut stack = vec![(a_root, b_root)];
    while let Some((x, y)) = stack.pop() {
        match (&a.node(x).kind, &b.node(y).kind) {
            (NodeKind::And(xs), NodeKind::And(ys)) | (NodeKind::Or(xs), NodeKind::Or(ys)) if xs.len() == ys.len() => {
                stack.extend(xs.iter().copied().zip(ys.iter().copied()));
            }
            (NodeKind::Not(x), NodeKind::Not(y)) | (NodeKind::Group(x), NodeKind::Group(y)) => stack.push((*x, *y)),
            (NodeKind::Term(x), NodeKind::Term(y)) if x == y => {}
            (NodeKind::NegatedOperator(x), NodeKind::NegatedOperator(y)) if x == y => {}
            _ => return false,
        }
    }
    true
}

fn context() -> SearchContext {
    SearchContext {
        now: 1_894_701_600,
        today: 100,
        next_day_at: 1_894_766_400,
        learn_ahead_secs: SearchContext::DEFAULT_LEARN_AHEAD_SECS,
        current_deck: DeckId(1),
        normalize_note_text: true,
        ignore_accents: false,
        fold: FoldMode::Unicode,
    }
}

/// Stands in for U81's and U82's matchers, which are stubs in U80. A term is
/// true on a fixed set of rows that depends on the term alone: it reads only
/// the ids its scope allows (Q3, Q5) and binds its text as a parameter, so
/// the ids a search finds depend on the plan's rows, joins, operators and
/// parameter numbers. The whole collection is the product's own SQL.
fn stand_in(term: &Term, env: &TermEnv<'_>, params: &mut Params) -> Result<TermOutcome, ErrorKind> {
    if *term == Term::WholeCollection {
        return compile_term(term, env, params);
    }
    let id = match (term.scope(), env.rows) {
        (Scope::Card, _) => "c.id",
        (Scope::Note, _) => "n.id",
        (Scope::CardAndNote, _) => "c.id + n.id",
        (Scope::Either, Rows::Notes) => "n.id",
        (Scope::Either, _) => "c.nid",
    };
    let text = params.bind(Param::Text(format!("{term:?}")));
    Ok(TermOutcome::Sql(TermSql {
        sql: format!("({id} + length({text})) % 3 = 0"),
        slow: false,
        reads: ContextReads::NONE,
    }))
}

/// The fixture collection: notes 1-8 with none to three cards each (note 8
/// has none), and card 120, whose note 9 is gone (Q6). Only the columns the
/// plan's statement and the stand-in read.
fn collection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY);
         CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL);
         INSERT INTO notes VALUES (1), (2), (3), (4), (5), (6), (7), (8);
         INSERT INTO cards VALUES (101, 1), (102, 1), (103, 2), (104, 3), (105, 3), (106, 3),
             (107, 4), (108, 5), (109, 5), (110, 6), (111, 7), (120, 9);",
    )
    .unwrap();
    conn
}

/// What a plan does; node ids are left out, being positions in the source.
fn summary(plan: &Plan) -> String {
    format!(
        "{:?} {} {:?} {:?} {}",
        plan.rows(),
        plan.sql(),
        plan.params(),
        plan.reads(),
        plan.slow_terms().len()
    )
}

/// The ids a plan finds in the fixture collection, ascending.
fn ids(conn: &Connection, plan: &Plan) -> Vec<i64> {
    let mut statement = conn
        .prepare_cached(plan.sql())
        .unwrap_or_else(|e| panic!("{}: {e}", plan.sql()));
    let values = plan.params().iter().map(|p| match p {
        Param::Integer(v) => rusqlite::types::Value::Integer(*v),
        Param::Real(v) => rusqlite::types::Value::Real(*v),
        Param::Text(v) => rusqlite::types::Value::Text(v.clone()),
    });
    let mut found: Vec<i64> = statement
        .query_map(params_from_iter(values), |row| row.get(0))
        .unwrap_or_else(|e| panic!("{}: {e}", plan.sql()))
        .collect::<Result<_, _>>()
        .unwrap();
    found.sort_unstable();
    found
}

// R1077, F7: canonical(canonical(t)) = canonical(t), and canonical(t) means
// what t means: the same query, the same plan, and on the fixture collection
// the same ids in both modes. F7's fourth shape is checked for its meaning
// only. A case counts only when both searches ran in both modes.
#[test]
fn canonical_texts_round_trip() {
    let conn = collection();
    let mut seq = Seq(0x00C0_FFEE);
    let mut checked = 0;
    let mut attempts = 0;
    // The distinct result sizes per mode, cards then notes.
    let mut sizes = [BTreeSet::new(), BTreeSet::new()];
    while checked < 10_000 {
        attempts += 1;
        assert!(attempts < 200_000, "the generator yields too few searches that run");
        let text = sequence(&mut seq, 3);
        let Ok(query) = parse(&text) else {
            continue;
        };
        if is_exception(&query) {
            continue;
        }
        let once = query.canonical_text();
        let again = parse(&once).unwrap_or_else(|e| panic!("{text:?} -> {once:?}: {e:?}"));
        assert!(
            same_meaning(&query, meaning_root(&query), &again, again.root()),
            "{text:?} -> {once:?} reads differently"
        );
        if !is_group_of_group(&query) {
            assert_eq!(again.canonical_text(), once, "{text:?}");
        }
        let mut ran = 0;
        for (k, mode) in [SearchMode::Cards, SearchMode::Notes].into_iter().enumerate() {
            let first = Plan::build_with(&query, mode, &context(), &mut stand_in);
            let second = Plan::build_with(&again, mode, &context(), &mut stand_in);
            match (first, second) {
                (Ok(first), Ok(second)) => {
                    assert_eq!(summary(&first), summary(&second), "{text:?} -> {once:?}");
                    let found = ids(&conn, &first);
                    assert_eq!(found, ids(&conn, &second), "{text:?} -> {once:?} in {mode:?}");
                    sizes[k].insert(found.len());
                    ran += 1;
                }
                (Err(BuildError::Search(first)), Err(BuildError::Search(second))) => {
                    assert_eq!(first.code(), second.code(), "{text:?} -> {once:?}");
                }
                (first, second) => panic!("{text:?} -> {once:?} in {mode:?}: {first:?} / {second:?}"),
            }
        }
        if ran == 2 {
            checked += 1;
        }
    }
    // The stand-in must not make every search find nothing or everything.
    assert!(sizes.iter().all(|seen| seen.len() > 3), "result sizes {sizes:?}");
}
