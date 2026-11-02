# Canonical small CloudKit fixture

Status: focused checks and real generation pass; signed delivery remains open.
Start: `a6236e0a`; captured source: `49b2b97e`.
Integration onto: `1b9cb66a`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-acceptance#Q4.2`, `conformance/cloudkit-agent#J2.3`.

The dedicated Rust generator produces the specified small base through existing
collection, media, history and atomic publication machinery. Its real CLI and
MG request retain the request, manifest and observed output. The base has
20 QA notes/cards, two marked decks retaining deck id 1, the standard preset,
five referenced 10240-byte media files, and 60 computed learning history rows
across the three specified days. Other generators retain their 2030 clock.

Independent reviewer: `native_bounds_review`.
The spec clarification in `a6236e0a` was reviewed before implementation.
Round 1 found a P2: same-size cached media corruption escaped readback.
Generation and readback now use the same bounded file construction from the
request-bound seed, and readback checks actual bytes. A CLI regression changes
one byte while preserving length and requires failure with no successful
receipt. It preserves the damaged file and original manifest. Final review
has no findings.

Prepared source: `cleanroom/20261102T055902Z-38519`.
The formatted seed was resumed by `cleanroom/20261102T062102Z-49151`.
Verification:

- `core-test/20261102T062939Z-52231`: 13 focused generator and CLI tests passed,
  zero skipped, including history/media readback, corruption, deterministic
  output, normal CLI behavior and output locking.
- `selftest/20261102T061142Z-43237`: five wrapper checks passed, zero skipped.
  Their inputs were unchanged by the media-byte repair.
- `core-lint/20261102T062942Z-52505`: formatting, workspace lint and layering passed.
- `fixtures/20261102T063046Z-53088`: the real MG request generated 20 notes/cards,
  two decks, one note type/preset, five media files and references, 51200 media
  bytes, and 60 history rows. It declares the fixture, generation and manifest
  artifacts. This is a development receipt from the working tree.
- `private/20261102T062820Z-51506`: all seven source files passed.
- `similarity/20261102T063050Z-53340`: all five Rust files scanned and passed.

The initial privacy result `private/20261102T061138Z-42718` remains retained;
the fix rewrote its own comment and did not change the guard.
The original 12/12 Rust and 5/5 wrapper results are retained too.
The generator establishes real source data, not app installation, SDK behavior,
convergence or a signed lane. Signed execution still requires a clean matching
producer receipt, frozen input/delivery and runtime checks. No Unit trailer,
full gate or release acceptance is claimed.
