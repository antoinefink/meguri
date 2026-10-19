//! Neutral A4–A9 examples; expected strings are independent literals.
use meguri_template::{
    ParsedTemplate,
    type_answer::{self as ta, Descriptor, Field, Mode, WarningForm},
};
fn descriptor() -> Descriptor {
    Descriptor {
        expected: "ka".into(),
        mode: Mode::Plain,
        case_insensitive: false,
        font: "Qav Serif".into(),
        size: 28,
    }
}
#[test]
fn ae17_and_preview_comparisons_are_exact() {
    assert_eq!(
        ta::compare("ka", "ka", Mode::Plain, false),
        "<code id=typeans><span class=typeGood>ka</span></code>"
    );
    assert_eq!(
        ta::compare("ka", "kq", Mode::Plain, false),
        "<code id=typeans><span class=typeGood>k</span><span class=typeBad>q</span><br><span id=typearrow>&darr;</span><br><span class=typeGood>k</span><span class=typeMissed>a</span></code>"
    );
    assert_eq!(ta::compare("ka", "", Mode::Plain, false), "<code id=typeans>ka</code>");
    assert_eq!(
        ta::compare("example", "sample", Mode::Plain, false),
        "<code id=typeans><span class=typeBad>s</span><span class=typeGood>ample</span><br><span id=typearrow>&darr;</span><br><span class=typeMissed>ex</span><span class=typeGood>ample</span></code>"
    );
}
#[test]
fn normalization_case_folding_and_unicode_versions_stay_separate() {
    assert_eq!(
        ta::compare("e\u{301}", "é", Mode::Plain, false),
        "<code id=typeans><span class=typeGood>é</span></code>"
    );
    assert_eq!(
        ta::compare("\u{301}ﬁ①", "fi1", Mode::Nc, false),
        "<code id=typeans><span class=typeGood>\u{301}ﬁ①</span></code>"
    );
    assert_eq!(ta::compare("é", "", Mode::Nc, false), "<code id=typeans>é</code>");
    for (expected, typed) in [("Σſǅ", "ςsǆ"), ("Qëvax", "qevax"), ("K", "k"), ("𐐀", "𐐨"), ("Ᲊ", "ᲊ")]
    {
        assert!(!ta::compare(expected, typed, Mode::Nc, true).contains("typeBad"));
    }
    for (expected, typed) in [("ß", "ss"), ("İ", "i")] {
        assert!(ta::compare(expected, typed, Mode::Plain, true).contains("typeBad"));
    }
    assert!(ta::compare("xy", "x\u{335}y", Mode::Plain, false).contains("class=typeBad>\u{a0}\u{335}</span>"));
    for mark in ['\u{8d3}', '\u{1ac1}'] {
        let result = ta::compare("xy", &format!("x{mark}y"), Mode::Plain, false);
        assert!(result.contains(&format!("class=typeBad>{mark}</span>")));
        assert!(!result.contains('\u{a0}'));
    }
}
#[test]
fn repeated_long_inputs_use_the_pinned_matcher() {
    let expected = format!("{}abc{}", "x".repeat(100), "y".repeat(100));
    // A6.7 explicitly fixes the whole response here, unlike Python difflib.
    assert_eq!(
        ta::compare(&expected, "abc", Mode::Plain, false),
        format!(
            "<code id=typeans><span class=typeBad>abc</span><br><span id=typearrow>&darr;</span><br><span class=typeMissed>{expected}</span></code>"
        )
    );
}
#[test]
fn expected_cleanup_reuses_av_grammar_and_plain_html_rules() {
    assert_eq!(
        ta::expected_text("[anki:tts]x[/anki:tts]a<BR/>b<br />c<br\t>d<DIV>e</Div>f<div class=x>g"),
        "a b c d e fg"
    );
    assert_eq!(
        ta::expected_text("[anki:foo a=1 a=2]x[/anki:foo][sound:q]"),
        "[anki:foo a=2]x[/anki:foo]"
    );
    assert_eq!(
        ta::expected_text("[anki:foo][sound:x.mp3][/anki:foo]y"),
        "[anki:foo][sound:x.mp3][/anki:foo]y"
    );
    assert_eq!(ta::expected_text("a & b <b>c</b>"), "a & b c");
    assert_eq!(ta::expected_text("one<br><br>\n<div>two</div>"), "one two");
}
fn question(
    text: &str,
    font: &str,
    value: &str,
    display: bool,
) -> Result<(String, Option<Descriptor>), ta::InvalidReplacement> {
    ta::question(
        text,
        &ParsedTemplate::parse("{{type:Back}}"),
        &[Field {
            name: "Back",
            value,
            font,
            size: 28,
        }],
        1,
        WarningForm::Token,
        display,
    )
}
#[test]
fn first_descriptor_repeated_boxes_attributes_and_empty_paths() {
    let (out, d) = question("[[type:Back]]|[[type:Other]]", "Qav Serif", "ka", true).unwrap();
    let box_html = "\n<center>\n<input type=text id=typeans autocapitalize=\"off\" autocorrect=\"off\" spellcheck=\"false\" onkeypress=\"_typeAnsPress();\"\n   style=\"font-family: 'Qav Serif'; font-size: 28px;\">\n</center>\n";
    assert_eq!(out, format!("{box_html}|{box_html}"));
    assert_eq!(d, Some(descriptor()));
    assert_eq!(
        question("[[type:Back]]|[[type:Other]]", "Arial", "", false).unwrap(),
        ("|".into(), None)
    );
    assert!(question("[[type:Back]]", "Arial", "<br>", false).unwrap().1.is_some());
    assert_eq!(
        question("[[type:Tags]]|[[type:Back]]", "Arial", "ka", false).unwrap(),
        (
            "[[warning:unknown_field:Tags]]|[[warning:unknown_field:Tags]]".into(),
            None
        )
    );
    assert_eq!(
        question("[[type:cloze:Back]]", "Arial", "ka", false).unwrap(),
        ("[[warning:empty_cards]]".into(), None)
    );
    assert_eq!(
        question("[[type:a\nb]]", "Arial", "ka", false).unwrap(),
        ("[[type:a\nb]]".into(), None)
    );
}
#[test]
fn replacements_expand_each_match_once_and_answer_font_is_literal() {
    let (out, d) = question(
        "[[type:Back]]|[[type:Other]]",
        r"Q\g<0>\1\g<1>\101\011\n\&\\",
        "ka",
        false,
    )
    .unwrap();
    assert!(out.contains("font-family: 'Q[[type:Back]]BackBackA\t\n\\&\\'"));
    assert!(out.contains("font-family: 'Q[[type:Other]]OtherOtherA\t\n\\&\\'"));
    let answer = ta::answer("[[type:Back]]", d.as_ref(), "ka");
    assert!(answer.contains(r"font-family: 'Q\g<0>\1\g<1>\101\011\n\&\\'"));
    for font in [
        r"Q\q",
        r"Q\2",
        r"Q\11",
        r"Q\400",
        r"Q\g<missing>",
        r"Q\g<2>",
        r"Q\g<",
        r"Q\g0",
    ] {
        assert_eq!(
            question("[[type:Back]]", font, "ka", false),
            Err(ta::InvalidReplacement),
            "{font}"
        );
    }
    // A font-ending backslash precedes punctuation in the *complete* template.
    assert!(question("[[type:Back]]", "Q\\", "ka", false).is_ok());
    let (out, _) = question("[[type:Back]]", r"\1", "ka", false).unwrap();
    assert!(out.contains("font-family: 'Back'"));
}
#[test]
fn answer_rule_positions_and_literal_font_text() {
    let d = descriptor();
    let block = "\n<div style=\"font-family: 'Qav Serif'; font-size: 28px\"><code id=typeans><span class=typeGood>ka</span></code></div>";
    assert_eq!(
        ta::answer(
            "[[type:Other]]<hr id=answer>[[type:Back]]<hr id=answer>",
            Some(&d),
            "ka"
        ),
        format!("<hr id=answer>{block}<hr id=answer>{block}")
    );
    assert_eq!(
        ta::answer("[[type:<hr id=answer>X]]", Some(&d), "ka"),
        format!("<hr id=answer>{block}")
    );
    assert_eq!(ta::answer("x<hr id=answer>y", Some(&d), "ka"), "x<hr id=answer>y");
    assert_eq!(
        ta::answer("[[type:Back]]<hr id=\"answer\">", Some(&d), "ka"),
        format!("{block}<hr id=\"answer\">")
    );
    assert_eq!(ta::answer("[[type:Back]]<hr id=answer>", None, "ka"), "<hr id=answer>");
}
#[test]
fn modifiers_use_first_source_replacement_even_inside_an_inactive_section() {
    let template = ParsedTemplate::parse("{{#Off}}{{type:ci:nc:cloze:Back}}{{/Off}}{{type:Back}}");
    let fields = [Field {
        name: "Back",
        value: "{{c1::Café}} {{c1::Té}}",
        font: "Arial",
        size: 20,
    }];
    let (_, descriptor) = ta::question("[[type:Back]]", &template, &fields, 1, WarningForm::Token, false).unwrap();
    let d = descriptor.unwrap();
    assert_eq!(d.expected, "Café, Té");
    assert_eq!(
        ta::compare_typed_answer(&d, "CAFE, TE"),
        "<code id=typeans><span class=typeGood>Café, Té</span></code>"
    );
}
