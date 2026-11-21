//! The canonical search text (`specs/search/canonical-form.md`): the text a
//! committed search is replaced with, byte-identical to Anki's. A separate
//! pass over the query that produces quoting and escaping anew (F1); it
//! walks with an explicit stack, so no nesting depth exhausts the call stack.

use crate::ast::{BoolOperator, NodeKind, Property, Query, TemplateRef, Term, TextMode};
use crate::error::SearchError;
use crate::parser::parse;

/// Parses `text` and writes its canonical text; a text that does not parse
/// has none (F1).
pub fn canonical_text(text: &str) -> Result<String, SearchError> {
    Ok(parse(text)?.canonical_text())
}

enum Piece {
    Node(crate::ast::NodeId),
    Text(&'static str),
}

impl Query {
    /// F2-F6.
    pub fn canonical_text(&self) -> String {
        // F2.2: a search that is one group loses one level of brackets; a
        // written-text item (`search/filter-tokens` T3.2) is written as it
        // stands.
        let start = match self.node(self.root()).kind {
            NodeKind::Group(inside) if self.written_text(self.root()).is_none() => inside,
            _ => self.root(),
        };
        self.canonical_text_of(start)
    }

    /// F3-F6 for the item at `id` as it stands inside a longer search: a
    /// group keeps its brackets. A built query's written-text items (T3.2)
    /// are written unchanged.
    pub fn canonical_text_of(&self, id: crate::ast::NodeId) -> String {
        let mut out = String::new();
        let mut stack = vec![Piece::Node(id)];
        while let Some(piece) = stack.pop() {
            let id = match piece {
                Piece::Text(text) => {
                    out.push_str(text);
                    continue;
                }
                Piece::Node(id) => id,
            };
            if let Some(written) = self.written_text(id) {
                out.push_str(written);
                continue;
            }
            match &self.node(id).kind {
                NodeKind::And(items) => joined(&mut stack, items, " "),
                NodeKind::Or(runs) => joined(&mut stack, runs, " OR "),
                NodeKind::Group(inside) => {
                    stack.push(Piece::Text(")"));
                    stack.push(Piece::Node(*inside));
                    stack.push(Piece::Text("("));
                }
                NodeKind::Not(item) => {
                    stack.push(Piece::Node(*item));
                    stack.push(Piece::Text("-"));
                }
                NodeKind::NegatedOperator(BoolOperator::And) => out.push_str("- "),
                NodeKind::NegatedOperator(BoolOperator::Or) => out.push_str("- OR "),
                NodeKind::Term(term) => out.push_str(&term_text(term)),
            }
        }
        out
    }
}

/// Pushes `items` with `joiner` between them, so they pop in order.
fn joined(stack: &mut Vec<Piece>, items: &[crate::ast::NodeId], joiner: &'static str) {
    for (k, item) in items.iter().enumerate().rev() {
        stack.push(Piece::Node(*item));
        if k > 0 {
            stack.push(Piece::Text(joiner));
        }
    }
}

/// F4: quotes when the text is an operator word, starts with `-` and goes on
/// with anything but a line feed, or holds a space, U+3000 or a bracket;
/// every `"` escaped either way.
pub fn quoted(text: &str) -> String {
    let mut chars = text.chars();
    let needs_quotes = text.eq_ignore_ascii_case("and")
        || text.eq_ignore_ascii_case("or")
        || (chars.next() == Some('-') && chars.next().is_some_and(|c| c != '\n'))
        || text.contains([' ', '\u{3000}', '(', ')']);
    let escaped = text.replace('"', "\\\"");
    if needs_quotes {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

fn mode_prefix(mode: TextMode) -> &'static str {
    match mode {
        TextMode::Normal => "",
        TextMode::Regex => "re:",
        TextMode::NoCombining => "nc:",
    }
}

/// F5, field row: `name:prefix+text`, colons in the name escaped, and in
/// normal mode a text that would read as a mode gets its first colon
/// escaped.
fn field_text(name: &str, mode: TextMode, text: &str) -> String {
    let name = name.replace(':', "\\:");
    let text = if mode == TextMode::Normal && (text.starts_with("re:") || text.starts_with("nc:")) {
        text.replacen(':', "\\:", 1)
    } else {
        text.to_owned()
    };
    quoted(&format!("{name}:{}{text}", mode_prefix(mode)))
}

/// F5 and F6 for one term.
pub(crate) fn term_text(term: &Term) -> String {
    match term {
        Term::WholeCollection => "deck:*".to_owned(),
        Term::Text(text) => quoted(&text.replace(':', "\\:")),
        Term::Field { name, mode, text } => field_text(name, *mode, text),
        Term::Tag { mode, text } => field_text("tag", *mode, text),
        Term::Deck(text) => quoted(&format!("deck:{text}")),
        Term::Notetype(text) => quoted(&format!("note:{text}")),
        Term::Regex(text) => quoted(&format!("re:{text}")),
        Term::NoCombining(text) => quoted(&format!("nc:{text}")),
        Term::StripCloze(text) => quoted(&format!("sc:{text}")),
        Term::Word(text) => quoted(&format!("w:{text}")),
        Term::HasCustomData(text) => quoted(&format!("has-cd:{text}")),
        Term::Preset(text) => quoted(&format!("preset:{text}")),
        Term::Template(TemplateRef::Ordinal(ordinal)) => format!("card:{}", u32::from(*ordinal) + 1),
        Term::Template(TemplateRef::Name(name)) => quoted(&format!("card:{name}")),
        Term::Flag(flag) => format!("flag:{flag}"),
        Term::State(state) => format!("is:{}", state.word()),
        Term::Added(days) => format!("added:{days}"),
        Term::Edited(days) => format!("edited:{days}"),
        Term::Introduced(days) => format!("introduced:{days}"),
        Term::Rated { days, button } => match button {
            Some(button) => format!("rated:{days}:{button}"),
            None => format!("rated:{days}"),
        },
        Term::Rescheduled(days) => format!("resched:{days}"),
        Term::Ids { kind, list } => format!("{}:{list}", kind.key()),
        Term::NotetypeId(id) => format!("mid:{id}"),
        Term::Duplicates { notetype, text } => quoted(&format!("dupe:{notetype},{}", text.replace('\\', "\\\\"))),
        Term::Property { property, comparison } => {
            let op = comparison.symbol();
            match property {
                Property::Interval(n) => format!("prop:ivl{op}{n}"),
                Property::Due(n) => format!("prop:due{op}{n}"),
                Property::Reps(n) => format!("prop:reps{op}{n}"),
                Property::Lapses(n) => format!("prop:lapses{op}{n}"),
                Property::Ease(x) => format!("prop:ease{op}{x}"),
                Property::Position(n) => format!("prop:pos{op}{n}"),
                Property::Rated { days, button } => match button {
                    Some(button) => format!("prop:rated{op}{days}:{button}"),
                    None => format!("prop:rated{op}{days}"),
                },
                Property::Rescheduled(days) => format!("prop:resched{op}{days}"),
                Property::Stability(x) => format!("prop:s{op}{x}"),
                Property::Difficulty(x) => format!("prop:d{op}{x}"),
                Property::Retrievability(x) => format!("prop:r{op}{x}"),
                Property::CustomNumber { key, value } => format!("prop:cdn:{key}{op}{value}"),
                Property::CustomString { key, value } => quoted(&format!("prop:cds:{key}{op}{value}")),
            }
        }
    }
}
