//! The Cards tab's core reads over FFI (U282, `notetypes/workspace` W5 to
//! W7): each card type's cards (a job on one snapshot), the red live issues
//! of a draft as it would be saved (one snapshot, no note read), the text
//! edit of W7's "Use “<name>”" fix, and the editor's colouring of a
//! template or of the styling. The last three are pure. Ranges cross the
//! bridge as UTF-16 offsets, the text views' own unit.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use meguri_engine::notetypes::card_text::{CssHighlight, css_highlights};
use meguri_engine::notetypes::issues::{self, IssueKind, ParseIssue, TemplateIssue};
use meguri_engine::notetypes::syntax::{TemplateHighlight, template_highlights};
use meguri_engine::notetypes::{TemplateSide, counts};
use meguri_engine::notetypes::structure::StructureDraft;
use meguri_model::ids::NotetypeId;

use crate::jobs::JobHandle;
use crate::notetype_card_text::CardTextColour;
use crate::notetype_structure::NotetypeStructureDraft;
use crate::{CoreError, callback, runtime};

#[derive(Debug, uniffi::Object)]
pub struct NotetypeTemplateCardCountsJob {
    result: Arc<Mutex<Option<Vec<u64>>>>,
}

#[uniffi::export]
impl NotetypeTemplateCardCountsJob {
    pub fn result(&self) -> Result<Option<Vec<u64>>, CoreError> {
        callback::enter("NotetypeTemplateCardCountsJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeTemplateCardCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<NotetypeTemplateCardCountsJob>,
}

/// W5's counts on the job pool: per stored template ordinal, its cards;
/// `NotFound` for an unknown note type.
#[uniffi::export]
pub fn start_notetype_template_card_counts(notetype_id: i64) -> Result<NotetypeTemplateCardCountsJobStart, CoreError> {
    callback::enter("start_notetype_template_card_counts");
    let collection = runtime::host()?.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let read = counts::template_card_counts(&collection, NotetypeId(notetype_id), &job.cancel, &job.progress)?;
        *out.lock().unwrap() = Some(read);
        Ok(())
    })?;
    Ok(NotetypeTemplateCardCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(NotetypeTemplateCardCountsJob { result }),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeTemplatePart {
    Front,
    Back,
}

/// What a red issue says (W7), with what its fix needs.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeTemplateIssueKind {
    /// The front repeats the card type at `earlier` (a draft position).
    DuplicateFront { earlier: u32 },
    /// A section left open: `tag` as written, `close` the tag the fix
    /// appends at the end of the part.
    UnclosedSection { tag: String, close: String },
    /// A close with no open section.
    UnopenedSection { tag: String },
    /// A close that does not end the innermost open section `open`.
    MismatchedSection { tag: String, open: String },
    /// A structural error whose tag could not be read back.
    UnmatchedSection,
    NoFrontField,
    /// `name` is no field; `suggestion` is the position in the draft's
    /// fields of W7.1's near name.
    UnknownField { name: String, suggestion: Option<u32> },
    MissingCloze,
}

/// One red issue, in `notetypes/validation` V4's order.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeTemplateIssue {
    /// The card type's position in the draft's templates.
    pub template: u32,
    pub part: Option<NotetypeTemplatePart>,
    /// The 1-based line of the part's draft text; none for the whole part.
    pub line: Option<u32>,
    pub kind: NotetypeTemplateIssueKind,
}

fn position(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

impl From<TemplateIssue> for NotetypeTemplateIssue {
    fn from(issue: TemplateIssue) -> Self {
        use NotetypeTemplateIssueKind as K;
        let kind = match issue.kind {
            IssueKind::DuplicateFront { earlier } => K::DuplicateFront { earlier: position(earlier) },
            IssueKind::Parse(ParseIssue::Unclosed { tag, close }) => K::UnclosedSection { tag, close },
            IssueKind::Parse(ParseIssue::Unopened { tag }) => K::UnopenedSection { tag },
            IssueKind::Parse(ParseIssue::Mismatched { tag, open }) => K::MismatchedSection { tag, open },
            IssueKind::Parse(ParseIssue::Unmatched) => K::UnmatchedSection,
            IssueKind::NoFrontField => K::NoFrontField,
            IssueKind::UnknownField { name, suggestion } => K::UnknownField {
                name,
                suggestion: suggestion.map(position),
            },
            IssueKind::MissingCloze => K::MissingCloze,
        };
        Self {
            template: position(issue.template),
            part: issue.side.map(|side| match side {
                TemplateSide::Front => NotetypeTemplatePart::Front,
                TemplateSide::Back => NotetypeTemplatePart::Back,
            }),
            line: issue.line,
            kind,
        }
    }
}

/// W7's red issues of the draft as it would be saved, on one snapshot.
/// Refuses as the draft's planning does (`Stale`, `InvalidArgument`,
/// `NotetypeRefused`).
#[uniffi::export]
pub fn notetype_template_issues(draft: NotetypeStructureDraft) -> Result<Vec<NotetypeTemplateIssue>, CoreError> {
    callback::enter("notetype_template_issues");
    let draft = StructureDraft::try_from(draft)?;
    let collection = runtime::host()?.collection()?;
    Ok(issues::template_issues(&collection, &draft)?
        .into_iter()
        .map(NotetypeTemplateIssue::from)
        .collect())
}

/// W7's "Use “<to>”" fix on one part's text: every reference to `from`
/// names `to`; nothing else changes. Pure.
#[uniffi::export]
pub fn notetype_replace_field_references(text: String, from: String, to: String) -> Result<String, CoreError> {
    callback::enter("notetype_replace_field_references");
    Ok(issues::replace_field_references(&text, &from, &to))
}

/// The kind of a coloured range (W6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotetypeSyntaxKind {
    Field,
    Section,
    Comment,
    Tag,
    Directive,
    AtRule,
    Property,
    Value,
    /// A colour value, with the colour its swatch shows.
    Colour { colour: CardTextColour },
}

/// One coloured range, in UTF-16 code units of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeSyntaxSpan {
    pub start: u32,
    pub end: u32,
    pub kind: NotetypeSyntaxKind,
}

/// Maps byte ranges of `text` to UTF-16 ranges in one pass over the text.
fn utf16_spans(text: &str, spans: Vec<(Range<usize>, NotetypeSyntaxKind)>) -> Vec<NotetypeSyntaxSpan> {
    let mut offsets: Vec<usize> = spans.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut units = Vec::with_capacity(offsets.len());
    let mut wanted = offsets.iter().peekable();
    let mut count = 0u32;
    for (at, ch) in text.char_indices().chain(std::iter::once((text.len(), '\0'))) {
        while let Some(&&offset) = wanted.peek() {
            if offset > at {
                break;
            }
            units.push((offset, count));
            wanted.next();
        }
        count = count.saturating_add(u32::try_from(ch.len_utf16()).unwrap_or(2));
    }
    let unit = |offset: usize| {
        units
            .binary_search_by_key(&offset, |(o, _)| *o)
            .map(|i| units[i].1)
            .unwrap_or(count)
    };
    spans
        .into_iter()
        .map(|(range, kind)| NotetypeSyntaxSpan {
            start: unit(range.start),
            end: unit(range.end),
            kind,
        })
        .collect()
}

/// W6's colouring of a Front or Back text. Pure.
#[uniffi::export]
pub fn notetype_template_syntax(text: String) -> Result<Vec<NotetypeSyntaxSpan>, CoreError> {
    callback::enter("notetype_template_syntax");
    let spans = template_highlights(&text)
        .into_iter()
        .map(|(range, highlight)| {
            let kind = match highlight {
                TemplateHighlight::Field => NotetypeSyntaxKind::Field,
                TemplateHighlight::Section => NotetypeSyntaxKind::Section,
                TemplateHighlight::Comment => NotetypeSyntaxKind::Comment,
                TemplateHighlight::Tag => NotetypeSyntaxKind::Tag,
                TemplateHighlight::Directive => NotetypeSyntaxKind::Directive,
            };
            (range, kind)
        })
        .collect();
    Ok(utf16_spans(&text, spans))
}

/// W6's colouring of the styling, with each colour value's colour. Pure.
#[uniffi::export]
pub fn notetype_styling_syntax(css: String) -> Result<Vec<NotetypeSyntaxSpan>, CoreError> {
    callback::enter("notetype_styling_syntax");
    let spans = css_highlights(&css)
        .into_iter()
        .map(|(range, highlight)| {
            let kind = match highlight {
                CssHighlight::Comment => NotetypeSyntaxKind::Comment,
                CssHighlight::AtRule => NotetypeSyntaxKind::AtRule,
                CssHighlight::Property => NotetypeSyntaxKind::Property,
                CssHighlight::Value => NotetypeSyntaxKind::Value,
                CssHighlight::Colour(colour) => NotetypeSyntaxKind::Colour { colour: colour.into() },
            };
            (range, kind)
        })
        .collect();
    Ok(utf16_spans(&css, spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cross_the_bridge_in_utf16_units() {
        // "é" is one UTF-16 unit and two bytes; "𝄞" two units and four bytes.
        let spans = notetype_template_syntax("é𝄞{{Front}}<b>".into()).unwrap();
        assert_eq!(
            spans,
            vec![
                NotetypeSyntaxSpan {
                    start: 3,
                    end: 12,
                    kind: NotetypeSyntaxKind::Field
                },
                NotetypeSyntaxSpan {
                    start: 12,
                    end: 15,
                    kind: NotetypeSyntaxKind::Tag
                },
            ]
        );
        let css = notetype_styling_syntax("/*é*/ .card { color: red }".into()).unwrap();
        assert_eq!((css[0].start, css[0].end), (0, 5));
        assert_eq!(css.last().map(|s| s.kind), Some(NotetypeSyntaxKind::Colour {
            colour: CardTextColour {
                red: 255,
                green: 0,
                blue: 0,
                alpha: 255
            }
        }));
    }

    #[test]
    fn the_use_fix_renames_one_field_s_references() {
        assert_eq!(
            notetype_replace_field_references("{{Wrod}} {{#Wrod}}{{/Wrod}}".into(), "Wrod".into(), "Word".into()).unwrap(),
            "{{Word}} {{#Word}}{{/Word}}"
        );
    }
}
