# Oracle driver provenance: card excerpts (U85a)

The prepared adapter was resumed from 4374e4a1, with the oracle pinned to
26.09.3. It implements the neutral contract in
`specs/conformance/card-excerpt-ops.md` E1–E3 through the pinned collection
library. No reference implementation was copied into Meguri targets.

The original preparation consulted these pinned public source locations:

| Location | Purpose |
|---|---|
| `rslib/src/browser_table.rs` | Question and Deck cell values, row admission, configuration and transient column selection |
| `rslib/src/notetype/mod.rs` | Missing-template behavior |
| `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs` | Per-row failure classification |
| `pylib/anki/_backend.py`, `pylib/anki/_backend_generated.py` | Python call boundary and error transport |
| `pylib/anki/collection.py`, `proto/anki/config.proto` | Cards-mode admission |

The prepared seven-test suite covers ordered and repeated references,
Question formatting, damaged and removed records, whole-run failures,
request validation, notes-mode refusal and unchanged collection output
after timing initialization. During resume, its synthetic labels were
independently renamed after the normal privacy hook rejected locations
without revealing a private token. Assertions retain the same semantics.
