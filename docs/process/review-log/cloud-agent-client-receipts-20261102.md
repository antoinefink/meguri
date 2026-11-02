# Signed-agent client receipt binding

Status: component verified; runtime integration remains open.
Start: `d7bc140c`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.1`, `conformance/cloudkit-acceptance#Q13.1`.

The client reader retains the four fixed receipt and manifest copies with their
original bytes and filesystem authority. It checks MG envelopes, strict numeric
types, declarations, digests, source and device bindings, and inventory models.
Host paths in JSON remain metadata. They are never opened as client paths.
Evidence JSON has a separate 16 MiB bound; existing header callers retain 1 MiB.
The API lists the observations it cannot establish from copied receipts alone.

Independent reviewer: `native_bounds_review`.
Round 1 found a P1 compilation failure in pure accessor isolation and a P2
argument parser that accepted unsupported MG options. Both were repaired.
The next native run compiled but failed seven new tests before parsing because
a Foundation path comparison rejected the canonical temporary root.
The repair follows the existing descriptor-based reader: raw lexical validation
and all no-follow, ownership, mode, identity and event checks remain.
The positive probe confirmed that Foundation changed the canonical path.
Schema refusal tests now require schema errors; storage failures cannot satisfy
them. Round 3 has no required findings, and the earlier P2 is resolved.

Filtered source: `cleanroom/20261102T051202Z-12857`, with a test-path-only
relocation; repairs `cleanroom/20261102T052316Z-18608` and
`cleanroom/20261102T053117Z-21990`. Test source stays outside the shared
app-hosted support directory.

Verification:

- `swift-test/20261102T053350Z-22531`: 64 passed, zero skipped, covering all
  48 existing agent tests and 16 new receipt tests.
- `build-app/20261102T053436Z-24503`: Mac CloudAgent compiled, zero errors or warnings.
- `build-app/20261102T053521Z-25566`: iOS simulator CloudAgent compiled, zero errors or warnings.
- `private/20261102T053418Z-23952`: all three source files passed.
- `similarity/20261102T053420Z-24180`: all three Swift files scanned and passed.
- `static-checks/20261102T053522Z-25976`: all three checks passed.

The failed compilation `swift-test/20261102T052109Z-16866` and the 56/63 result
`swift-test/20261102T052752Z-19668` remain retained. Privacy findings in the first
generated fixture were resolved by rewriting its own public text; the guard
was unchanged.

The host job registration also passed `milestone-gate/20261102T052502Z-19041`
with `--check`; this is manifest consistency only.
Actual loaded products, signatures, VM routing, delivery, installation, OS and
whole-lane observations remain separate obligations. Existing runtime refusal
guards are unchanged. Builds are compile-only; no VM run, Unit trailer, full
gate or signed acceptance is claimed. The original batch boundary remains open.
