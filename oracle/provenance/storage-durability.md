# Storage durability and files (harvest rows)

Spec-writer notes for `specs/storage/durability-and-files.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`) in session s-20261117T152613Z-96466c16 (spec writer) to classify a deferred harvest row; no code was copied.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| Classification of the relative-destination atomic rename row (proposed storage/durability-and-files F4.1) | `rslib/io/src/lib.rs` | That publishing to a one-component relative path must still sync its parent directory. Used only to state Meguri's own destination-directory rule. |
