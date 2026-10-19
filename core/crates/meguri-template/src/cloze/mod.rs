//! Cloze deletions (`specs/render/cloze.md`): the `cloze:` and
//! `cloze-only:` filters, the cloze numbers of a text and the answer a cloze
//! type-in field expects. Every output must equal the oracle's string for
//! string (R1907).
//!
//! A text is read once into a tree of top-level items ([`tokenizer`], C2-C4);
//! each function walks that tree. No function fails: every string is a valid
//! text (C12).

mod numbers;
mod render;
pub(crate) mod tokenizer;
mod typing;

pub use numbers::{has_cloze, numbers};
pub use render::{render, render_only};
pub use typing::type_answer;

/// The side a cloze filter renders for (C1): the front template evaluates
/// `{{cloze:F}}` with [`Side::Front`], the back template, `{{FrontSide}}`
/// included, with [`Side::Back`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}
