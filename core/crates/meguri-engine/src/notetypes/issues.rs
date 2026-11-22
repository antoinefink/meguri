//! The Cards tab's red live issues (`notetypes/workspace` W7): every
//! failure `notetypes/validation` V4 finds on the draft as it would be
//! saved, each with the line of the draft's own text it points at and the
//! one-click fix W7 offers, and the fixes themselves as pure text edits.
//!
//! The failures are V4's on the saved note type the save would store (the
//! draft on its base, normalized and with field renames applied, S3 steps 1
//! to 4), so renaming `Front` to `Prompt` reports no `{{Front}}`. The lines
//! are read in the draft's texts, which are what the editor shows: the
//! rewrite leaves a text with a structural error byte for byte, and never
//! touches a tag whose key names no base field (`rewrite`), so the failing
//! tag of a parse error or an unknown field is in the draft's text too.

use std::ops::Range;

use meguri_storage::{ReadRows, ReaderRole};
use meguri_template::{ErrorKind, NodeKind, ParsedTemplate};

use super::structure::StructureDraft;
use super::structure::plan;
use super::validate::{CheckKind, TemplateSide, template_failures};
use crate::{Collection, EngineError};

/// A structural error of one side (`render/template-syntax` T6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseIssue {
    /// A section left open. `tag` is the open tag as written;
    /// `close` the tag that closes it, in the text's delimiters, which the
    /// fix appends at the end of the part.
    Unclosed { tag: String, close: String },
    /// A close with no open section; `tag` as written.
    Unopened { tag: String },
    /// A close that does not end the innermost open section `open`.
    Mismatched { tag: String, open: String },
    /// A structural error whose tag could not be read back (not reached:
    /// the draft's text holds the saved text's error).
    Unmatched,
}

/// What an issue says, with what its fix needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// The front repeats the card type at `earlier` (a position in the draft).
    DuplicateFront { earlier: usize },
    Parse(ParseIssue),
    NoFrontField,
    /// `name` is no field; `suggestion` is the position in the draft's
    /// fields of W7.1's near name, if any.
    UnknownField { name: String, suggestion: Option<usize> },
    MissingCloze,
}

/// One red issue (W7), in V4's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateIssue {
    /// The card type's position in the draft's templates.
    pub template: usize,
    pub side: Option<TemplateSide>,
    /// The 1-based line of the side's draft text the failing tag starts on;
    /// none for an issue about the whole part.
    pub line: Option<u32>,
    pub kind: IssueKind,
}

/// W7's red issues of `draft` on `reader`'s rows. Refuses as the draft's
/// planning does before it counts (`InvalidArgument`, `Stale`,
/// `invalid_notetype`); reads no note and writes nothing.
pub fn draft_issues(reader: &impl ReadRows, draft: &StructureDraft) -> Result<Vec<TemplateIssue>, EngineError> {
    let base = plan::checked_base(reader, draft)?;
    let layout = plan::layout_of_draft(reader, base, draft)?;
    let fields: Vec<&str> = draft.fields.iter().map(|f| f.name.as_str()).collect();
    Ok(template_failures(&layout.saved)
        .into_iter()
        .map(|failure| {
            let text = draft.templates.get(failure.template).map(|t| match failure.side {
                Some(TemplateSide::Back) => t.back.as_str(),
                _ => t.front.as_str(),
            });
            let parsed = text.map(ParsedTemplate::parse);
            let (kind, at) = match failure.kind {
                CheckKind::DuplicateFront => (IssueKind::DuplicateFront { earlier: failure.earlier.unwrap_or(0) }, None),
                CheckKind::NoFrontField => (IssueKind::NoFrontField, None),
                CheckKind::MissingCloze => (IssueKind::MissingCloze, None),
                CheckKind::UnknownField => {
                    let name = failure.name.unwrap_or_default();
                    let at = parsed.as_ref().and_then(|p| first_reference(p, &name));
                    let suggestion = super::counts::near_field_name(&name, &fields);
                    (IssueKind::UnknownField { name, suggestion }, at)
                }
                CheckKind::TemplateParseError => match parsed.as_ref().and_then(parse_issue) {
                    Some((issue, at)) => (IssueKind::Parse(issue), Some(at)),
                    None => (IssueKind::Parse(ParseIssue::Unmatched), None),
                },
            };
            let line = at.and_then(|at| text.map(|t| line_of(t, at)));
            TemplateIssue {
                template: failure.template,
                side: failure.side,
                line,
                kind,
            }
        })
        .collect())
}

/// [`draft_issues`] on one snapshot of the collection.
pub fn template_issues(collection: &Collection, draft: &StructureDraft) -> Result<Vec<TemplateIssue>, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    draft_issues(&*snapshot, draft)
}

/// The byte where the first reference (T11.1) to `name` starts.
fn first_reference(parsed: &ParsedTemplate, name: &str) -> Option<usize> {
    parsed.nodes().iter().find_map(|node| match &node.kind {
        NodeKind::Replacement { key, .. } | NodeKind::Open { key, .. } if key == name => Some(node.span.start),
        _ => None,
    })
}

/// The side's structural error and the byte its tag starts at.
fn parse_issue(parsed: &ParsedTemplate) -> Option<(ParseIssue, usize)> {
    let error = parsed.structure_error()?;
    let tag = parsed.source().get(error.span.clone())?.to_owned();
    let issue = match error.kind {
        ErrorKind::UnclosedConditional => {
            let key = parsed.nodes().iter().find_map(|node| match &node.kind {
                NodeKind::Open { key, .. } if node.span == error.span => Some(key.as_str()),
                _ => None,
            })?;
            let delimiters = parsed.delimiters();
            ParseIssue::Unclosed {
                tag,
                close: format!("{}/{key}{}", delimiters.open(), delimiters.close()),
            }
        }
        ErrorKind::UnopenedConditional => ParseIssue::Unopened { tag },
        ErrorKind::MismatchedConditional => {
            // The innermost open section the close should have ended.
            let open = open_before(parsed, error.span.start).unwrap_or_default();
            ParseIssue::Mismatched { tag, open }
        }
        ErrorKind::UnknownField | ErrorKind::UnknownConditional => return None,
    };
    Some((issue, error.span.start))
}

/// The innermost section still open at `before`, as written.
fn open_before(parsed: &ParsedTemplate, before: usize) -> Option<String> {
    let mut open: Vec<Range<usize>> = Vec::new();
    for node in parsed.nodes().iter().take_while(|node| node.span.start < before) {
        match node.kind {
            NodeKind::Open { .. } => open.push(node.span.clone()),
            NodeKind::Close { .. } => {
                open.pop();
            }
            _ => {}
        }
    }
    open.pop().and_then(|span| parsed.source().get(span).map(str::to_owned))
}

/// The 1-based line `at` lies on: LF, CR LF and a lone CR end a line.
fn line_of(text: &str, at: usize) -> u32 {
    let before = text.get(..at).unwrap_or(text).as_bytes();
    let mut line = 1u32;
    for (i, byte) in before.iter().enumerate() {
        match byte {
            b'\n' => line = line.saturating_add(1),
            b'\r' if before.get(i + 1) != Some(&b'\n') => line = line.saturating_add(1),
            _ => {}
        }
    }
    line
}

/// W7's "Use “<name>”": every reference to the field `from` in `text`
/// (replacements, section opens and closes; comments excluded) names `to`
/// instead. Only the key's characters change; filters, spacing and every
/// other byte stay as written.
pub fn replace_field_references(text: &str, from: &str, to: &str) -> String {
    let parsed = ParsedTemplate::parse(text);
    let (open, close) = (parsed.delimiters().open(), parsed.delimiters().close());
    let mut output = String::with_capacity(text.len());
    let mut through = 0;
    for node in parsed.nodes() {
        let key = match &node.kind {
            NodeKind::Replacement { key, .. } | NodeKind::Open { key, .. } | NodeKind::Close { key } => key,
            _ => continue,
        };
        if key != from {
            continue;
        }
        // The key is the last piece of the trimmed tag content (T4, T5), so
        // it ends where that content's trailing white space starts.
        let inner = node.span.start + open.len()..node.span.end - close.len();
        let Some(content) = text.get(inner.clone()) else { continue };
        let end = inner.start + content.trim_end().len();
        let Some(start) = end.checked_sub(key.len()).filter(|s| text.get(*s..end) == Some(key.as_str())) else {
            continue;
        };
        output.push_str(&text[through..start]);
        output.push_str(to);
        through = end;
    }
    output.push_str(&text[through..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_count_lf_crlf_and_lone_cr() {
        assert_eq!(line_of("a\nb\r\nc\rd", 0), 1);
        assert_eq!(line_of("a\nb\r\nc\rd", 2), 2);
        assert_eq!(line_of("a\nb\r\nc\rd", 5), 3);
        assert_eq!(line_of("a\nb\r\nc\rd", 7), 4);
    }

    #[test]
    fn a_fix_renames_only_the_keys_of_the_named_field() {
        let text = "{{Frnt}} {{ hint:Frnt }} {{#Frnt}}x{{/Frnt}} <!--{{Frnt}}--> {{Frnt2}} Frnt";
        assert_eq!(
            replace_field_references(text, "Frnt", "Front"),
            "{{Front}} {{ hint:Front }} {{#Front}}x{{/Front}} <!--{{Frnt}}--> {{Frnt2}} Frnt"
        );
        assert_eq!(
            replace_field_references("{{=<% %>=}}<% Frnt %> {{Frnt}}", "Frnt", "Front"),
            "{{=<% %>=}}<% Front %> {{Frnt}}"
        );
        assert_eq!(replace_field_references("{{{Frnt}}}", "Frnt", "Front"), "{{{Front}}}");
        assert_eq!(replace_field_references("no tags", "a", "b"), "no tags");
    }

    #[test]
    fn parse_issues_name_their_tag_and_the_close_that_fixes_them() {
        let unclosed = ParsedTemplate::parse("x\n{{#Reading}}\n{{Reading}}");
        assert_eq!(
            parse_issue(&unclosed),
            Some((
                ParseIssue::Unclosed {
                    tag: "{{#Reading}}".into(),
                    close: "{{/Reading}}".into()
                },
                2
            ))
        );
        let alternate = ParsedTemplate::parse("{{=<% %>=}}<%^ Blank %>");
        assert!(matches!(
            parse_issue(&alternate),
            Some((ParseIssue::Unclosed { ref close, .. }, _)) if close == "<%/Blank%>"
        ));
        let unopened = ParsedTemplate::parse("a {{/Gone}}");
        assert_eq!(
            parse_issue(&unopened),
            Some((ParseIssue::Unopened { tag: "{{/Gone}}".into() }, 2))
        );
        let mismatched = ParsedTemplate::parse("{{#A}}{{#B}}{{/A}}{{/B}}");
        assert_eq!(
            parse_issue(&mismatched),
            Some((
                ParseIssue::Mismatched {
                    tag: "{{/A}}".into(),
                    open: "{{#B}}".into()
                },
                12
            ))
        );
    }
}
