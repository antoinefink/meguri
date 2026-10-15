//! The template engine never panics and keeps its parse contract on any
//! input (specs/render/template-syntax.md T12, T13; R1805): parsing, both
//! sides' rendering, the analysis and the legacy requirement, on the bytes
//! as a template and on a template generated from them, with the bytes as a
//! field's text so the filters see them too.

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_template::{CardSpecials, FieldMap, Form, ParsedTemplate, Side, legacy_requirement};

/// What a generated template is made of, one piece per input byte.
const PIECES: [&str; 32] = [
    "{{",
    "}}",
    "<%",
    "%>",
    "{{=<% %>=}}",
    "<!--",
    "-->",
    "{",
    "}",
    ":",
    "#",
    "^",
    "/",
    " ",
    "\n",
    "\u{3000}",
    "Front",
    "Back",
    "c1",
    "c+2",
    "Tags",
    "FrontSide",
    "text",
    "furigana",
    "hint",
    "tts en ",
    "type",
    "nc",
    "kana",
    "cloze",
    "<br>",
    "x",
];

fn check(template: &str, fields: &FieldMap) {
    let parsed = ParsedTemplate::parse(template);
    // T12: the nodes tile the source.
    let mut at = 0;
    for node in parsed.nodes() {
        assert_eq!(node.span.start, at, "{template:?}");
        at = node.span.end;
    }
    assert_eq!(at, template.len(), "{template:?}");
    let question = parsed.render(fields, Side::Question);
    if let Err(error) = &question {
        assert!(template.get(error.span.clone()).is_some(), "{template:?}");
    }
    let front_side = question.as_deref().unwrap_or_default();
    let _ = parsed.render(fields, Side::Answer { front_side });
    if let Some(error) = parsed.structure_error() {
        assert!(error.kind.is_structural());
        assert!(!parsed.renders_something(|_| true, Form::Strict));
    }
    let _ = parsed.is_blank_question(fields);
    let _ = parsed.renders_something(|name| name.len() % 2 == 0, Form::Legacy);
    let _ = parsed.field_references();
    let _ = parsed.cloze_references();
    let _ = legacy_requirement(&parsed, &["Front", "Back", "c1"]);
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let tags = vec!["t".to_owned(), "x::y".to_owned()];
    let specials = CardSpecials {
        ord: 1,
        notetype_name: "N",
        template_name: "Card 2",
        deck_name: Some("A::B"),
        flags: 3,
        card_id: 7,
        tags: &tags,
    };
    let fields = FieldMap::new([("Front", text.as_ref()), ("Back", ""), ("FrontSide", "f")], &specials);
    check(&text, &fields);
    let generated: String = data.iter().map(|&b| PIECES[usize::from(b) % PIECES.len()]).collect();
    check(&generated, &fields);
});
