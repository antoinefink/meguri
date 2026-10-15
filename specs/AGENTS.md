# specs/

- Written only in the spec-writer role (`docs/process/roles/spec-writer.md`); format and spec ids in `README-format.md`. Implementers read, never write. Exception: a "Meguri design doc" that a unit's Files field names for a Meguri-designed contract with no Anki behavior (`engine/op-model`, `change-stream`, `lifecycle`, `jobs`: U24) is written by that unit's implementer; its header says `Anki: none`.
- No Anki code, Anki-shaped pseudocode, non-interface Anki identifiers, Anki paths or line numbers here. Provenance goes to `oracle/provenance/`, test links to `oracle/harvest/links.csv`.
- Section ids are permanent: commit trailers and scenarios cite them.
- Every interface name a spec introduces goes into `<area>/interop-vocab.txt`; the similarity scan masks only what is listed there.
