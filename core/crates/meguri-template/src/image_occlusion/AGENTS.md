# image_occlusion/

- `parse_raw` is the oracle-facing permissive reading; `parse` refuses unsafe edits. Both use `cloze::tokenizer`, whose source spans must include both markers.
- Keep `Document` and each shape's private source through edits. Equality ignores source spelling; `Document::write` compares the original reading and `write` checks each source separately (F11).
- F8 scales the exact binary significand with integer arithmetic. A floating-point multiplication by 10000 rounds twice and breaks the spec's 12.34565 example.
