# U113 control records

Status: focused phase verified; U113 and U114 remain incomplete.
Phase start: `efcf26cabdcb42653e6adff2811249aff1d97340`.
Applied after: `4c0673b760134b4e8f0dc4ca77a0a000c03152f6`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Reuse the existing wire reader/writer for typed Epoch and MediaRoot values.
Keep Active and the old encoder entry points. Read format before the V1 size
limit; reject malformed V1 bytes and distinguish a newer format. Keys stay
opaque in Debug and errors. No store, SDK, account or publish policy is added.

Author: `cleanroom/20261028T183451Z-99710`.
R1 found a P2: the V1 size limit ran before the format read.
Repair `cleanroom/20261028T183958Z-2469` moves that limit after the read.
Independent R2 is clear of P0-P2. It also checks the old encoder bytes and the
independent literal vectors. The V1 maxima are 217 bytes for Epoch and 47 for
MediaRoot; 1,024 bytes bounds the V1 reader only.

`core-test/20261028T201240Z-26602`: all 122 selected CloudKit tests pass,
including 12 new codec tests. The explicit filter excludes 3,650 other tests.
`similarity/20261028T201552Z-28126`: pass.
The first privacy check flags one comment; Clippy requests a test-helper API
change. Those failed receipts remain in MG.

`cleanroom/20261028T201755Z-29278` removes the comment and uses as_chunks in
the hex helper, retaining its even-length check. Independent exact-delta review
is clear; guards, vectors and assertions are unchanged.

Final checks:

- `core-format/20261028T201949Z-30577`: pass.
- `core-test/20261028T202014Z-31075`: all 12 codec tests pass, zero skipped.
  The other 110 tests' inputs are unchanged by this final edit.
- `core-lint/20261028T202105Z-31585`: all four checks pass.
- `private/20261028T202106Z-31806`: pass.
- `similarity/20261028T202106Z-32023`: pass.

Publication, adoption, incoming changes, media, K19, P24, budgets, signed checks
and full integration evidence remain required. No Unit trailer is added.
