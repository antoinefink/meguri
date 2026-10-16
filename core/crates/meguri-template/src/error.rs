//! Template errors (`specs/render/template-syntax.md` T6, T8, T9).
//!
//! Callers compare the kind, never the wording (R1887). The message is
//! Meguri's own (R959): it names the offending tag, written with `{{ }}`
//! whatever the template's delimiters (T2). Which side failed is the
//! caller's to add.

use std::fmt;
use std::ops::Range;

/// The five kinds of T9; the list is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// T6: the template ends inside an open section.
    UnclosedConditional,
    /// T6: a close with no open section.
    UnopenedConditional,
    /// T6: a close whose key differs from the innermost open section's.
    MismatchedConditional,
    /// T7, T8: a replacement whose key is not in the field map.
    UnknownField,
    /// T7, T8: a section key that is neither in the field map nor a card
    /// number.
    UnknownConditional,
}

impl ErrorKind {
    /// The kind's name in observations (`specs/conformance/render-ops.md` Q2).
    pub const fn name(self) -> &'static str {
        match self {
            ErrorKind::UnclosedConditional => "unclosed_conditional",
            ErrorKind::UnopenedConditional => "unopened_conditional",
            ErrorKind::MismatchedConditional => "mismatched_conditional",
            ErrorKind::UnknownField => "unknown_field",
            ErrorKind::UnknownConditional => "unknown_conditional",
        }
    }

    /// Whether T6 finds it before anything renders.
    pub const fn is_structural(self) -> bool {
        matches!(
            self,
            ErrorKind::UnclosedConditional | ErrorKind::UnopenedConditional | ErrorKind::MismatchedConditional
        )
    }
}

/// One side's error: its kind and the tag that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub kind: ErrorKind,
    /// The offending tag's byte range in the template source: the section
    /// left open, the close that does not fit, the unknown replacement or
    /// section.
    pub span: Range<usize>,
    /// That tag, as `{{…}}`.
    pub tag: String,
    /// For [`ErrorKind::MismatchedConditional`], the open section the close
    /// should have ended, as `{{#…}}` or `{{^…}}`.
    pub open: Option<String>,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = &self.tag;
        match (self.kind, &self.open) {
            (ErrorKind::UnclosedConditional, _) => write!(f, "The section {tag} has no closing tag."),
            (ErrorKind::UnopenedConditional, _) => {
                write!(f, "{tag} closes a section that was never opened.")
            }
            (ErrorKind::MismatchedConditional, Some(open)) => {
                write!(f, "{tag} cannot close {open}: close the inner section first.")
            }
            (ErrorKind::MismatchedConditional, None) => write!(f, "{tag} closes the wrong section."),
            (ErrorKind::UnknownField, _) => write!(f, "{tag} uses a field this note type does not have."),
            (ErrorKind::UnknownConditional, _) => {
                write!(f, "The section {tag} tests a field this note type does not have.")
            }
        }
    }
}

impl std::error::Error for TemplateError {}
