# U113 incoming engine prerequisites

Status: enabled tests-first contracts; implementation and execution pending.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `ac4b0eca786180bcfe29bb0765fe0e515d469f7d`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Source author `cleanroom/20261029T120616Z-34760` adds 20 deterministic
engine tests for CloudKit sources, stamps, cancellation, selective upload flags,
acknowledgements, targeted undo/redo cuts and retained journal images.
Independent R1 clears those cases and requests the M9.10 mixed-history property.
Fixer `cleanroom/20261029T122008Z-41264` adds 16 deterministic seeds with
1,376 real engine actions and an independent history model. Independent R2
finds no P0-P2 issues in the tests or model. Existing executable assertions
are preserved; custom rekey names and comments are changed mechanically.

- `core-format/20261029T122831Z-44469`: formatting run; see its receipt.
- `core-test/20261029T122846Z-44805`: expected compile RED on the missing
  CloudKit sources, hold/ACK APIs and detached-run result. Zero tests executed;
  this is not evidence that the runtime invariants pass.
- `private/20261029T122855Z-45192`: flags one new comment. Only that comment
  is deleted; independent R2 confirms equivalent executable tokens.
- Final privacy and similarity receipts are recorded below before commit.

The normal manual-transfer source remains separate and unchanged. The future
FFI cases must append tags 8 and 9 while retaining the existing tags 1 to 7.
Actual inbox application, durable intents, WAL/restart handling, media, native
integration and signed exchanges still remain. No Unit or P24 closure, full
integration gate, or working-sync claim is made by this tests-first checkpoint.

Privacy: `private/20261029T122923Z-45792`: pass.

Similarity: `similarity/20261029T122855Z-45126`: pass.
