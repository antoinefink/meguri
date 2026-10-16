//! The private collection vault (U15, R1938–R1940): read-only access to the
//! user's collection and per-run copies ([`vault`]), the strings the output
//! guard looks for ([`redact`]), and the private suite's runner and summary
//! ([`suite`]).
//!
//! `tools/mg private` is the only caller that points these at the user's
//! collection: it mounts `local-data/anki/` read-only in a container without
//! a network and works on the copy (`tools/AGENTS.md`). Nothing here prints
//! a deck name, tag, field text or media name: errors and reports carry
//! counts, sizes, hashes and fixed names only, because job logs and results
//! are read by agents (R1939).

pub mod redact;
pub mod suite;
pub mod vault;
