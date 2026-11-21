//! The Card Text row (`notetypes/workspace` W10, W10.1, W10.2): when it
//! shows, what its controls read, and that an edit changes only one
//! declaration's value characters (or adds one declaration as W10 says),
//! every other byte of the user's CSS staying as it was.

use meguri_engine::notetypes::card_text::{
    Alignment, CardText, CardTextEdit, CardTextEdited, CardTextRefusal, Colour, Font, edit, read,
};

const fn rgb(red: u8, green: u8, blue: u8) -> Colour {
    Colour {
        red,
        green,
        blue,
        alpha: 255,
    }
}

fn ltr(css: &str) -> Option<CardText> {
    read(css, false)
}

/// The edit's text, after checking that only `old` (the bytes it replaced,
/// found once in `css`) changed and that the reported range holds `new`.
fn replaced(css: &str, change: CardTextEdit, old: &str, new: &str) -> String {
    let CardTextEdited { css: out, value } = edit(css, &change, false).unwrap();
    assert_eq!(&out[value.clone()], new);
    assert_eq!(&out[..value.start], &css[..value.start], "bytes before the value");
    assert_eq!(&out[value.end..], &css[value.start + old.len()..], "bytes after the value");
    assert_eq!(&css[value.start..value.start + old.len()], old);
    out
}

fn inserted(css: &str, change: CardTextEdit) -> String {
    let result = edit(css, &change, false).unwrap();
    result.css
}

#[test]
fn the_controls_read_the_plain_card_rule() {
    let css = ".card {\n  font-family: \"Hiragino Sans\", serif;\n  font-size: 15pt;\n  text-align: start;\n  color: rgb(10, 20, 30);\n  background-color: #FFF;\n}\n";
    assert_eq!(
        ltr(css),
        Some(CardText {
            font: Some(Font::Family("Hiragino Sans".into())),
            size: Some(20.0),
            alignment: Some(Alignment::Leading),
            text_colour: Some(rgb(10, 20, 30)),
            background: Some(rgb(255, 255, 255)),
        })
    );
    // A rule without the declarations: every control is empty.
    assert_eq!(ltr(".card {}"), Some(CardText::default()));
    // Property names and keywords in any case.
    let upper = ltr(".card { FONT-SIZE: 20PX; Text-Align: RIGHT; COLOR: Red }").unwrap();
    assert_eq!(upper.size, Some(20.0));
    assert_eq!(upper.alignment, Some(Alignment::Trailing));
    assert_eq!(upper.text_colour, Some(rgb(255, 0, 0)));
}

#[test]
fn font_size_and_alignment_follow_w10_2() {
    let font = |value: &str| ltr(&format!(".card {{ font-family: {value}; }}")).unwrap().font;
    assert_eq!(font("system-ui, sans-serif"), Some(Font::System));
    assert_eq!(font("-apple-system, serif"), Some(Font::System));
    assert_eq!(font("BlinkMacSystemFont"), Some(Font::System));
    assert_eq!(font("serif"), Some(Font::Family("serif".into())));
    assert_eq!(font("monospace"), Some(Font::Family("monospace".into())));
    assert_eq!(font("Hiragino   Sans, serif"), Some(Font::Family("Hiragino Sans".into())));
    assert_eq!(font("'Noto Sans', serif"), Some(Font::Family("Noto Sans".into())));
    assert_eq!(font("\"system-ui\""), Some(Font::Family("system-ui".into())));
    assert_eq!(font("var(--face), serif"), None);

    let size = |value: &str| ltr(&format!(".card {{ font-size: {value}; }}")).unwrap().size;
    assert_eq!(size("20px"), Some(20.0));
    assert_eq!(size("20.5px"), Some(20.5));
    assert_eq!(size("12pt"), Some(16.0));
    assert_eq!(size("10pt"), Some(13.0));
    for other in ["1.5em", "120%", "large", "calc(10px + 2px)", "20", "20px 2px"] {
        assert_eq!(size(other), None, "{other}");
    }

    let align = |value: &str, rtl: bool| read(&format!(".card {{ text-align: {value}; }}"), rtl).unwrap().alignment;
    assert_eq!(align("left", false), Some(Alignment::Leading));
    assert_eq!(align("right", false), Some(Alignment::Trailing));
    assert_eq!(align("center", false), Some(Alignment::Center));
    assert_eq!(align("left", true), Some(Alignment::Trailing));
    assert_eq!(align("right", true), Some(Alignment::Leading));
    assert_eq!(align("start", true), Some(Alignment::Leading));
    assert_eq!(align("end", false), Some(Alignment::Trailing));
    assert_eq!(align("justify", false), None);
    assert_eq!(align("-webkit-center", false), None);
}

#[test]
fn colours_read_their_forms_and_nothing_else() {
    let colour = |value: &str| ltr(&format!(".card {{ color: {value}; }}")).unwrap().text_colour;
    assert_eq!(colour("#0af"), Some(rgb(0, 0xaa, 0xff)));
    assert_eq!(
        colour("#11223344"),
        Some(Colour {
            red: 0x11,
            green: 0x22,
            blue: 0x33,
            alpha: 0x44
        })
    );
    assert_eq!(colour("hsl(120 100% 50%)"), Some(rgb(0, 255, 0)));
    assert_eq!(colour("navy"), Some(rgb(0, 0, 0x80)));
    for other in ["var(--ink)", "currentColor", "lab(50% 40 59)", "red !important x"] {
        assert_eq!(colour(other), None, "{other}");
    }
    // `!important` is not part of the value.
    assert_eq!(colour("red !important"), Some(rgb(255, 0, 0)));
    assert_eq!(colour("red!IMPORTANT"), Some(rgb(255, 0, 0)));
}

/// The background: the last `background-color`, else a `background`
/// shorthand whose value is one colour, whose token alone is edited.
#[test]
fn the_background_reads_and_edits_the_shorthand_only_without_background_color() {
    let css = ".card { background: #123456; }";
    assert_eq!(ltr(css).unwrap().background, Some(rgb(0x12, 0x34, 0x56)));
    assert_eq!(
        replaced(css, CardTextEdit::Background(rgb(0xab, 0xcd, 0xef)), "#123456", "#abcdef"),
        ".card { background: #abcdef; }"
    );
    // Not one colour: the control is empty and a background-color is added
    // after the shorthand.
    let layered = ".card { background: url(paper.png) #fff; }";
    assert_eq!(ltr(layered).unwrap().background, None);
    assert_eq!(
        inserted(layered, CardTextEdit::Background(rgb(1, 2, 3))),
        ".card { background: url(paper.png) #fff;  background-color: #010203; }"
    );
    // A background-color anywhere in the rule is the one read and edited.
    let both = ".card { background-color: red; background: blue }";
    assert_eq!(ltr(both).unwrap().background, Some(rgb(255, 0, 0)));
    assert_eq!(
        replaced(both, CardTextEdit::Background(rgb(0, 0, 0)), "red", "#000000"),
        ".card { background-color: #000000; background: blue }"
    );
    // The background is written without alpha.
    let translucent = Colour {
        alpha: 10,
        ..rgb(1, 2, 3)
    };
    assert_eq!(
        replaced(css, CardTextEdit::Background(translucent), "#123456", "#010203"),
        ".card { background: #010203; }"
    );
}

/// Comments, odd white space, `!important` and the last of two
/// declarations: only the value's characters change.
#[test]
fn an_edit_changes_only_the_values_characters() {
    let css = "/* top */\n.card{font-size:  12px /* was 10 */ ;color:red}\n.x { color: blue }\n";
    assert_eq!(
        replaced(css, CardTextEdit::Size(18.0), "12px", "18px"),
        "/* top */\n.card{font-size:  18px /* was 10 */ ;color:red}\n.x { color: blue }\n"
    );
    assert_eq!(
        replaced(css, CardTextEdit::TextColour(rgb(0, 255, 0)), "red", "#00ff00"),
        "/* top */\n.card{font-size:  12px /* was 10 */ ;color:#00ff00}\n.x { color: blue }\n"
    );
    let important = ".card { color: red ! important; font-size: 12px!important }";
    assert_eq!(
        replaced(important, CardTextEdit::TextColour(rgb(0, 0, 255)), "red", "#0000ff"),
        ".card { color: #0000ff ! important; font-size: 12px!important }"
    );
    assert_eq!(
        replaced(important, CardTextEdit::Size(14.5), "12px", "14.5px"),
        ".card { color: red ! important; font-size: 14.5px!important }"
    );
    let twice = ".card { color: red; color: /* ink */ blue; }";
    assert_eq!(ltr(twice).unwrap().text_colour, Some(rgb(0, 0, 255)));
    assert_eq!(
        replaced(twice, CardTextEdit::TextColour(rgb(1, 1, 1)), "blue", "#010101"),
        ".card { color: red; color: /* ink */ #010101; }"
    );
    let upper = ".card { COLOR: Red }";
    assert_eq!(
        replaced(upper, CardTextEdit::TextColour(rgb(0, 0, 0)), "Red", "#000000"),
        ".card { COLOR: #000000 }"
    );
    // A family list is replaced whole; a family with a space is quoted.
    let fonts = ".card { font-family: Arial, serif; }";
    assert_eq!(
        replaced(fonts, CardTextEdit::Font(Font::System), "Arial, serif", "system-ui, -apple-system, sans-serif"),
        ".card { font-family: system-ui, -apple-system, sans-serif; }"
    );
    assert_eq!(
        replaced(fonts, CardTextEdit::Font(Font::Family("Hiragino Sans".into())), "Arial, serif", "\"Hiragino Sans\""),
        ".card { font-family: \"Hiragino Sans\"; }"
    );
    assert_eq!(
        replaced(fonts, CardTextEdit::Font(Font::Family("Optima".into())), "Arial, serif", "Optima"),
        ".card { font-family: Optima; }"
    );
    // An alignment in the interface's direction.
    let aligned = ".card { text-align: left; }";
    let rtl = edit(aligned, &CardTextEdit::Alignment(Alignment::Leading), true).unwrap();
    assert_eq!(rtl.css, ".card { text-align: right; }");
    assert_eq!(
        replaced(aligned, CardTextEdit::Alignment(Alignment::Center), "left", "center"),
        ".card { text-align: center; }"
    );
    // A colour that is not opaque keeps its alpha.
    let translucent = Colour {
        alpha: 0x80,
        ..rgb(1, 2, 3)
    };
    assert_eq!(
        replaced(".card { color: red }", CardTextEdit::TextColour(translucent), "red", "#01020380"),
        ".card { color: #01020380 }"
    );
    // A declaration with no value gets one where its value would be.
    assert_eq!(
        replaced(".card { color: ; }", CardTextEdit::TextColour(rgb(255, 0, 0)), "", "#ff0000"),
        ".card { color: #ff0000; }"
    );
    assert_eq!(
        replaced(".card{color:}", CardTextEdit::TextColour(rgb(255, 0, 0)), "", "#ff0000"),
        ".card{color:#ff0000}"
    );
}

/// W10's insertion: a `;` after an unterminated last declaration, then the
/// declaration on its own line before a `}` alone on its line (that line's
/// line break), or ` <property>: <value>; ` just before `}`.
#[test]
fn a_missing_declaration_is_added_at_the_end_of_the_block() {
    assert_eq!(
        inserted(".card {\n  color: red;\n}\n", CardTextEdit::Size(20.0)),
        ".card {\n  color: red;\n  font-size: 20px;\n}\n"
    );
    assert_eq!(
        inserted(".card {\r\n\tcolor: red\r\n}\r\n", CardTextEdit::Size(20.0)),
        ".card {\r\n\tcolor: red;\r\n  font-size: 20px;\r\n}\r\n"
    );
    assert_eq!(
        inserted(".card {\n    color: red /* last */\n    }\n.next {}", CardTextEdit::Alignment(Alignment::Center)),
        ".card {\n    color: red; /* last */\n  text-align: center;\n    }\n.next {}"
    );
    assert_eq!(
        inserted("/* top */\n.card{font-size:  12px /* was 10 */ ;color:red}\n", CardTextEdit::Alignment(Alignment::Center)),
        "/* top */\n.card{font-size:  12px /* was 10 */ ;color:red; text-align: center; }\n"
    );
    assert_eq!(inserted(".card {}", CardTextEdit::Size(20.0)), ".card { font-size: 20px; }");
    assert_eq!(
        inserted(".card { color: red !important }", CardTextEdit::Size(20.0)),
        ".card { color: red !important;  font-size: 20px; }"
    );
    // `}` followed by more on its line is not alone.
    assert_eq!(
        inserted(".card {\n  color: red;\n} .x {}", CardTextEdit::Size(20.0)),
        ".card {\n  color: red;\n font-size: 20px; } .x {}"
    );
    // A nested rule stays as it is; the declaration goes after it.
    assert_eq!(
        inserted(".card { color: red; .x { color: blue } }", CardTextEdit::Size(20.0)),
        ".card { color: red; .x { color: blue }  font-size: 20px; }"
    );
    let result = edit(".card {}", &CardTextEdit::Size(20.0), false).unwrap();
    assert_eq!(&result.css[result.value], "20px");
}

/// W10.1: exactly one top-level rule whose prelude is `.card`; other
/// selectors with `.card`, rules inside at-rules and night-mode rules do
/// not count, and they stay byte for byte when the plain rule is edited.
#[test]
fn only_one_top_level_plain_card_rule_shows_the_row() {
    let css = "@import url(base.css);\n.card.nightMode { color: white }\n.nightMode .card { color: white }\n.card .x {}\n.card, .x {}\ndiv.card {}\n.cards {}\n@media (prefers-color-scheme: dark) { .card { color: white } }\n@supports (display: grid) { .card { color: gray } }\n/* a */ .card /* b */ { color: red }\n";
    assert_eq!(ltr(css).unwrap().text_colour, Some(rgb(255, 0, 0)));
    let out = replaced(css, CardTextEdit::TextColour(rgb(0, 0, 0)), "red", "#000000");
    assert_eq!(out, css.replace("{ color: red }", "{ color: #000000 }"));

    for absent in [
        "",
        ".x { color: red }",
        ".card { color: red }\n.card { font-size: 12px }",
        "@media screen { .card { color: red } }",
        ".card.nightMode { color: white }",
        ".c\\61 rd { color: red }",
        "#card { color: red }",
    ] {
        assert_eq!(ltr(absent), None, "{absent:?}");
        assert_eq!(edit(absent, &CardTextEdit::Size(20.0), false), Err(CardTextRefusal::NoRow), "{absent:?}");
    }
    // Comments are removed from the prelude as text.
    assert!(ltr(".ca/**/rd { color: red }").is_some());
    assert!(ltr("\t.card\n{}").is_some());
}

/// W10.1's not well formed: the text ends inside a block, a string, a
/// `url(` or a comment, or a `}` closes no block. There is no row, and no
/// edit is made: the user's CSS is never repaired.
#[test]
fn css_that_is_not_well_formed_has_no_row() {
    for malformed in [
        ".card { color: red",
        ".card { color: red }\n@media screen { .x { }",
        ".card { font-family: \"Arial; }",
        ".card { font-family: 'Arial }",
        ".card { background: url(paper.png }",
        ".card { background: url( paper.png ; }",
        ".card { color: red } /* note",
        ".card { color: red } }",
        "} .card { color: red }",
        ".card { color: rgb(1, 2, 3 }",
        ".card { color: [red }",
    ] {
        assert_eq!(ltr(malformed), None, "{malformed:?}");
        assert_eq!(
            edit(malformed, &CardTextEdit::TextColour(rgb(0, 0, 0)), false),
            Err(CardTextRefusal::NoRow),
            "{malformed:?}"
        );
    }
    // Well formed by W10.1: a string broken by a line break, a `)` or `]`
    // that closes nothing, a closed `url(`.
    assert_eq!(ltr(".card { content: \"a\n; color: red }").unwrap().text_colour, Some(rgb(255, 0, 0)));
    assert!(ltr(".card { color: red) }").is_some());
    assert!(ltr(".card { x: a] ; color: red }").is_some());
    assert!(ltr(".card { background: url(paper.png) }").is_some());
}

#[test]
fn invalid_values_are_refused() {
    let css = ".card {}";
    for points in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(edit(css, &CardTextEdit::Size(points), false), Err(CardTextRefusal::InvalidValue));
    }
    for name in ["", "Line\nBreak", "Tab\tName"] {
        assert_eq!(
            edit(css, &CardTextEdit::Font(Font::Family(name.into())), false),
            Err(CardTextRefusal::InvalidValue),
            "{name:?}"
        );
    }
    // A name that could end the declaration or the rule is quoted, with
    // its quote and backslash escaped, so the text stays well formed.
    let quoted = inserted(css, CardTextEdit::Font(Font::Family("A;B}\"C\\".into())));
    assert_eq!(quoted, ".card { font-family: \"A;B}\\\"C\\\\\"; }");
    assert_eq!(ltr(&quoted).unwrap().font, Some(Font::Family("A;B}\"C\\".into())));
}

/// After an item that is not a declaration (half typed, a star hack, an
/// at-rule statement) the `;` goes after that item, so the new declaration
/// is one; where a `;` would be absorbed (a broken string, a lone `\\`),
/// nothing is written. Every edit reads back as the value written.
#[test]
fn a_new_declaration_never_joins_the_text_before_it() {
    assert_eq!(
        inserted(".card {\n  color: red;\n  font-weig\n}", CardTextEdit::Size(20.0)),
        ".card {\n  color: red;\n  font-weig;\n  font-size: 20px;\n}"
    );
    assert_eq!(
        inserted(".card { color: red; leftover }", CardTextEdit::Size(20.0)),
        ".card { color: red; leftover;  font-size: 20px; }"
    );
    assert_eq!(inserted(".card { *zoom: 1 }", CardTextEdit::Size(20.0)), ".card { *zoom: 1;  font-size: 20px; }");
    assert_eq!(
        inserted(".card { color: red; @extra thing }", CardTextEdit::Size(20.0)),
        ".card { color: red; @extra thing;  font-size: 20px; }"
    );
    for text in [".card {\n  content: \"abc\n}", ".card {\n  color: red \\\n}"] {
        assert_eq!(
            edit(text, &CardTextEdit::Size(20.0), false),
            Err(CardTextRefusal::NotApplied),
            "{text:?}"
        );
    }
    // Multi-byte text before the value: the range is in bytes of the result.
    let css = "/* é ü 漢字 */\n.card { color: red }";
    let out = edit(css, &CardTextEdit::TextColour(rgb(1, 2, 3)), false).unwrap();
    assert_eq!(&out.css[out.value.clone()], "#010203");
    assert_eq!(out.css, "/* é ü 漢字 */\n.card { color: #010203 }");
    // A colour inside a nested rule or an at-rule of the block is not the
    // rule's own.
    let nested = ".card { .x { color: red } @media print { color: blue } }";
    assert_eq!(ltr(nested).unwrap().text_colour, None);
}

/// A family is quoted unless it reads back as one identifier that is not a
/// CSS-wide keyword.
#[test]
fn families_that_would_read_otherwise_are_quoted() {
    for (name, written) in [
        ("3Dumb", "\"3Dumb\""),
        ("-1x", "\"-1x\""),
        ("-", "\"-\""),
        ("inherit", "\"inherit\""),
        ("Revert-Layer", "\"Revert-Layer\""),
        ("Optima", "Optima"),
        ("-apple-x", "-apple-x"),
    ] {
        let out = inserted(".card {}", CardTextEdit::Font(Font::Family(name.into())));
        assert_eq!(out, format!(".card {{ font-family: {written}; }}"), "{name}");
        assert_eq!(ltr(&out).unwrap().font, Some(Font::Family(name.into())), "{name}");
    }
}
