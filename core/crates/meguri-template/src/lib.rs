//! Card templates: parser and renderer, filters, cloze, type-in answers,
//! image occlusion and previews.
//!
//! The template engine (U60) follows `specs/render/template-syntax.md`,
//! `filters.md` and `special-fields.md`; its output must equal the oracle's
//! string for string (R1907). A template parses once into a
//! [`ParsedTemplate`] (never failing: structural errors are reported beside
//! the nodes) and renders many times against a [`FieldMap`]. The same parse
//! answers card generation's and validation's questions without rendering
//! ([`ParsedTemplate::renders_something`], [`legacy_requirement`]), and its
//! nodes carry their byte ranges in the source for reference rewriting and
//! highlighting.
//!
//! Each side is rendered on its own: composing a card (the blank check, which
//! side's error counts, the FrontSide text the answer sees) is the caller's,
//! as `template-syntax.md` T9-T10 describe.

pub mod analysis;
pub mod cloze;
pub mod error;
pub mod filters;
pub mod image_occlusion;
pub mod parser;
pub mod render;

pub use analysis::{Form, LegacyRequirement, RequirementKind, legacy_requirement};
pub use error::{ErrorKind, TemplateError};
pub use parser::{Delimiters, Node, NodeKind, ParsedTemplate};
pub use render::{CardSpecials, FieldMap, Side};
