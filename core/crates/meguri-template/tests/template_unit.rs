//! The template engine through its public API (U60). Expected values come
//! from the examples and tables of specs/render/template-syntax.md,
//! filters.md and special-fields.md; the oracle comparison of every render
//! scenario is T0 (meguri-testkit's `template` adapter).

use std::fs;
use std::path::{Path, PathBuf};

use meguri_template::{
    CardSpecials, ErrorKind, FieldMap, Form, NodeKind, ParsedTemplate, RequirementKind, Side, legacy_requirement,
};

/// A card not yet added (special-fields.md P3), ordinal `ord`, with these
/// note fields and tags.
fn new_card(fields: &[(&str, &str)], ord: u32, tags: &[&str]) -> FieldMap {
    let tags: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
    let name = format!("Card {}", ord + 1);
    let specials = CardSpecials {
        ord,
        notetype_name: "Probe",
        template_name: &name,
        deck_name: None,
        flags: 0,
        card_id: 0,
        tags: &tags,
    };
    FieldMap::new(fields.iter().copied(), &specials)
}

fn question(template: &str, fields: &FieldMap) -> Result<String, ErrorKind> {
    ParsedTemplate::parse(template)
        .render(fields, Side::Question)
        .map_err(|e| e.kind)
}

fn answer(template: &str, fields: &FieldMap, front_side: &str) -> Result<String, ErrorKind> {
    ParsedTemplate::parse(template)
        .render(fields, Side::Answer { front_side })
        .map_err(|e| e.kind)
}

fn front(value: &str) -> FieldMap {
    new_card(&[("Front", value), ("Back", "back")], 0, &[])
}

fn ok(s: &str) -> Result<String, ErrorKind> {
    Ok(s.to_owned())
}

// T3, T4, T14: R953's wording differs from the oracle for the triple brace.
#[test]
fn spaces_and_extra_opening_braces_name_the_same_field() {
    let f = front("front");
    assert_eq!(question("{{Front}}", &f), ok("front"));
    assert_eq!(question("{{ Front }}", &f), ok("front"));
    assert_eq!(question("{{\t Front\n}}", &f), ok("front"));
    assert_eq!(question("{{\u{3000}Front }}", &f), ok("front"));
    // The closing brace beyond `}}` is text.
    assert_eq!(question("{{{Front}}}", &f), ok("front}"));
    assert_eq!(question("{{{{Front}}}}", &f), ok("front}}"));
    // Only braces at the very start go: ` {Front` keeps its brace.
    assert_eq!(question("{{ {Front}}", &f), Err(ErrorKind::UnknownField));
}

#[test]
fn tokens_follow_the_first_closing_delimiter() {
    let f = front("front");
    assert_eq!(question("{{Back}} and {{Front", &f), ok("back and {{Front"));
    assert_eq!(question("text }} more {{Front}} {", &f), ok("text }} more front {"));
    // One tag with content `a{{Back`.
    assert_eq!(question("{{Front}}{{a{{Back}}", &f), Err(ErrorKind::UnknownField));
    // The tag `Front <!--` wins: it starts first.
    assert_eq!(question("{{Front <!-- }} -->", &f), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{Front}}{{a}b}}", &f), Err(ErrorKind::UnknownField));
}

// T4 step 3: names shorter than two bytes are replacements.
#[test]
fn one_character_tags_are_replacements() {
    let f = front("front");
    for template in [
        "{{Front}}{{}}",
        "{{Front}}{{ }}",
        "{{Front}}{{#}}",
        "{{Front}}{{/}}",
        "{{Front}}{{^}}",
    ] {
        assert_eq!(question(template, &f), Err(ErrorKind::UnknownField), "{template}");
    }
    let named = new_card(&[("#", "hash"), ("Front", "f")], 0, &[]);
    assert_eq!(question("{{#}}{{Front}}", &named), ok("hashf"));
}

#[test]
fn section_keys_lose_white_space_at_their_start() {
    let f = front("front");
    assert_eq!(question("{{ #Back }}shown{{/Back}}{{Front}}", &f), ok("shownfront"));
    assert_eq!(
        question("{{#  Back}}a{{/ Back}}{{^Back }}b{{/Back}}{{Front}}", &f),
        ok("afront")
    );
    // The key is not split on `:`.
    assert_eq!(
        question("{{Front}}{{#text:Front}}x{{/text:Front}}", &f),
        Err(ErrorKind::UnknownConditional)
    );
}

// T5, R956.
#[test]
fn filters_apply_nearest_the_key_first_and_empty_names_change_nothing() {
    let f = new_card(&[("Key", "k"), ("My Field", "日本[にほん]")], 0, &[]);
    // kanji first leaves no ruby group for furigana.
    assert_eq!(question("{{furigana:kanji:My Field}}", &f), ok("日本"));
    assert_eq!(
        question("{{kanji:furigana:My Field}}", &f),
        ok("<ruby><rb>日本</rb><rt>にほん</rt></ruby>")
    );
    let g = front("<b>x</b>");
    assert_eq!(question("{{::Front}}", &g), ok("<b>x</b>"));
    assert_eq!(question("{{:Front}}", &g), ok("<b>x</b>"));
    assert_eq!(question("{{text::Front}}", &g), ok("x"));
    // Pieces are not trimmed: the field ` Front` does not exist.
    assert_eq!(question("{{text: Front}}", &g), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{ text:Front }}", &g), ok("x"));
}

// T1: exact code points, no case folding or normalization.
#[test]
fn field_names_match_exactly() {
    let f = new_card(&[("Front", "f"), ("Ünï", "u")], 0, &[]);
    assert_eq!(question("{{front}}", &f), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{FRONT}}", &f), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{Ünï}}", &f), ok("u"));
    // The name typed in decomposed form.
    assert_eq!(question("{{U\u{308}ni\u{308}}}", &f), Err(ErrorKind::UnknownField));
}

// T7's table and E1.
#[test]
fn nested_sections_across_empty_and_non_empty_fields() {
    let t = "{{#A}}{{^B}}x{{/B}}{{/A}}";
    for (a, b, out) in [("", "", ""), ("", "b", ""), ("a", "", "x"), ("a", "b", "")] {
        let f = new_card(&[("A", a), ("B", b)], 0, &[]);
        assert_eq!(question(t, &f), ok(out), "A={a:?} B={b:?}");
    }
    let t = "{{#A}}Y{{/A}}{{^A}}N{{/A}}";
    for (value, out) in [
        ("<br>", "N"),
        (" <div></div> ", "N"),
        ("<BR/><br /></br />", "N"),
        ("\t\n\r\u{b}\u{c}", "N"),
        ("&nbsp;", "Y"),
        ("<span></span>", "Y"),
        ("\u{a0}", "Y"),
        ("<br  >", "Y"),
        ("<!-- -->", "Y"),
        // Given values are not save-normalized.
        ("<br>\u{1}", "Y"),
        ("0", "Y"),
    ] {
        let f = new_card(&[("A", value)], 0, &[]);
        assert_eq!(question(t, &f), ok(out), "{value:?}");
    }
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    const DEPTH: usize = 200_000;
    let template = format!("{}{{{{A}}}}{}", "{{#A}}".repeat(DEPTH), "{{/A}}".repeat(DEPTH));
    let f = new_card(&[("A", "a")], 0, &[]);
    assert_eq!(question(&template, &f), ok("a"));
    let unclosed = "{{#A}}".repeat(DEPTH);
    assert_eq!(question(&unclosed, &f), Err(ErrorKind::UnclosedConditional));
}

// T7 and special-fields.md P2: `c<ordinal+1>` is in the map; other
// card-number keys are empty without error.
#[test]
fn card_number_sections_show_only_on_their_card() {
    let t = "{{#c2}}two{{/c2}}{{^c2}}not two{{/c2}}";
    assert_eq!(question(t, &new_card(&[("F", "f")], 1, &[])), ok("two"));
    assert_eq!(question(t, &new_card(&[("F", "f")], 0, &[])), ok("not two"));
    let first = new_card(&[("F", "f")], 0, &[]);
    assert_eq!(
        question(
            "{{#c0}}a{{/c0}}{{#c01}}b{{/c01}}{{#c+1}}c{{/c+1}}{{#c4294967295}}d{{/c4294967295}}{{^c+1}}e{{/c+1}}",
            &first
        ),
        ok("e")
    );
    for key in ["C1", "c", "c1x", "c4294967296", "c-1", "c++1", "FrontSide"] {
        let t = format!("{{{{#{key}}}}}x{{{{/{key}}}}}");
        assert_eq!(question(&t, &first), Err(ErrorKind::UnknownConditional), "{key}");
    }
    assert_eq!(question("{{c1}}", &first), ok("1"));
    assert_eq!(question("{{c2}}", &first), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{c500}}", &new_card(&[("F", "f")], 499, &[])), ok("1"));
}

// T3, T7, R957.
#[test]
fn comments_are_output_verbatim_and_never_evaluated() {
    let f = front("front");
    assert_eq!(
        question("{{Front}}<!-- {{#Missing}} -->", &f),
        ok("front<!-- {{#Missing}} -->")
    );
    assert_eq!(
        question("<!--{{#A}}{{/B}}{{Nope}}-->{{Front}}", &f),
        ok("<!--{{#A}}{{/B}}{{Nope}}-->front")
    );
    assert_eq!(
        question("{{Front}}<!---->|<!-- a --> --> ", &f),
        ok("front<!---->|<!-- a --> --> ")
    );
    // No `-->` after the opening four characters: text.
    assert_eq!(question("{{Front}}<!-->x", &f), ok("front<!-->x"));
    // An unterminated opener is text, and the tag after it counts.
    assert_eq!(question("{{Front}}<!-- {{Back}}", &f), ok("front<!-- back"));
    assert_eq!(
        question("{{Front}}<!--{{#Back}}-->{{/Back}}", &f),
        Err(ErrorKind::UnopenedConditional)
    );
}

// T2, R958.
#[test]
fn the_alternate_directive_switches_delimiters_at_the_start_only() {
    let f = front("front");
    assert_eq!(question("{{=<% %>=}}<%Front%>{{Front}}", &f), ok("front{{Front}}"));
    assert_eq!(question("  \n\t{{=<% %>=}}<% Front %>", &f), ok("front"));
    assert_eq!(
        question("{{=<% %>=}}{{=<% %>=}}<%{Front%>|<%Back%>", &f),
        ok("front|back")
    );
    // The space stops the removal; the second directive holds the tag `<% %>`.
    assert_eq!(
        question("{{=<% %>=}} {{=<% %>=}}<%Front%>", &f),
        Err(ErrorKind::UnknownField)
    );
    assert_eq!(question("x{{=<% %>=}}<%Front%>", &f), Err(ErrorKind::UnknownField));
    assert_eq!(question("{{=<%  %>=}}<%Front%>", &f), Err(ErrorKind::UnknownField));
    assert_eq!(
        question("{{=<% %>=}}<!--<%Nope%>--><%Front%>", &f),
        ok("<!--<%Nope%>-->front")
    );
    // Per template text: the answer is in brace mode.
    assert_eq!(
        answer("{{FrontSide}}|{{Back}}|<%Back%>", &f, "Q"),
        ok("Q|back|<%Back%>")
    );
}

// T6, T8, T9.
#[test]
fn errors_have_kinds_and_structure_comes_first() {
    let f = front("front");
    let cases = [
        ("{{Front", Ok("{{Front")),
        ("{{Front}}{{#Back}}x", Err(ErrorKind::UnclosedConditional)),
        ("{{Front}}{{#Back}}x{{/Front}}", Err(ErrorKind::MismatchedConditional)),
        (
            "{{#Front}}{{#Back}}x{{/Front}}{{/Back}}",
            Err(ErrorKind::MismatchedConditional),
        ),
        ("{{Front}}{{/Back}}", Err(ErrorKind::UnopenedConditional)),
        ("{{Front}}{{/Back}}{{#Back}}", Err(ErrorKind::UnopenedConditional)),
        ("{{Front}}{{Nope}}", Err(ErrorKind::UnknownField)),
        ("{{Front}}{{#Nope}}x{{/Nope}}", Err(ErrorKind::UnknownConditional)),
        // A structural error anywhere beats an earlier render error.
        ("{{Nope}}{{#Back}}", Err(ErrorKind::UnclosedConditional)),
        // Render errors: the first in template order.
        ("{{Back}}{{#Nope}}{{/Nope}}{{Zip}}", Err(ErrorKind::UnknownConditional)),
        ("{{Back}}{{Zip}}{{#Nope}}{{/Nope}}", Err(ErrorKind::UnknownField)),
        // Hidden sections are still checked.
        (
            "{{Back}}{{^Back}}{{#Nope}}{{/Nope}}{{/Back}}",
            Err(ErrorKind::UnknownConditional),
        ),
    ];
    for (template, expected) in cases {
        let expected = expected.map(str::to_owned);
        assert_eq!(question(template, &f), expected, "{template}");
    }
    let parsed = ParsedTemplate::parse("{{#A}}x{{/B}}");
    let error = parsed.structure_error().expect("a structural error");
    assert_eq!(error.kind, ErrorKind::MismatchedConditional);
    assert_eq!(error.span, 7..13);
    assert_eq!(ErrorKind::MismatchedConditional.name(), "mismatched_conditional");
    assert!(!error.to_string().is_empty());
}

// special-fields.md P2-P4, R960.
#[test]
fn special_fields_of_a_new_card() {
    let f = new_card(&[("Front", "neko")], 0, &["animal", "jlpt-n5"]);
    assert_eq!(
        question(
            "{{Tags}}|{{Type}}|{{Deck}}|{{Subdeck}}|{{Card}}|{{CardFlag}}|{{CardID}}|{{c1}}",
            &f
        ),
        ok("animal jlpt-n5|Probe|(Deck)|(Deck)|Card 1|flag0|0|1")
    );
    assert_eq!(question("{{#Tags}}T{{/Tags}}{{^Tags}}-{{/Tags}}", &f), ok("T"));
    let untagged = new_card(&[("Front", "neko")], 0, &[]);
    assert_eq!(
        question(
            "{{#Tags}}T{{/Tags}}{{^Tags}}-{{/Tags}}{{#CardFlag}}F{{/CardFlag}}",
            &untagged
        ),
        ok("-F")
    );
    assert_eq!(question("{{tags}}", &untagged), Err(ErrorKind::UnknownField));
}

#[test]
fn special_values_of_a_stored_card() {
    let tags = vec!["zeta".to_owned(), "alpha".to_owned()];
    let specials = CardSpecials {
        ord: 2,
        notetype_name: "Words",
        template_name: "Listening",
        deck_name: Some("Lang::Japanese::Verbs"),
        flags: 9,
        card_id: 1_700_000_000_123,
        tags: &tags,
    };
    let f = FieldMap::new([("Front", "f")], &specials);
    assert_eq!(
        question(
            "{{Tags}}|{{Deck}}|{{Subdeck}}|{{Card}}|{{CardFlag}}|{{CardID}}|{{c3}}",
            &f
        ),
        ok("zeta alpha|Lang::Japanese::Verbs|Verbs|Listening|flag9|1700000000123|1")
    );
    assert_eq!(f.ord(), 2);
}

#[test]
fn note_fields_named_like_specials_win_except_front_side() {
    let f = new_card(
        &[
            ("Front", "f"),
            ("Tags", ""),
            ("FrontSide", "real"),
            ("c1", ""),
            ("Deck", "my deck"),
        ],
        0,
        &["t1"],
    );
    // A note field `Tags` with an empty value hides the note's tags.
    assert_eq!(
        question("{{Tags}}|{{#Tags}}T{{/Tags}}{{^Tags}}-{{/Tags}}", &f),
        ok("|-")
    );
    assert_eq!(question("{{c1}}|{{#c1}}C{{/c1}}{{^c1}}-{{/c1}}", &f), ok("|-"));
    // Subdeck still comes from the card's deck.
    assert_eq!(question("{{Deck}}|{{Subdeck}}", &f), ok("my deck|(Deck)"));
    // FrontSide in a replacement is always the front side...
    assert_eq!(question("[{{FrontSide}}]", &f), ok("[]"));
    assert_eq!(
        answer("{{FrontSide}}|{{text:FrontSide}}|{{type:FrontSide}}", &f, "Q"),
        ok("Q|Q|Q")
    );
    // ...but a section tests the note field.
    assert_eq!(question("{{#FrontSide}}FS{{/FrontSide}}", &f), ok("FS"));
}

// F3, R962.
#[test]
fn the_text_filter_strips_markup_and_decodes_entities() {
    assert_eq!(question("{{text:Front}}", &front("<b>a&amp;b</b>")), ok("a&b"));
    assert_eq!(question("{{text:Front}}", &front("<i>x</i> &bogus;")), ok("x &bogus;"));
}

// F5, R965.
#[test]
fn hints_have_the_exact_markup_and_blake3_ids() {
    let hint = |id: &str, key: &str, text: &str| {
        format!(
            "\n<a class=hint href=\"#\"\nonclick=\"this.style.display='none';\ndocument.getElementById('hint{id}').style.display='block';\nreturn false;\" draggable=false>\n{key}</a>\n<div id=\"hint{id}\" class=hint style=\"display: none\">{text}</div>\n"
        )
    };
    let f = new_card(&[("Front", "hello"), ("Back", "b")], 0, &[]);
    assert_eq!(
        question("{{hint:Front}}", &f),
        Ok(hint("eb589297cb31b693", "Front", "hello"))
    );
    assert_eq!(
        answer("{{hint:Back}}", &f, ""),
        Ok(hint("90eb8a52408c5123", "Back", "b"))
    );
    // White space alone comes back unchanged.
    for value in ["", " ", "\u{3000}\t\n"] {
        assert_eq!(question("{{hint:Front}}", &front(value)), ok(value), "{value:?}");
    }
}

// F4, R966: the spec's table.
#[test]
fn ruby_filters_follow_the_group_rules() {
    let ruby = |b: &str, r: &str| format!("<ruby><rb>{b}</rb><rt>{r}</rt></ruby>");
    let rows: [(&str, String, &str, &str); 11] = [
        (
            " 日本[にほん]語[ご]",
            format!("{}{}", ruby("日本", "にほん"), ruby("語", "ご")),
            "にほんご",
            "日本語",
        ),
        ("&nbsp;私[わたし]", ruby("私", "わたし"), "わたし", "私"),
        ("a>b[c]", format!("a>{}", ruby("b", "c")), "a>c", "a>b"),
        ("abc日本[にほん]", ruby("abc日本", "にほん"), "にほん", "abc日本"),
        ("a  b[c]", format!("a {}", ruby("b", "c")), "a c", "a b"),
        (
            "<b>漢字[かんじ]</b>",
            format!("<b>{}</b>", ruby("漢字", "かんじ")),
            "<b>かんじ</b>",
            "<b>漢字</b>",
        ),
        (
            "[sound:x.mp3]",
            "[sound:x.mp3]".to_owned(),
            "[sound:x.mp3]",
            "[sound:x.mp3]",
        ),
        (
            " 語[sound:x.mp3]",
            " 語[sound:x.mp3]".to_owned(),
            " 語[sound:x.mp3]",
            " 語[sound:x.mp3]",
        ),
        ("a&nbsp;b", "a b".to_owned(), "a b", "a b"),
        ("x[]]", ruby("x", "]"), "]", "x"),
        ("&NBSP;x[y]", ruby("&NBSP;x", "y"), "y", "&NBSP;x"),
    ];
    for (input, furigana, kana, kanji) in rows {
        let f = front(input);
        assert_eq!(question("{{furigana:Front}}", &f), Ok(furigana), "furigana {input:?}");
        assert_eq!(question("{{kana:Front}}", &f), ok(kana), "kana {input:?}");
        assert_eq!(question("{{kanji:Front}}", &f), ok(kanji), "kanji {input:?}");
    }
    for unchanged in ["x[]", "a\nb[c\nd]"] {
        assert_eq!(question("{{furigana:Front}}", &front(unchanged)), ok(unchanged));
    }
    assert_eq!(question("{{kanji:Front}}", &front("a[\n]b[c]")), ok("a[\n]b"));
    assert_eq!(question("{{kana:Front}}", &front("a[b[c]")), ok("b[c"));
    assert_eq!(question("{{kanji:Front}}", &front("a\nb[c]")), ok("a\nb"));
}

// F6, R967.
#[test]
fn tts_wraps_the_text_with_its_options_verbatim() {
    let f = front("こんにちは");
    assert_eq!(
        question("{{tts ja_JP voices=Apple_Kyoko speed=0.8:Front}}", &f),
        ok("[anki:tts lang=ja_JP voices=Apple_Kyoko speed=0.8]こんにちは[/anki:tts]")
    );
    let x = front("x");
    assert_eq!(
        question("{{tts  ja_JP:Front}}", &x),
        ok("[anki:tts lang= ja_JP]x[/anki:tts]")
    );
    assert_eq!(question("{{tts :Front}}", &x), ok("[anki:tts lang=]x[/anki:tts]"));
    assert_eq!(question("{{tts:Front}}|{{TTS en:Front}}", &x), ok("x|x"));
    // `b` is an unknown filter and ends the chain.
    assert_eq!(question("{{tts en_US voices=a:b:Front}}", &x), ok("x"));
}

// F1, F2, F7, F9, R964, R968.
#[test]
fn unknown_filters_end_the_chain_and_type_gives_markers() {
    let f = new_card(
        &[("Front", "front"), ("Back", "<b>back</b>"), ("Reading", "日本[にほん]")],
        0,
        &[],
    );
    assert_eq!(
        question("{{editable:furigana:Reading}}", &f),
        question("{{furigana:Reading}}", &f)
    );
    assert_eq!(question("{{furigana:editable:Reading}}", &f), ok("日本[にほん]"));
    assert_eq!(question("{{text:unknown:Back}}", &f), ok("<b>back</b>"));
    assert_eq!(question("{{Text:Back}}", &f), ok("<b>back</b>"));
    for (template, out) in [
        ("{{type:Back}}", "[[type:Back]]"),
        ("{{type:text:Back}}", "[[type:Back]]"),
        ("{{type:unknown:Back}}", "[[type:Back]]"),
        ("{{type:type:Back}}", "[[type:Back]]"),
        ("{{type::Back}}", "[[type:Back]]"),
        ("{{type:nc:Back}}", "[[type:nc:Back]]"),
        ("{{type-nc:Back}}", "[[type:nc:Back]]"),
        ("{{type:cloze:Back}}", "[[type:cloze:Back]]"),
        ("{{type-cloze:Back}}", "[[type:cloze:Back]]"),
        ("{{type:ci:Back}}", "[[type:Back]]"),
        ("{{type:nc:ci:Back}}", "[[type:Back]]"),
        ("{{type:ci:nc:Back}}", "[[type:Back]]"),
        ("{{type:cloze:nc:Back}}", "[[type:Back]]"),
        ("{{text:type:Back}}", "[[type:Back]]"),
        ("{{furigana:type:Back}}", "<ruby><rb>[</rb><rt>type:Back</rt></ruby>]"),
        ("{{type-cloze:text:Back}}", "[[type:cloze:Back]]"),
        ("{{nc:type:Back}}", "[[type:Back]]"),
        ("{{Type:Back}}", "<b>back</b>"),
        ("[{{type:}}]", "[]"),
    ] {
        assert_eq!(question(template, &f), ok(out), "{template}");
    }
    assert_eq!(question("{{type:Missing}}", &f), Err(ErrorKind::UnknownField));
}

// T7 rules 1 and 2.
#[test]
fn empty_keys_with_filters_render_nothing() {
    let f = front("front");
    assert_eq!(
        question("{{Front}}[{{text:}}][{{type:}}][{{hint:}}][{{:}}]", &f),
        ok("front[][][][]")
    );
}

// T10 through the strict analysis.
#[test]
fn a_question_is_blank_when_no_non_empty_name_would_render() {
    let blank = |template: &str, fields: &FieldMap| ParsedTemplate::parse(template).is_blank_question(fields);
    let f = new_card(&[("Front", "front"), ("Back", "back")], 0, &[]);
    assert!(!blank("{{Front}}", &f));
    assert!(blank("Question text only", &f));
    assert!(blank("{{#Front}}text{{/Front}}", &f));
    assert!(blank("{{FrontSide}}", &f));
    assert!(blank("{{text:}}", &f));
    assert!(blank("<!--{{Front}}-->", &f));
    assert!(blank("{{^Front}}{{Front}}{{/Front}}", &f));
    assert!(!blank("{{Type}}", &f));
    assert!(!blank("{{c1}}", &f));
    assert!(!blank("{{#c1}}{{Front}}{{/c1}}", &f));
    assert!(blank("{{Tags}}", &f));
    assert!(!blank("{{Tags}}", &new_card(&[("Front", "")], 0, &["tagged"])));
    for empty in ["", "<br>", " "] {
        assert!(blank("{{Front}}", &new_card(&[("Front", empty)], 0, &[])), "{empty:?}");
    }
    assert!(!blank("{{Front}}", &new_card(&[("Front", "&nbsp;")], 0, &[])));
    assert!(!blank("{{text:Front}}", &new_card(&[("Front", "<b></b>")], 0, &[])));
}

// T11.2's examples, non-empty names {A}.
#[test]
fn renders_something_in_both_forms() {
    let a = |name: &str| name == "A";
    let r = |template: &str, form: Form| ParsedTemplate::parse(template).renders_something(a, form);
    for (template, strict, legacy) in [
        ("{{B}}{{A}}", true, true),
        ("{{B}}", false, false),
        ("{{#A}}{{^B}}{{A}}{{/B}}{{/A}}", true, true),
        ("{{^A}}{{B}}{{/A}}", false, false),
        ("{{^A}}{{A}}{{/A}}", false, true),
        ("{{#A}}text{{/A}}", false, false),
        ("{{text:A}}", true, true),
        ("{{FrontSide}}", false, false),
    ] {
        assert_eq!(r(template, Form::Strict), strict, "strict {template}");
        assert_eq!(r(template, Form::Legacy), legacy, "legacy {template}");
    }
    let both = |name: &str| matches!(name, "A" | "B");
    let t = ParsedTemplate::parse("{{#A}}{{B}}{{/A}}");
    assert!(t.renders_something(both, Form::Strict));
    assert!(!t.renders_something(|n: &str| n == "B", Form::Strict));
}

// T11.3's table, fields A, B, C.
#[test]
fn legacy_requirements() {
    let fields = ["A", "B", "C"];
    for (template, kind, ords) in [
        ("{{A}}{{B}}", RequirementKind::Any, vec![0, 1]),
        ("{{#A}}{{B}}{{/A}}", RequirementKind::All, vec![0, 1]),
        ("{{^A}}{{B}}{{/A}}", RequirementKind::Any, vec![1]),
        ("{{^A}}{{#B}}{{C}}{{/B}}{{/A}}", RequirementKind::All, vec![1, 2]),
        ("{{^C}}{{#A}}{{B}}{{/A}}{{/C}}", RequirementKind::All, vec![0, 1]),
        ("{{^A}}{{A}}{{/A}}", RequirementKind::Any, vec![0]),
        ("{{text:A}}{{hint:C}}", RequirementKind::Any, vec![0, 2]),
        ("{{Tags}}", RequirementKind::None, vec![]),
        ("{{#A}}x{{/A}}", RequirementKind::None, vec![]),
        ("{{^A}}x{{/A}}", RequirementKind::None, vec![]),
        ("{{#c1}}{{A}}{{/c1}}", RequirementKind::None, vec![]),
        ("{{Nope}}", RequirementKind::None, vec![]),
        ("{{#A}}{{B}}", RequirementKind::None, vec![]),
        ("<!--{{A}}-->{{B}}", RequirementKind::Any, vec![1]),
    ] {
        let req = legacy_requirement(&ParsedTemplate::parse(template), &fields);
        assert_eq!((req.kind, req.ords), (kind, ords), "{template}");
    }
}

// T11.1.
#[test]
fn referenced_names() {
    let t = ParsedTemplate::parse(
        "{{Front}}{{text:Back}}{{#Extra}}{{/Extra}}{{^c2}}{{/c2}}{{#c1x}}{{/c1x}}<!--{{Hidden}}-->{{cloze:Text}}{{Front}}{{FrontSide}}{{}}",
    );
    assert_eq!(
        t.field_references(),
        ["Front", "Back", "Extra", "c1x", "Text", "FrontSide", ""]
    );
    assert_eq!(t.cloze_references(), ["Text"]);
}

// T12: spans of tags and comments in both delimiter modes.
#[test]
fn nodes_carry_exact_source_spans() {
    let t = ParsedTemplate::parse("{{{Front}}}<!-- c -->{{#A}}x{{/A}}");
    let spans: Vec<_> = t.nodes().iter().map(|n| (n.span.clone(), &n.kind)).collect();
    assert_eq!(spans.len(), 6);
    assert_eq!(spans[0].0, 0..10);
    assert_eq!(
        spans[0].1,
        &NodeKind::Replacement {
            key: "Front".to_owned(),
            filters: vec![]
        }
    );
    assert_eq!((spans[1].0.clone(), spans[1].1), (10..11, &NodeKind::Text));
    assert_eq!((spans[2].0.clone(), spans[2].1), (11..21, &NodeKind::Comment));
    assert_eq!(
        spans[3],
        (
            21..27,
            &NodeKind::Open {
                key: "A".to_owned(),
                negated: false
            }
        )
    );
    assert_eq!(spans[5], (28..34, &NodeKind::Close { key: "A".to_owned() }));

    let alt = ParsedTemplate::parse(" {{=<% %>=}}<%text:Front%>{{x}}<%^ B%><%/B%>");
    let kinds: Vec<_> = alt.nodes().iter().map(|n| (n.span.clone(), n.kind.clone())).collect();
    assert_eq!(kinds[0], (0..12, NodeKind::Prefix));
    assert_eq!(
        kinds[1],
        (
            12..26,
            NodeKind::Replacement {
                key: "Front".to_owned(),
                filters: vec!["text".to_owned()]
            }
        )
    );
    assert_eq!(kinds[2], (26..31, NodeKind::Text));
    assert_eq!(
        kinds[3],
        (
            31..38,
            NodeKind::Open {
                key: "B".to_owned(),
                negated: true
            }
        )
    );
    assert_eq!(kinds[4], (38..44, NodeKind::Close { key: "B".to_owned() }));
}

fn rebuilt(t: &ParsedTemplate) -> String {
    t.nodes().iter().map(|n| &t.source()[n.span.clone()]).collect()
}

/// Every spanned node starts and ends with its delimiters, and the nodes
/// tile the source.
fn check_spans(template: &str) {
    let t = ParsedTemplate::parse(template);
    assert_eq!(rebuilt(&t), template);
    let (open, close) = (t.delimiters().open(), t.delimiters().close());
    let mut at = 0;
    for node in t.nodes() {
        assert_eq!(node.span.start, at, "{template:?}");
        at = node.span.end;
        let text = &template[node.span.clone()];
        match &node.kind {
            NodeKind::Comment => assert!(text.starts_with("<!--") && text.ends_with("-->") && text.len() >= 7),
            NodeKind::Replacement { .. } | NodeKind::Open { .. } | NodeKind::Close { .. } => {
                assert!(
                    text.starts_with(open) && text.ends_with(close) && text.len() >= 4,
                    "{text:?}"
                );
            }
            NodeKind::Prefix => assert!(text.trim_start().starts_with("{{=<% %>=}}")),
            NodeKind::Text => assert!(!text.is_empty()),
        }
    }
    assert_eq!(at, template.len());
}

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

#[test]
fn every_corpus_template_rebuilds_from_its_nodes() {
    let dir = conformance_dir().join("scenarios/render/corpus");
    let mut checked = 0;
    let mut alternate = 0;
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let scenario: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for step in scenario["steps"].as_array().unwrap() {
            for request in step["observe"].as_array().unwrap() {
                for case in request["template"]["cases"].as_array().unwrap() {
                    for side in ["front", "back"] {
                        if let Some(text) = case[side].as_str() {
                            check_spans(text);
                            if text.contains("{{=<% %>=}}") {
                                alternate += 1;
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 8_000);
    assert!(alternate > 100, "{alternate} alternate-mode templates");
}

#[test]
fn awkward_templates_rebuild_from_their_nodes() {
    for template in [
        "",
        "{{",
        "}}",
        "{{}}",
        "{{{{}}}}",
        "<!---->",
        "<!-->",
        "{{=<% %>=}}",
        "\u{3000}{{=<% %>=}}{{=<% %>=}}<%%>%>",
        "<%Front%>",
        "{{#A}}{{/B}}{{/C}}",
        "日本{{日本}}<!--語-->😀{{",
        "{{a<!--}}-->",
        "<!--{{-->}}",
    ] {
        check_spans(template);
    }
}

/// T13, R1805: 5 MB templates of the shapes that make a naive scan
/// quadratic, and 1 MB fields (R1805's worst-case notes) under many sections,
/// take time close to linear. Each case takes about 0.1 s in the test
/// profile; the bound leaves room for a loaded machine and is far below a
/// quadratic run, which takes minutes to hours. Each case runs on its own
/// thread, so a regression fails at the bound instead of running to its end.
#[test]
fn five_megabyte_templates_parse_and_render_in_bounded_time() {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::time::Duration;

    const SIZE: usize = 5 * 1024 * 1024;
    const BOUND: Duration = Duration::from_secs(2);
    let fill = |unit: &str| unit.repeat(SIZE / unit.len());
    let small = new_card(&[("A", "a"), ("Front", "日本[にほん]")], 0, &[]);
    // Empty (E1) only once every one of its bytes has been read: judging it
    // again at each of the template's 400,000 section tags reads 400 GB.
    let blank_mib = " ".repeat(1024 * 1024);
    let big = new_card(&[("A", &blank_mib)], 0, &[]);
    for (template, fields) in [
        (fill("{{"), &small),
        (fill("<!--"), &small),
        (fill("{{=<% %>=}}"), &small),
        (fill("{{#A}}"), &small),
        (fill("{{A}}<!--"), &small),
        (format!("{}}}}}", fill("{{x")), &small),
        (fill("{{furigana:Front}}x"), &small),
        (fill("{{#A}}{{A}}{{/A}}"), &small),
        (fill("<%{{-->"), &small),
        (fill("{{#A}}{{/A}}{{^A}}{{/A}}"), &big),
    ] {
        let label = template[..12].to_owned();
        let fields = fields.clone();
        let (done, finished) = channel();
        std::thread::spawn(move || {
            let parsed = ParsedTemplate::parse(&template);
            let _ = parsed.render(&fields, Side::Question);
            let _ = parsed.is_blank_question(&fields);
            let _ = done.send(());
        });
        match finished.recv_timeout(BOUND) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => panic!("{label:?}…: not done within {BOUND:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("{label:?}…: the engine panicked"),
        }
    }
}
