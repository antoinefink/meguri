//! Canonical search text (`specs/search/canonical-form.md`): F8's examples,
//! and the R1077 round trip over 10,000 generated searches. The committed
//! oracle goldens (`conformance/goldens/search/`) are checked through the
//! testkit's `canonical_search` adapter in T0.

use meguri_model::ids::DeckId;
use meguri_search::ast::{NodeId, NodeKind, Property, Query};
use meguri_search::plan::{BuildError, Plan, SearchMode};
use meguri_search::{FoldMode, SearchContext, Term, canonical_text, parse};

fn canonical(text: &str) -> String {
    canonical_text(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

// F8 and F6, from the spec's tables.
#[test]
fn the_spec_examples_write_anki_bytes() {
    let cases = [
        ("dog cat or  fish", "dog cat OR fish"),
        ("", "deck:*"),
        ("   ", "deck:*"),
        ("\t\n", "deck:*"),
        ("a\tb", "a b"),
        ("a\u{A0}b", "a b"),
        ("a\u{3000}b", "a b"),
        ("\"foo\" \"b a r\"", "foo \"b a r\""),
        ("field:\"foo\"", "field:foo"),
        ("key:\"value with spaces\"", "\"key:value with spaces\""),
        ("key: \"value\"", "key: value"),
        ("\\( \\)", "\"(\" \")\""),
        ("\\-foo", "\"-foo\""),
        ("\"-foo\"", "\"-foo\""),
        ("--a", "-\"-a\""),
        ("\\*\\:\\_", "\\*\\:\\_"),
        ("deck:\\:", "deck::"),
        ("deck:a\\-b", "deck:a-b"),
        ("deck:a\\(b", "\"deck:a(b\""),
        ("\"aNd\" \"oR\"", "\"aNd\" \"oR\""),
        ("\"f o o\"bar", "\"f o o\" bar"),
        ("tag:re\\:x", "tag:re\\:x"),
        ("f:re\\:x", "f:re\\:x"),
        ("re:\\(", "\"re:\\(\""),
        ("card:0", "card:1"),
        ("card:+1", "card:1"),
        ("card:65536", "card:65536"),
        ("rated:0", "rated:1"),
        ("rated:2:+3", "rated:2:3"),
        ("resched:0", "resched:0"),
        ("prop:ease>1.0", "prop:ease>1"),
        ("prop:s>1.0", "prop:s>1"),
        ("prop:pos=+3", "prop:pos=3"),
        ("\"prop:cds:abc=foo bar\"", "\"prop:cds:abc=foo bar\""),
        ("dupe:1,a\\\\b", "dupe:1,a\\\\b"),
        ("-(a or b)", "-(a OR b)"),
        ("(a b)", "a b"),
        ("((a b))", "(a b)"),
        ("(a)", "a"),
        ("-(a b)", "-(a b)"),
        // F3.
        ("1 and 2 or 3", "1 2 OR 3"),
        ("a(b)", "a (b)"),
        ("\"x\"\"y\"", "x y"),
        ("a -and b", "a -  b"),
        ("-or", "- OR "),
        // F4.
        ("\"-\"", "-"),
        ("\"a-b\"", "a-b"),
        ("\\(", "\"(\""),
        ("\"aNd\"", "\"aNd\""),
        // F5.
        ("DECK:x", "deck:x"),
        ("flag:007", "flag:7"),
        ("added:0", "added:1"),
        ("nid:007", "nid:007"),
        ("mid:+5", "mid:5"),
        ("prop:due=-0", "prop:due=0"),
        ("prop:ivl>=+5", "prop:ivl>=5"),
        ("dupe:+1,a", "dupe:1,a"),
        ("has\\-cd:x", "has-cd:x"),
        ("preset:a\\\"b", "preset:a\\\\\"b"),
        // F6.
        ("prop:ease>1.", "prop:ease>1"),
        ("prop:ease>1E0", "prop:ease>1"),
        ("prop:ease>2.50", "prop:ease>2.5"),
        ("prop:ease>1e3", "prop:ease>1000"),
        ("prop:ease>+.5", "prop:ease>0.5"),
        ("prop:ease>0.1", "prop:ease>0.1"),
        ("prop:ease>0.3333333333", "prop:ease>0.33333334"),
        ("prop:ease>1e-10", "prop:ease>0.0000000001"),
        ("prop:ease>16777217", "prop:ease>16777216"),
        ("prop:ease>123456789", "prop:ease>123456790"),
        ("prop:ease>1e39", "prop:ease>inf"),
        ("prop:ease>1e400", "prop:ease>inf"),
        ("prop:ease>infinity", "prop:ease>inf"),
        ("prop:ease>-inf", "prop:ease>-inf"),
        ("prop:ease>nan", "prop:ease>NaN"),
        ("prop:ease>1e-50", "prop:ease>0"),
        ("prop:ease>-1e-50", "prop:ease>-0"),
        ("prop:ease>-0", "prop:ease>-0"),
    ];
    for (text, expected) in cases {
        assert_eq!(canonical(text), expected, "{text:?}");
    }
}

// F7's three shapes whose canonical text does not read back the same.
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
    // F2.2 removes one level of brackets per writing, so a search that is a
    // group holding a group shrinks again (not listed in F7).
    assert_eq!(canonical("((a))"), "(a)");
    assert_eq!(canonical("(a)"), "a");
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
            seq.pick(&["Default", "a\\x", "a\\-b", "a\\:b"])
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

/// F7's shapes, excluded from the round trip.
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

/// What a plan does, or why there is none; ranges and node ids are left
/// out, being positions in the source.
fn plan(query: &Query, mode: SearchMode) -> Result<String, String> {
    match Plan::build(query, mode, &context()) {
        Ok(plan) => Ok(format!(
            "{:?} {} {:?} {:?} {}",
            plan.rows(),
            plan.sql(),
            plan.params(),
            plan.reads(),
            plan.slow_terms().len()
        )),
        Err(BuildError::Search(e)) => Err(e.code().to_owned()),
        Err(BuildError::NotYetSupported { family, .. }) => Err(format!("{family:?}")),
    }
}

// R1077, F7: canonical(canonical(t)) = canonical(t), and canonical(t) means
// what t means: the same query, so the same plan and the same ids on any
// collection, in both modes.
#[test]
fn canonical_texts_round_trip() {
    let mut seq = Seq(0x00C0_FFEE);
    let mut checked = 0;
    let mut attempts = 0;
    while checked < 10_000 {
        attempts += 1;
        assert!(attempts < 200_000, "the generator yields too few searches that parse");
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
        // F2.2: only a search that is a group holding a group shrinks again.
        let twice = again.canonical_text();
        match query.node(meaning_root(&query)).kind {
            NodeKind::Group(_) if matches!(query.node(query.root()).kind, NodeKind::Group(_)) => {
                assert_eq!(format!("({twice})"), once, "{text:?}");
            }
            _ => assert_eq!(twice, once, "{text:?}"),
        }
        for mode in [SearchMode::Cards, SearchMode::Notes] {
            assert_eq!(plan(&query, mode), plan(&again, mode), "{text:?} -> {once:?}");
        }
        checked += 1;
    }
}
