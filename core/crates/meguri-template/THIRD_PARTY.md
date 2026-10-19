U64 Unicode data dependencies (crates.io releases; no Anki code or data):

- `unic-ucd-category =0.9.0`: Unicode 10.0 general categories, MIT OR Apache-2.0. Used only for the leading-mark NBSP in A6.5; its table is intentionally independent of normalization.
- `unicode-normalization =0.1.24`: Unicode 16.0 normalization and mark categories, MIT OR Apache-2.0. Existing workspace parity pin.
- `icu_casemap =2.0.0` and compiled `icu_casemap_data =2.0.0`: ICU4X Unicode 16.0 simple case folding, Unicode-3.0. Only A9's one-character folding is used. The data release is pinned separately to prevent Unicode drift.

The existing `difflib =0.4.0` (MIT) and `htmlescape =0.3.1` (MIT OR Apache-2.0 OR MPL-2.0) pins remain authoritative for opcodes and escaping. Host dependency resolution must retain these exact releases in Cargo.lock.
