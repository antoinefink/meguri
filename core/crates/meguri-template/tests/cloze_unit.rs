//! `specs/render/cloze.md` through the public cloze functions: the spec's
//! own examples and the plan's U61 test scenarios, as amended to the
//! oracle's behavior (A1 and A2, `docs/process/review-log/U61.md`).

use meguri_template::cloze::{self, Side};
use meguri_template::{CardSpecials, FieldMap, ParsedTemplate};

fn front(text: &str, card: u32) -> String {
    cloze::render(text, card, Side::Front)
}

fn back(text: &str, card: u32) -> String {
    cloze::render(text, card, Side::Back)
}

fn only_front(text: &str, card: u32) -> String {
    cloze::render_only(text, card, Side::Front)
}

fn only_back(text: &str, card: u32) -> String {
    cloze::render_only(text, card, Side::Back)
}

fn numbers(text: &str) -> Vec<u16> {
    cloze::numbers(text).into_iter().collect()
}

/// An active span on the back (C6 table).
fn shown(list: &str, content: &str) -> String {
    format!(r#"<span class="cloze" data-ordinal="{list}">{content}</span>"#)
}

/// An inactive span on either side (C6 table).
fn inactive(list: &str, content: &str) -> String {
    format!(r#"<span class="cloze-inactive" data-ordinal="{list}">{content}</span>"#)
}

/// An active span on the front, `encoded` already in C7's form.
fn hidden(list: &str, encoded: &str, hint: &str) -> String {
    format!(r#"<span class="cloze" data-cloze="{encoded}" data-ordinal="{list}">[{hint}]</span>"#)
}

// C2: opening markers.

#[test]
fn marker_numbers_follow_the_c2_table() {
    let cases: &[(&str, &[u16])] = &[
        ("{{c1::x}}", &[1]),
        ("{{c3,1,3,2::x}}", &[1, 2, 3]),
        ("{{c2,,5::x}}", &[2, 5]),
        ("{{c,1::x}}", &[1]),
        ("{{c1,::x}}", &[1]),
        ("{{c01::x}}", &[1]),
        ("{{c65535::x}}", &[65535]),
        ("{{c0,2::x}}", &[2]),
        ("{{c0::x}}", &[]),
        ("{{c65536::x}}", &[]),
        ("{{c99999::x}}", &[]),
        ("{{c::x}}", &[]),
        ("{{c,::x}}", &[]),
        ("{{c1 ::x}}", &[]),
        ("{{c1:x}}", &[]),
        ("{{C1::x}}", &[]),
        ("{{ c1::x}}", &[]),
        ("{c1::x}}", &[]),
    ];
    for (text, expected) in cases {
        assert_eq!(numbers(text), *expected, "numbers of {text:?}");
    }
}

#[test]
fn leading_zeros_do_not_reach_the_ordinal_list() {
    assert_eq!(back("{{c01::x}}", 1), shown("1", "x"));
    assert_eq!(back("{{c0,02::x}}", 2), shown("0,2", "x"));
}

#[test]
fn number_zero_opens_a_cloze_that_is_never_active() {
    assert_eq!(
        back("{{c0::a}}{{c1::b}}", 1),
        format!("{}{}", inactive("0", "a"), shown("1", "b"))
    );
    assert_eq!(front("{{c0::x}}", 1), "");
    assert_eq!(back("{{c0::x}}", 1), "");
}

#[test]
fn a_failed_marker_stays_text() {
    // The marker fails, so its closer has no cloze to close and stays text.
    assert_eq!(
        back("{{c65536::x}} {{c1::y}}", 1),
        format!("{{{{c65536::x}}}} {}", shown("1", "y"))
    );
    assert_eq!(
        back("{{c::a}} {{c1::b}}", 1),
        format!("{{{{c::a}}}} {}", shown("1", "b"))
    );
    assert_eq!(
        back("{{c1:a}} {{c1::b}}", 1),
        format!("{{{{c1:a}}}} {}", shown("1", "b"))
    );
}

// C3: tokens.

#[test]
fn extra_braces_split_as_c3_says() {
    assert_eq!(back("{{c1::x}}}", 1), format!("{}}}", shown("1", "x")));
    assert_eq!(back("{{{c1::x}}", 1), format!("{{{}", shown("1", "x")));
}

// C4: the tree.

#[test]
fn hints_split_at_the_first_double_colon() {
    assert_eq!(front("{{c1::Paris::capital}}", 1), hidden("1", "Paris", "capital"));
    assert_eq!(front("{{c1::a::b::c}}", 1), hidden("1", "a", "b::c"));
    assert_eq!(front("{{c1::a::}}", 1), hidden("1", "a", ""));
}

#[test]
fn a_later_run_replaces_the_hint() {
    let text = "{{c1::x::h1 {{c2::y}} z::h2}}";
    assert_eq!(only_front(text, 1), "h2");
    assert_eq!(only_back(text, 1), "xy z");
    assert_eq!(back(text, 1), shown("1", &format!("x{} z", inactive("2", "y"))));

    let text = "{{c1::x::h{{c2::y}}z}}";
    assert_eq!(only_front(text, 1), "h");
    assert_eq!(back(text, 1), shown("1", &format!("x{}z", inactive("2", "y"))));
}

#[test]
fn a_hint_after_a_nested_cloze_leaves_an_empty_child() {
    let text = "{{c1::{{c2::y}}::h}}";
    assert_eq!(only_front(text, 1), "h");
    assert_eq!(back(text, 1), shown("1", &inactive("2", "y")));
}

#[test]
fn a_nested_cloze_keeps_its_own_hint() {
    let text = "{{c1::a {{c2::b::hb}} c}}";
    assert_eq!(only_front(text, 2), "hb");
    assert_eq!(only_front(text, 1), "...");
    assert_eq!(back(text, 2), inactive("1", &format!("a {} c", shown("2", "b"))));
}

#[test]
fn a_stray_closer_is_top_level_text() {
    assert_eq!(numbers("a }} b"), [0u16; 0]);
    assert_eq!(back("a }} b {{c1::x}}", 1), format!("a }}}} b {}", shown("1", "x")));
}

#[test]
fn unclosed_clozes_are_discarded_with_their_contents() {
    assert_eq!(numbers("a {{c1::b {{c2::c}} d"), [0u16; 0]);
    assert_eq!(back("a {{c1::b {{c2::c}} d", 1), "");
    assert_eq!(
        back("{{c3::z}} a {{c1::b {{c2::c}} d", 3),
        format!("{} a ", shown("3", "z"))
    );
    assert_eq!(back("{{c1::a}} {{c2::b", 1), format!("{} ", shown("1", "a")));
    assert_eq!(back("{{c1::a}} {{c2::b", 2), "");
}

/// Plan (R972): ten nested levels, an eleventh dropped, a stray `}}`.
#[test]
fn the_eleventh_open_marker_is_dropped() {
    let mut text = String::new();
    for n in 1..=11 {
        text.push_str(&format!("{{{{c{n}::"));
    }
    text.push('x');
    text.push_str(&"}}".repeat(11));
    assert_eq!(numbers(&text), (1..=10).collect::<Vec<u16>>());

    // C4: x is a child of cloze 10, the eleventh `}}` is top-level text.
    let mut expected = shown("10", "x");
    for n in (1..=9).rev() {
        expected = inactive(&n.to_string(), &expected);
    }
    expected.push_str("}}");
    assert_eq!(back(&text, 10), expected);
    assert_eq!(back(&text, 11), "");
}

#[test]
fn a_dropped_marker_leaves_no_text_and_splits_the_run() {
    let mut text = String::new();
    for n in 1..=10 {
        text.push_str(&format!("{{{{c{n}::"));
    }
    text.push_str("a::h{{c11::b::g");
    text.push_str(&"}}".repeat(10));
    // Cloze 10 gets children `a` and `b`; `g` replaces `h` as its hint.
    assert_eq!(only_front(&text, 10), "g");
    assert_eq!(only_back(&text, 10), "ab");
}

// C5: numbers and top-level presence.

#[test]
fn cloze_numbers_follow_the_c5_table() {
    assert_eq!(numbers("{{c1::a}}{{c3::b}}{{c0::l}}{{c700::z}}"), [1, 3, 700]);
    assert_eq!(numbers("{{c2::ab{{c1::cd}}}}e{{"), [1, 2]);
    assert_eq!(numbers("{{c0,3,4::x}}"), [3, 4]);
    assert_eq!(numbers("{{c1::open"), [0u16; 0]);
    assert_eq!(numbers("ÄÖÜ öäü"), [0u16; 0]);
    assert_eq!(numbers(""), [0u16; 0]);
}

#[test]
fn top_level_presence_ignores_nested_clozes() {
    assert_eq!(numbers("{{c0::a {{c1::b}}}}"), [1]);
    assert!(!cloze::has_cloze("{{c0::a {{c1::b}}}}"));
    assert!(cloze::has_cloze("{{c1::a}}"));
    assert!(cloze::has_cloze("x {{c0,2::y}}"));
    assert!(!cloze::has_cloze("{{c0::a}}"));
    assert!(!cloze::has_cloze("x {{c2::y"));
    assert!(!cloze::has_cloze("plain"));
}

// C15: the blank check of a cloze note type (template-syntax.md T10) looks
// at every value of the card's field map, special values included.

/// The specials of a card not yet added, with the parts C15 varies.
struct Specials<'a> {
    notetype: &'a str,
    template: &'a str,
    deck: Option<&'a str>,
    tags: &'a [String],
}

const PLAIN: Specials<'static> = Specials {
    notetype: "Gap",
    template: "Cloze",
    deck: None,
    tags: &[],
};

fn blank(fields: &[(&str, &str)], card: u32, specials: &Specials<'_>) -> bool {
    let specials = CardSpecials {
        ord: card - 1,
        notetype_name: specials.notetype,
        template_name: specials.template,
        deck_name: specials.deck,
        flags: 0,
        card_id: 0,
        tags: specials.tags,
    };
    FieldMap::new(fields.iter().copied(), &specials).is_blank_cloze_card()
}

#[test]
fn a_cloze_card_is_blank_without_its_number_in_any_field() {
    let text = |value| [("Text", value), ("Extra", "")];
    assert!(blank(&text("{{c1::a}} {{c2::b}}"), 3, &PLAIN));
    assert!(!blank(&text("{{c1::a}} {{c2::b}}"), 2, &PLAIN));
    // A field no template uses counts too: the templates play no part.
    assert!(!blank(&[("Text", "plain"), ("Extra", "{{c2::x}}")], 2, &PLAIN));
    assert!(blank(&text("no clozes"), 1, &PLAIN));
    // C5's numbers: nested clozes count; 0, unclosed clozes and markers that
    // are not markers count for nothing.
    assert!(!blank(&text("{{c0::a {{c2::b}}}}"), 2, &PLAIN));
    assert!(blank(&text("{{c1::open"), 1, &PLAIN));
    assert!(blank(&text("{{c0::label}}"), 1, &PLAIN));
    assert!(blank(&text("{{C1::x}} {{c1:x}} {c1::x}"), 1, &PLAIN));
    assert!(!blank(&text("{{c1,3::x}}"), 3, &PLAIN));
    assert!(!blank(&text("{{c500::x}}"), 500, &PLAIN));
    assert!(!blank(&text("{{c65535::x}}"), 65535, &PLAIN));
    assert!(blank(&[], 1, &PLAIN));
}

#[test]
fn special_values_count_unless_a_field_replaces_them() {
    let fields = [("Text", "{{c1::a}}")];
    let tags = ["{{c3::t}}".to_owned()];
    let tagged = Specials { tags: &tags, ..PLAIN };
    assert!(!blank(&fields, 3, &tagged));
    let named = Specials {
        template: "{{c4::card}}",
        ..PLAIN
    };
    assert!(!blank(&fields, 4, &named));
    let typed = Specials {
        notetype: "Gap {{c5::type}}",
        ..PLAIN
    };
    assert!(!blank(&fields, 5, &typed));
    assert!(blank(&fields, 6, &typed));
    let decked = Specials {
        deck: Some("Lang::{{c6::a}}"),
        ..PLAIN
    };
    assert!(!blank(&fields, 6, &decked));
    // A note field named like a special replaces it.
    assert!(blank(&[("Text", "{{c1::a}}"), ("Tags", "")], 3, &tagged));
    let tags = ["t".to_owned()];
    let plain_tags = Specials { tags: &tags, ..PLAIN };
    assert!(!blank(&[("Text", "{{c1::a}}"), ("Tags", "{{c3::f}}")], 3, &plain_tags));
    assert!(blank(&[("Text", "{{c1::a}}"), ("Card", "")], 4, &named));
    // `c<N>` is `1`, CardFlag `flag0`, CardID `0`: no cloze in any.
    assert!(blank(&fields, 2, &PLAIN));
}

// C6: the `cloze:` filter.

#[test]
fn single_clozes_render_as_the_c6_examples() {
    let text = "{{c1::Paris::capital}} is in {{c2::France}}";
    assert_eq!(
        front(text, 1),
        format!("{} is in {}", hidden("1", "Paris", "capital"), inactive("2", "France"))
    );
    assert_eq!(
        back(text, 1),
        format!("{} is in {}", shown("1", "Paris"), inactive("2", "France"))
    );
    assert_eq!(front("{{c1,2::x}}", 1), hidden("1,2", "x", "..."));
    assert_eq!(back("{{c1,2::x}}", 1), shown("1,2", "x"));
    assert_eq!(front("{{c1::}}", 1), hidden("1", "", "..."));
    assert_eq!(back("{{c1::}}", 1), shown("1", ""));
}

#[test]
fn nested_clozes_render_as_the_c6_examples() {
    let text = "{{c2::a {{c1::b}}}}";
    assert_eq!(front(text, 1), inactive("2", &format!("a {}", hidden("1", "b", "..."))));
    assert_eq!(back(text, 1), inactive("2", &format!("a {}", shown("1", "b"))));

    let text = "{{c1::a {{c2::b}}}}";
    assert_eq!(
        front(text, 1),
        hidden(
            "1",
            "a&#x20;&lt;span&#x20;class&#x3D;&quot;cloze&#x2D;inactive&quot;&#x20;data&#x2D;ordinal&#x3D;&quot;2&quot;&gt;b&lt;&#x2F;span&gt;",
            "..."
        )
    );
    assert_eq!(back(text, 1), shown("1", &format!("a {}", inactive("2", "b"))));
}

#[test]
fn no_active_cloze_renders_nothing() {
    assert_eq!(front("{{c2::x}}", 1), "");
    assert_eq!(back("{{c2::x}}", 1), "");
    assert_eq!(front("no cloze", 1), "");
    assert_eq!(front("", 1), "");
    // Plan (R975): `{{cloze:Extra}}` where Extra has no c2, on card 2.
    assert_eq!(front("{{c1::note}} and more", 2), "");
    assert_eq!(back("{{c1::note}} and more", 2), "");
}

/// Plan (R969, R974): the hint on the front, `data-cloze` encoded, c2
/// inactive on both sides.
#[test]
fn plan_hint_and_inactive_scenario() {
    let text = "{{c1::a::hint}} {{c2::b}}";
    assert_eq!(
        front(text, 1),
        format!("{} {}", hidden("1", "a", "hint"), inactive("2", "b"))
    );
    assert_eq!(back(text, 1), format!("{} {}", shown("1", "a"), inactive("2", "b")));
}

/// Plan (R970): one cloze on cards 1, 2 and 3.
#[test]
fn plan_multi_number_scenario() {
    let text = "{{c1,3,3,2::x}}";
    for card in 1..=3 {
        assert_eq!(front(text, card), hidden("1,2,3", "x", "..."));
        assert_eq!(back(text, card), shown("1,2,3", "x"));
    }
    assert_eq!(front(text, 4), "");
}

/// Plan (R971): c1 used three times hides all three.
#[test]
fn plan_repeated_number_scenario() {
    let text = "{{c1::a}} {{c1::b}} {{c1::c}}";
    assert_eq!(
        front(text, 1),
        format!(
            "{} {} {}",
            hidden("1", "a", "..."),
            hidden("1", "b", "..."),
            hidden("1", "c", "...")
        )
    );
}

#[test]
fn the_hint_is_written_as_is() {
    assert_eq!(front(r#"{{c1::x::a<b>"&}}"#, 1), hidden("1", "x", r#"a<b>"&"#));
}

// C7: attribute encoding.

#[test]
fn data_cloze_encodes_as_the_c7_table() {
    let cases = [
        ("\"", "&quot;"),
        ("&", "&amp;"),
        ("'", "&#x27;"),
        ("<", "&lt;"),
        (">", "&gt;"),
        ("AZaz09", "AZaz09"),
        (" ", "&#x20;"),
        ("\n", "&#x0A;"),
        ("\t", "&#x09;"),
        ("\r", "&#x0D;"),
        ("\0", "&#x00;"),
        ("\\", "&#x5C;"),
        ("(", "&#x28;"),
        ("[", "&#x5B;"),
        (".", "&#x2E;"),
        ("=", "&#x3D;"),
        ("-", "&#x2D;"),
        ("/", "&#x2F;"),
        ("\u{7f}", "&#x7F;"),
        ("\u{80}", "&#x80;"),
        ("é", "&#xE9;"),
        ("\u{a0}", "&#xA0;"),
        ("\u{ff}", "&#xFF;"),
        ("\u{100}", "\u{100}"),
        ("日本😀€\u{2028}", "日本😀€\u{2028}"),
    ];
    for (content, encoded) in cases {
        let text = format!("{{{{c1::{content}}}}}");
        assert_eq!(front(&text, 1), hidden("1", encoded, "..."), "content {content:?}");
    }
}

#[test]
fn a_sound_tag_inside_a_hidden_cloze_is_no_sound_tag() {
    assert_eq!(
        front("{{c1::[sound:a.mp3]}}", 1),
        hidden("1", "&#x5B;sound&#x3A;a&#x2E;mp3&#x5D;", "...")
    );
    assert_eq!(back("{{c1::[sound:a.mp3]}}", 1), shown("1", "[sound:a.mp3]"));
}

// C8: MathJax.

#[test]
fn spans_inside_math_lose_their_tags() {
    let text = r"\(a {{c1::b}} c\)";
    assert_eq!(front(text, 1), r"\(a [...] c\)");
    assert_eq!(back(text, 1), r"\(a b c\)");
}

/// R973 as amended (A1; C8, C13): `}}` inside math closes the cloze.
#[test]
fn math_does_not_shield_the_closer() {
    let text = r"{{c1::\(a}}b\)}}";
    assert_eq!(back(text, 1), r#"<span class="cloze" data-ordinal="1">\(ab\)}}"#);
    assert_eq!(
        front(text, 1),
        format!("{}b\\)}}}}", hidden("1", "&#x5C;&#x28;a", "..."))
    );
    assert_eq!(
        back(r"{{c2::\(\frac{1}{x^{2}}\)}}", 2),
        r#"<span class="cloze" data-ordinal="2">\(\frac{1}{x^{2\)}}"#
    );
}

#[test]
fn math_matches_follow_c8() {
    let y = shown("1", "y");
    let cases = [
        // Entities stay, a `<` with no `>` stays.
        (r"{{c1::y}}\(a &lt; b\)", format!(r"{y}\(a &lt; b\)")),
        (r"{{c1::y}}\(a < b\)", format!(r"{y}\(a < b\)")),
        (r"{{c1::y}}\(<b>x</b>\)", format!(r"{y}\(x\)")),
        // Opener and closer need not correspond.
        (r"{{c1::y}}\(<i>z</i>\]", format!(r"{y}\(z\]")),
        (r"{{c1::y}}\[<i>z</i>\)", format!(r"{y}\[z\)")),
        // The match starts at the second backslash.
        (r"{{c1::y}}\\(<i>x</i>\)", format!(r"{y}\\(x\)")),
        // No closer: no match.
        (r"{{c1::y}}\(<i>x</i>", format!(r"{y}\(<i>x</i>")),
        // Shortest run, then scanning resumes after the closer.
        (
            r"{{c1::y}}\(<i>a</i>\) <b>b</b> \[<i>c</i>\]",
            format!(r"{y}\(a\) <b>b</b> \[c\]"),
        ),
        // An empty run.
        (r"{{c1::y}}\(\)", format!(r"{y}\(\)")),
    ];
    for (text, expected) in cases {
        assert_eq!(back(text, 1), expected, "text {text:?}");
    }
}

#[test]
fn only_the_cloze_filter_runs_the_math_pass() {
    let text = r"{{c1::\(<b>x</b>\)}}";
    assert_eq!(only_back(text, 1), r"\(<b>x</b>\)");
    assert_eq!(cloze::type_answer(text, 1), r"\(<b>x</b>\)");
}

// C9: `cloze-only:`.

#[test]
fn cloze_only_follows_the_c9_table() {
    let text = "{{c1::Paris::capital}} and {{c1::Rome}}";
    assert_eq!(only_front(text, 1), "capital, ...");
    assert_eq!(only_back(text, 1), "Paris, Rome");

    let text = "{{c1,2::Seine}} and {{c1::Loire}} or {{c2::Rhône}}";
    assert_eq!(only_front(text, 2), "..., ...");
    assert_eq!(only_back(text, 2), "Seine, Rhône");

    let text = "{{c1::a {{c1::b}}}}";
    assert_eq!(only_front(text, 1), "..., ...");
    assert_eq!(only_back(text, 1), "a b, b");

    assert_eq!(only_front("no cloze here", 1), "");
    assert_eq!(only_back("no cloze here", 1), "");
}

#[test]
fn cloze_only_keeps_an_empty_hint_entry_and_writes_html_as_is() {
    assert_eq!(only_front("{{c1::a::}} {{c1::b}}", 1), ", ...");
    assert_eq!(only_back("{{c1::<b>a & b</b>}}", 1), "<b>a & b</b>");
    // A nested active cloze inside an inactive one still counts.
    assert_eq!(only_back("{{c2::a {{c1::b}}}}", 1), "b");
}

// filters.md F8 through the template engine: the card number is the
// ordinal plus one (C1), the question template renders the front and the
// answer template the back, and `cloze` never ends a chain (F1).

/// A card not yet added at ordinal `ord`, with these note fields.
fn card(fields: &[(&str, &str)], ord: u32) -> FieldMap {
    let name = format!("Card {}", ord + 1);
    let specials = CardSpecials {
        ord,
        notetype_name: "Probe",
        template_name: &name,
        deck_name: None,
        flags: 0,
        card_id: 0,
        tags: &[],
    };
    FieldMap::new(fields.iter().copied(), &specials)
}

#[test]
fn the_engine_renders_cloze_filters_for_the_card_and_side() {
    let fields = card(&[("Text", "{{c1::a::h}} {{c2::b}}"), ("Extra", "{{c1::e}}")], 1);
    let template = ParsedTemplate::parse("{{cloze:Text}}|{{cloze:Extra}}|{{cloze-only:Text}}");
    // R975: Extra has no c2, so it renders nothing on card 2.
    assert_eq!(
        template.render(&fields, meguri_template::Side::Question).unwrap(),
        format!("{} {}||...", inactive("1", "a"), hidden("2", "b", "...")),
    );
    let answer = meguri_template::Side::Answer { front_side: "" };
    assert_eq!(
        template.render(&fields, answer).unwrap(),
        format!("{} {}||b", inactive("1", "a"), shown("2", "b")),
    );
    let chains = ParsedTemplate::parse("{{text:cloze:Text}}|{{edit:cloze:Text}}|{{cloze:edit:Text}}");
    assert_eq!(
        chains.render(&fields, answer).unwrap(),
        format!(
            "a b|{} {}|{{{{c1::a::h}}}} {{{{c2::b}}}}",
            inactive("1", "a"),
            shown("2", "b")
        ),
    );
}

// C10: type-in extraction.

#[test]
fn type_answers_follow_the_c10_table() {
    let cases = [
        ("{{c1::a}} {{c1::a}}", 1, "a"),
        // R977 as amended (A2; C13): duplicates stay unless all are identical.
        ("{{c1::a}} {{c1::a}} {{c1::b}}", 1, "a, a, b"),
        ("{{c1::a  }} {{c1::a}}", 1, "a  , a"),
        ("{{c1::x {{c1::x}}}}", 1, "x x, x"),
        ("{{c2::a}}", 1, ""),
        ("{{c1::<b>Paris</b>}}", 1, "<b>Paris</b>"),
        ("{{c1::Paris::capital}}", 1, "Paris"),
    ];
    for (text, card, expected) in cases {
        assert_eq!(cloze::type_answer(text, card), expected, "text {text:?} card {card}");
    }
}

// C11: image-occlusion clozes.

#[test]
fn image_occlusion_clozes_are_ordinary_outside_the_cloze_filter() {
    let text = "{{c3::image-occlusion:rect:left=.1:top=.2:width=.3:height=.4}}";
    assert_eq!(numbers(text), [3]);
    assert_eq!(
        only_back(text, 3),
        "image-occlusion:rect:left=.1:top=.2:width=.3:height=.4"
    );
    assert_eq!(only_front(text, 3), "...");
    // No hint splitting in an image-occlusion run.
    assert_eq!(only_front("{{c1::image-occlusion:rect::x}}", 1), "...");
    assert_eq!(
        only_back("{{c1::image-occlusion:rect::x}}", 1),
        "image-occlusion:rect::x"
    );
    assert_eq!(
        cloze::type_answer("{{c1::image-occlusion:rect::x}}", 1),
        "image-occlusion:rect::x"
    );
}

#[test]
fn clozes_inside_an_image_occlusion_cloze_do_not_make_the_output() {
    // C6 step 1 does not consult cloze 2, nested in an image-occlusion cloze.
    assert_eq!(front("{{c1::image-occlusion:rect {{c2::y}}}}", 2), "");
    assert_eq!(back("{{c1::image-occlusion:rect {{c2::y}}}}", 2), "");
    // C5 still counts it.
    assert_eq!(numbers("{{c1::image-occlusion:rect {{c2::y}}}}"), [1, 2]);
}

// C12: robustness, and the span balance that holds before C8.

/// Every sequence of up to five pieces from a small alphabet of markers and
/// text: no function panics, the C6 output has balanced spans when C8 has
/// nothing to match, and outputs agree with the cloze numbers.
#[test]
fn small_texts_keep_the_c12_properties() {
    const PIECES: [&str; 9] = ["{{c1::", "{{c2,1::", "{{c0::", "}}", "::", "a", "<b>", "\\(", "\\)"];
    let mut indices = vec![0usize; 0];
    let mut checked = 0usize;
    loop {
        let text: String = indices.iter().map(|&i| PIECES[i]).collect();
        check_properties(&text);
        checked += 1;
        // Next sequence in length-then-lexicographic order.
        let mut position = indices.len();
        loop {
            if position == 0 {
                if indices.len() == 5 {
                    assert_eq!(checked, (0..=5).map(|n| PIECES.len().pow(n)).sum::<usize>());
                    return;
                }
                indices = vec![0; indices.len() + 1];
                break;
            }
            position -= 1;
            indices[position] += 1;
            if indices[position] < PIECES.len() {
                break;
            }
            indices[position] = 0;
        }
    }
}

fn check_properties(text: &str) {
    let numbers = cloze::numbers(text);
    for card in 1..=3u32 {
        let active = numbers.iter().any(|&n| u32::from(n) == card);
        for side in [Side::Front, Side::Back] {
            let rendered = cloze::render(text, card, side);
            let only = cloze::render_only(text, card, side);
            if !active {
                assert_eq!(rendered, "", "{text:?} card {card}");
                assert_eq!(only, "", "{text:?} card {card}");
            }
            if !text.contains("\\(") && !text.contains("\\[") {
                assert_eq!(
                    rendered.matches("<span ").count(),
                    rendered.matches("</span>").count(),
                    "{text:?} card {card} {side:?}: {rendered:?}"
                );
            }
        }
        let typed = cloze::type_answer(text, card);
        if !active {
            assert_eq!(typed, "", "{text:?} card {card}");
        }
    }
}
