//! Review type-in substitution, independent of storage (render/type-answer A2–A10).
mod compare;
mod replacement;
mod unicode10_marks;
use crate::{NodeKind, ParsedTemplate};
pub use compare::{compare, compare_typed_answer, expected_text};
use regex::Regex;
use replacement::Replacement;
use std::sync::LazyLock;

/// Stable open place in pre-rendered answers. Insert comparison C into every place.
pub const ANSWER_SLOT: &str = "<span data-meguri-type-answer></span>";

static MARKERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[type:([^\n]+?)\]\]").expect("A2 markers"));
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Plain,
    Nc,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// Stored field or C10 cloze extraction, before display transforms.
    pub expected: String,
    pub mode: Mode,
    pub case_insensitive: bool,
    pub font: String,
    pub size: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidReplacement;
impl std::fmt::Display for InvalidReplacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid_type_answer_replacement")
    }
}
impl std::error::Error for InvalidReplacement {}
#[derive(Clone, Copy)]
pub struct Field<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub font: &'a str,
    pub size: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningForm {
    Display,
    Token,
}

/// First marker plus template modifiers. Sections deliberately do not gate A9.
pub fn question(
    text: &str,
    template: &ParsedTemplate,
    fields: &[Field<'_>],
    card_number: u32,
    warnings: WarningForm,
    display_attributes: bool,
) -> Result<(String, Option<Descriptor>), InvalidReplacement> {
    let Some(first) = MARKERS.captures(text) else {
        return Ok((text.to_owned(), None));
    };
    let mut name = &first[1];
    let mut cloze = false;
    let mut mode = Mode::Plain;
    let mut ci = false;
    if let Some(rest) = name.strip_prefix("cloze:") {
        cloze = true;
        name = rest.split(':').next().unwrap_or_default();
    }
    if let Some(rest) = name.strip_prefix("nc:") {
        mode = Mode::Nc;
        name = rest.split(':').next().unwrap_or_default();
    }
    for node in template.nodes() {
        if let NodeKind::Replacement { key, filters } = &node.kind
            && key == name
            && filters.first().is_some_and(|f| f == "type")
        {
            cloze |= filters.iter().any(|f| f == "cloze");
            if filters.iter().any(|f| f == "nc") {
                mode = Mode::Nc;
            }
            ci = filters.iter().any(|f| f == "ci");
            break;
        }
    }
    let field = fields.iter().find(|f| f.name == name);
    let expected = field.map(|f| {
        if cloze {
            crate::cloze::type_answer(f.value, card_number)
        } else {
            f.value.to_owned()
        }
    });
    let missing = expected.as_ref().is_none_or(|s| cloze && s.is_empty());
    let descriptor = if !missing && expected.as_ref().is_some_and(|s| !s.is_empty()) {
        let field = field.expect("field exists");
        Some(Descriptor {
            expected: expected.expect("field exists"),
            mode,
            case_insensitive: ci,
            font: field.font.to_owned(),
            size: field.size,
        })
    } else {
        None
    };
    let replacement = if missing {
        match (warnings, cloze) {
            (WarningForm::Token, true) => "[[warning:empty_cards]]".to_owned(),
            (WarningForm::Token, false) => format!("[[warning:unknown_field:{name}]]"),
            (WarningForm::Display, true) => "Please run Tools>Empty Cards".to_owned(),
            (WarningForm::Display, false) => format!("Type answer: unknown field {name}"),
        }
    } else if let Some(d) = &descriptor {
        format!(
            "\n<center>\n<input type=text id=typeans onkeypress=\"_typeAnsPress();\"\n   style=\"font-family: '{}'; font-size: {}px;\">\n</center>\n",
            d.font, d.size
        )
    } else {
        String::new()
    };
    let replacement = Replacement::parse(&replacement)?;
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for found in MARKERS.captures_iter(text) {
        let whole = found.get(0).expect("whole match");
        out.push_str(&text[at..whole.start()]);
        let mut expanded = String::new();
        replacement.expand(whole.as_str(), &found[1], &mut expanded);
        if display_attributes && descriptor.is_some() {
            // Only the generated input, once. Captured text is never rescanned.
            expanded = expanded.replacen(
                "<input type=text id=typeans",
                "<input type=text id=typeans autocapitalize=\"off\" autocorrect=\"off\" spellcheck=\"false\"",
                1,
            );
        }
        out.push_str(&expanded);
        at = whole.end();
    }
    out.push_str(&text[at..]);
    Ok((out, descriptor))
}

/// `render/type-answer` A11.1: the text shown in place of every type-in
/// marker after a side's first.
pub const PREVIEW_EXTRA_MARKER: &str = "Only the first type-in box of a card template works.";

/// A11.1 (`render/preview` V9): the template preview's simulated type-in
/// on one side's display text. A side without `[[type:` is unchanged.
/// Otherwise every exact `<hr id=answer>` goes first; the first marker
/// becomes the read-only box holding `example` on the question, and the
/// comparison of expected `example` with typed `sample` on the answer
/// (after `<hr id=answer>` when the side had one); every later marker
/// becomes the warning. Nothing reads the note's fields.
pub fn simulate_preview(text: &str, answer_side: bool) -> String {
    if !text.contains("[[type:") {
        return text.to_owned();
    }
    let had_rule = text.contains("<hr id=answer>");
    let source = text.replace("<hr id=answer>", "");
    let first = if answer_side {
        let comparison = compare("example", "sample", Mode::Plain, false);
        format!("{}{comparison}", if had_rule { "<hr id=answer>" } else { "" })
    } else {
        "<center><input id='typeans' type=text value='example' readonly='readonly'></center>".to_owned()
    };
    let later = format!("<center><b>{PREVIEW_EXTRA_MARKER}</b></center>");
    let mut out = String::with_capacity(source.len() + first.len());
    let mut at = 0;
    for (index, found) in MARKERS.find_iter(&source).enumerate() {
        out.push_str(&source[at..found.start()]);
        out.push_str(if index == 0 { &first } else { &later });
        at = found.end();
    }
    out.push_str(&source[at..]);
    out
}

/// A5: insert literal blocks and move exact answer rules when R has a marker.
pub fn answer(text: &str, descriptor: Option<&Descriptor>, typed: &str) -> String {
    let Some(d) = descriptor else {
        return MARKERS.replace_all(text, "").into_owned();
    };
    answer_with_comparison(text, d, &compare_typed_answer(d, typed))
}
/// Also usable at reveal: supply C separately, without interpreting its text.
pub fn answer_with_comparison(text: &str, d: &Descriptor, comparison: &str) -> String {
    let has_rule = text.contains("<hr id=answer>");
    let removed = text.replace("<hr id=answer>", "");
    if has_rule && !MARKERS.is_match(&removed) {
        return text.to_owned();
    }
    let source = if has_rule { &removed } else { text };
    let block = format!(
        "{}\n<div style=\"font-family: '{}'; font-size: {}px\">{comparison}</div>",
        if has_rule { "<hr id=answer>" } else { "" },
        d.font,
        d.size
    );
    MARKERS
        .replace_all(source, |_: &regex::Captures<'_>| block.clone())
        .into_owned()
}
