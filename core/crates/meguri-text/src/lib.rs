//! Text primitives that stored values depend on, bit-identical to the oracle:
//! HTML stripping and entities, field save normalization, sort-field text and
//! checksum, field emptiness, media references and their IRI encoding, the
//! media file name transform, the `unicase` comparison and GUID text. Also
//! Meguri's own deck description renderer.
//!
//! Each function names the section of `specs/text/` it implements. The
//! functions take one value and return one value; composing them into what a
//! note save stores (`checksum-and-sort-field.md` C1 first, then C2 or C4) is
//! the caller's job.

pub mod base91;
pub mod checksum;
pub mod description;
pub mod entities;
pub mod field_empty;
pub mod fold;
pub mod html;
pub mod iri;
pub mod media_name;
pub mod media_refs;
pub mod normalize;
mod unicode10_unassigned;
