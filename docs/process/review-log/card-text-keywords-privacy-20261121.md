# Public-file admission: Card Text CSS keywords, 2026-11-21

Routine privacy admission under `docs/process/provenance.md` ("Routine privacy admissions").

- Path: `core/crates/meguri-engine/src/notetypes/card_text/keywords.rs`
- Reviewed digest (SHA-256): `88590bbdd44094ec0fdc7ae5942ca26bfa4fcb22810067ee3cb68e72e9a86893` (162 lines, 4781 bytes)
- Author: the U268 implementer session `s-20261121T111906Z-2a644cab`.
- Reviewer: a separate read-only agent session that did not write the file. It read all 162 lines, computed the same digest, and opened no private input, `refs/`, `oracle/` or Anki source. It did not look for the token the scanner matched.
- Decision: PUBLIC.

What the file holds: a module comment citing three CSS standards, two keyword constants, and a table of the CSS named colours with their standard values. The reviewer checked the table against the standard: 148 entries, all unique, none absent, every value correct. `notetypes/workspace` W10.2 requires each item.

Authorship and licence: the vocabulary and values of a public W3C standard, written from the standard; nothing in the file looks copied from a particular implementation.

The orchestrator (Claude, coordinator) verified the bytes and the path and added this one whole-file entry to `PUBLIC_ARTIFACTS` in `tools/scripts/private-scan/private_scan.py`. No path pattern or partial exemption was added. Any later change to the file needs a new complete review.
