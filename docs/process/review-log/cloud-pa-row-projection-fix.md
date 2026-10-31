# Ordinary pa row projection fix

- Role: fixer; source base `d9df5e6ce59a6c60de93eda65f1dc208d745acf7`;
  original integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Reported actual FFI probes stop at `capture.projection invalid_data` and
  `incoming.apply invalid_data`. Source confirms `project()` rejected canonical
  payloads above the inline envelope threshold. It now retains codec validation
  without treating envelope placement as row validity. Codec errors still map
  to `InvalidData`; the canonical 100 MiB cap and wire codec are unchanged.
- The module's only other inline use was `notetype()`'s estimated part-size cap.
  Replaced it with existing canonical counting validation after each assembled
  page and at completion, including empty layouts. No encoded copy or estimated
  lower cap is introduced. Generation, part identity/order and acknowledged-layout
  checks remain. First-adoption assets and structural generation/ma/K19 workflows
  remain outside this fix; no structural gate or tracking rule is relaxed.
- Only `row_payload.rs` and this note are owned here. Seed pa R2, accessor repairs,
  feature-gated diagnostics, existing tests and concurrent builder/storage work
  are preserved. No user content is recorded.
- One scoped source/diff consistency check; no build, test, formatter, network,
  commit or gate execution. Root must rerun the unchanged four FFI payload cases,
  three lifecycle cases and `live_new_pa_card...` same-capture regression, then
  complete independent review and provenance/privacy/similarity checks. No unit
  or acceptance claim.
