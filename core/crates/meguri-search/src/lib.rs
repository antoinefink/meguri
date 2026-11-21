//! Search: lexer and parser, canonical writer, SQL writer, matchers, browser
//! rows and accelerators.
//!
//! U80 (`specs/search/grammar.md`, `errors.md`, `canonical-form.md`,
//! `query-model.md`): search text becomes a typed [`Query`] or a
//! [`SearchError`] with Anki's kind and details and Meguri's own range and
//! message; [`Query::canonical_text`] writes Anki's canonical text; a
//! [`Plan`] compiles a query for cards mode or notes mode against a
//! [`SearchContext`] and the collection's [`SearchNotetypes`]. Nothing here
//! recurses on the input's nesting depth.
//!
//! U81 and U82 (`text-matching.md`, `fields.md`, `case-folding.md`,
//! `names-and-ids.md`, `state-and-flags.md`, `properties.md`,
//! `history.md`): every term compiles to SQL over the product's functions
//! ([`sqlfns`]).
//!
//! U83 (`specs/browser/config-keys.md`): [`config`] reads the browser and
//! search config keys and computes what each user action writes.
//!
//! U88a (`specs/search/join-and-replace.md` J1-J5, J8;
//! `specs/search/editing-services.md` D5, D6.7): [`builder`] writes the
//! search text of built items, [`tail`] classifies the text at the cursor
//! while the user types, and [`name_index`] holds the names suggestions
//! read.

pub mod ast;
pub mod browser;
pub mod builder;
pub mod canonical;
pub mod config;
pub mod context;
pub mod custom_data;
pub mod error;
mod lexer;
pub mod name_index;
pub mod notetypes;
mod parser;
mod pattern;
pub mod plan;
pub mod sqlfns;
pub mod tail;

pub use ast::{Node, NodeId, NodeKind, Query, Scope, Term};
pub use builder::{BuiltItem, BuilderError, InvalidItem, Joiner, build, build_query};
pub use canonical::canonical_text;
pub use context::{ContextReads, ContextValue, FoldMode, SearchContext};
pub use error::{ErrorKind, SearchError, SearchFailure};
pub use notetypes::{FieldShape, NotetypeShape, SearchNotetypes};
pub use parser::parse;
pub use plan::{BuildError, Param, Plan, Rows, SearchMode};
