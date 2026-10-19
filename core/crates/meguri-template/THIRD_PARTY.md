U64 Unicode data (official Unicode data and crates.io releases; no Anki code or data):

- `src/type_answer/unicode10_marks.rs`: Unicode 10.0 Mn/Mc/Me intervals, used only for the leading-mark NBSP in A6.5, independently of normalization. Derived from the neutral [reference and provenance](../../../specs/render/unicode10-marks.md) and [JSON](../../../specs/render/unicode10-marks.json) (SHA-256 `53455c47f2d7be861bc4c5f65f8253608462db98276de23e824802e98ad5e72d`). Official source: <https://www.unicode.org/Public/10.0.0/ucd/UnicodeData.txt> (SHA-256 `52423e4d7492167b62f518f68d54db88930abbbff7f11edfcaec8f726498cab1`). Unicode-3.0; retain the [complete official notice](../../../specs/render/unicode-data-LICENSE.txt) with derived tables and application third-party documentation.
- `unicode-normalization =0.1.24`: Unicode 16.0 normalization and mark categories, MIT OR Apache-2.0. Existing workspace parity pin.
- `icu_casemap =2.0.0` and compiled `icu_casemap_data =2.0.0`: ICU4X Unicode 16.0 simple case folding, Unicode-3.0. Only A9's one-character folding is used. The data release is pinned separately to prevent Unicode drift.

The existing `difflib =0.4.0` (MIT) and `htmlescape =0.3.1` (MIT OR Apache-2.0 OR MPL-2.0) pins remain authoritative for opcodes and escaping. Host dependency resolution must retain these exact releases in Cargo.lock.
