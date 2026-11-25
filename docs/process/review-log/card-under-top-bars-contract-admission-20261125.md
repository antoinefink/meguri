# Card under the top bars: contract admission (2026-11-25)

Scope: `docs/design/native-interface-contract.md` after merging master into the branch that adds N16.12 (the card scrolls on under the top bars on touch). The merge changed the file's bytes, so its whole-file entry in the public-artifact map (`tools/scripts/private-scan/private_scan.py`) needed a new complete review (`docs/process/provenance.md`, routine privacy admissions).

Reviewer: an independent Claude subagent, not the author of N16.12 or of the merge. Read-only; it read no `oracle/`, `refs/`, `local-data/`, `build/private/`, token list or Anki source, and did not look for the matched word.

Reviewed digest (SHA-256, whole file, 1571 lines): `5333a492abf1bdfb639e463774c66d0f1b5c8616fea47aeb7bc4e50ff84a147a`.

Decision: public-only, admit. No line judged private.

Noted as not decidable from the file alone, all in text already on master and unchanged by this branch: the example deck names in the Decks and Home strings, and the example counts in the Statistics and Home strings. They read as placeholders; earlier admissions of this file covered them.

Disposition: the map's entry for this path was set to the reviewed digest in the merge commit. No other entry changed.
