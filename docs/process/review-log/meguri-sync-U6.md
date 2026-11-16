# Transactional incremental reconciliation

The independently authored Rust body is committed at f9680ac2. The integrated
server passes 162/162 tests and both lint configurations. All ten selected
current Desktop incremental groups pass against the exact committed executable.
Coherent-batch acceptance remains pending. Original batch base is aa8565719af435051de7d5f068c0c817286b568e.
Source preparation began at 5a1d747d448db2fcda921082a38469b2f40ec6d8.

## Body and boundaries

The implementation adds an account-scoped unpublished normal round, graves,
object/configuration exchange, row chunks, sanity and closing stamps. Sessions
bind collection and credential generation. Abort, replacement, expiry, revocation
and shutdown discard unpublished work; successful publication retains account
ownership through its committing barrier. A small storage-owner accessor keeps
root ownership alive through rollback. No core, app, Cargo graph or lock changed.

Known deck/type edits preserve supported opaque protobuf values through an
independent MIT adapter over the audited public model API. Measured current
conflict, tag, configuration, card-data and grave rules come from the neutral
Desktop specification and immutable original observations. Seed-only copies of
those two observation files are retained in author/review evidence but excluded
from canonical source; tests do not consume them. The test citation maps to the
canonical neutral JSON under conformance/fixtures/meguri-sync-server/observations.

Structural type-count changes, mixed optional configuration forms, non-null
incoming config when lnewer is false, default-deck deletion and outgoing stored
unknown card-data fields remain explicit unsupported/unobserved boundaries.
Incoming unknown card keys follow the measured normalization. No Unicode,
Mobile or whole-family closure is claimed.

## Evidence and review

Original start-route proofs failed 404 versus 200 before handler implementation.
The opaque-deck edit proof failed 400 before its preservation adapter. The seven
fact-fed matrix proofs failed after 16 existing session checks passed. The first
complete matrix implementation passed 154 tests before independent review.

One fresh read-only combined correctness and five-rubric simplification review
found two correctness defects: tuple snapshots captured before incoming graves
could resend deleted rows or stale relocated deck IDs; direct i32 decoding of
review intervals rejected supported SQLite REAL values. Three added real HTTP
and persisted-state regressions failed with 27 other checks passing. The fixes
capture outgoing tuples after graves and before object merge, and apply SQLite
integer conversion only for outgoing review values, preserving stored REALs.

A suggested prepare_cached optimization failed compilation because deliberately
minimal rusqlite features do not expose it. No test ran in that failed build.
The original prepare path was restored; dependency features and lock remain
unchanged. The unmeasured optimization is not required for acceptance.

Final source receipts in /private/tmp/meguri-sync-u6-20261115/build/mg:

- sync-server-test/20261116T004707Z-77043/result.json: 157/157.
- sync-server-lint/20261116T004710Z-77319/result.json: both configurations passed.
- sync-server-test/20261116T003930Z-66404/result.json:three observed pre-fix failures.

Combined review: /private/tmp/meguri-sync-u6-evidence/combined-review.md.
Targeted closure: /private/tmp/meguri-sync-u6-evidence/targeted-closure.md.
Both P1/P2 are closed; the optional optimization is an agreed skip. Reviewers
preserved the complete filtered source identities and ran no runtime tests.
Protected lifecycle, opaque-value and matrix proofs remain unchanged by fixes.

## Provenance

Sixteen original metadata-only session commits are retained in the exported
history; no source commit was reconstructed from failed exports. Actual sessions:

- s-20261115T224456Z-6541d701 (implementer, sync-u6-proof).
- s-20261115T225104Z-b0ac7c40 (implementer, sync-u6-incremental).
- s-20261115T231201Z-9f3bbce1 (fixer, sync-u6-compile).
- s-20261115T231448Z-e17d18d5 (fixer, sync-u6-snapshot).
- s-20261115T231706Z-44f004d2 (fixer, sync-u6-proof-permissions).
- s-20261115T232047Z-603e54aa (implementer, sync-u6-opaque-proof).
- s-20261115T232630Z-b31de29f (implementer, sync-u6-opaque-overlay).
- s-20261116T000435Z-74e2f6c6 (implementer, sync-u6-matrix-proof).
- s-20261116T001320Z-5daf0db5 (implementer, sync-u6-matrix-handlers).
- s-20261116T002632Z-6ec312fd (implementer, sync-u6-combined-review).
- s-20261116T003616Z-1788ffdc (implementer, sync-u6-review-proofs).
- s-20261116T004049Z-02086f5e (fixer, sync-u6-review-fixes).
- s-20261116T004524Z-e92c78f3 (fixer, sync-u6-cache-unavailable).
- s-20261116T004905Z-f15d7d11 (implementer, sync-u6-targeted-closure).

Initial reviewed source patch before the tuple refactor, SHA256:
bb0705a3e2c50401710bf1db7936f4592f6cff5ac1595a7a0c21868922a62001.
Initial 14-file manifest before that refactor:
dfec01f269128b4cff71fb5fcfb24a596faae2e4d47445ffab681b40c96a7ece.
The complete immutable input inventory is retained separately. These checks
record independent creation and runtime evidence; they do not certify exclusive
ownership or complete client compatibility.

## Canonical composition before final similarity repair

The web and incremental server together passed all 162 Rust tests at
build/mg/sync-server-test/20261116T011347Z-28961/result.json. Both lint
configurations passed at sync-server-lint/20261116T011433Z-29348.
The first final similarity check flagged two structural regions in chunks.rs
and the session-state observation decoder. It blocked acceptance before the
candidate run. That failed result remains at
build/mg/similarity/20261116T011014Z-28261/result.json.
A fresh filtered fix uses existing typed wire records and explicit observation
types, preserving every fixture and expected assertion. Targeted closure verified 18/9/11 positional wire shapes, checked numeric widths,
SQLite REAL conversion, tolerant kind mapping and identical expected state.
All 201 other assertion macros remain exact. No threshold or new similarity
exception changed. Additional actual sessions are s-20261116T011445Z-b24e99f5
(filtered fixer) and s-20261116T012348Z-649d45db (independent targeted reviewer).

## Final canonical and Desktop evidence

- Privacy: build/mg/private/20261116T013529Z-35693/result.json, no matches.
- Static: build/mg/static-checks/20261116T013530Z-36035/result.json, 3/3.
- Similarity: build/mg/similarity/20261116T013538Z-40838/result.json, all 15 supported files passed with no allowlist use.
- Rust: build/mg/sync-server-test/20261116T013543Z-41234/result.json, 162/162 and zero skips.
- Lint: build/mg/sync-server-lint/20261116T013625Z-41714/result.json, both configurations.

Final source patch SHA256:
0f830458f179b366ae2e1bc95b36b8709ece8f1cbd7f2932197aa271b5be7966.
Compact 14-file mapping SHA256:
8593015df6713467aa44b998069084d725e06b2722db3d134dad43c6d95c850f.
Final targeted receipt:
/private/tmp/meguri-sync-u6-evidence/similarity-targeted-closure.md.

One clean committed debug build at 4e34620f6973c04396b21bdf73fdbaae5b7e5d67
was reused for both actual Desktop selections:

- Build: build/mg/sync-server-candidate-build/20261116T014409Z-51245/result.json.
- Full transfer: build/mg/sync-server-interop/20261116T014426Z-51784/result.json, 11/11.
- Incremental: build/mg/sync-server-interop/20261116T014436Z-52593/result.json, 10/10.

Both client and semantic-validator exits are zero. Strict local retained-artifact
validation passes, compatibility credit is true and cleanup completes.
The ten incremental groups include the required reverse exchanges. Media,
Mobile, GUI, release packaging and whole-family acceptance remain separate.

Source archive SHA256:
8373d8a183a5b61bd357cd8a9ae31aa90c493511dcab0297dfdffb3643a2274b.
Cargo lock SHA256:
e6ff1a10840e84fac2a83621a1c4d4dbe21cf6915215ff59deb4d4532b3b8036.
Executable SHA256:
af8a2424d3fc3dbb5ce90461206451402f90cf9510998aa1e0480a9f98726ec9.
The build has no enabled Cargo features; it is not a release artifact.


## Accepted shared batch verification

The coherent collection, incremental, media and web batch is accepted at clean source `e54b80acb91675e3527a8445596682753f313612`.
Its original batch base remains `aa8565719af435051de7d5f068c0c817286b568e`.
Earlier pending statements above describe historical checkpoints.

The complete [gate receipt](/private/tmp/meguri-sync-collection-media-gate-20261116/build/mg/gate/20261116T173757Z-67818/receipt.json) is passing and merge-ready.
All 29 selected stages passed, with no blocking, incomplete, queued or quarantined checks.
The wrapper reports a warning for 30 untriggered jobs and the pre-existing disabled legacy Anki server harness.
That disabled entry predates this batch; no test was disabled to obtain this result.

The exact source passed 188 server tests with zero skips, nine frontend proofs and the actual browser journey.
One fresh compiled debug server passed all selected Desktop full-transfer (11), incremental (10) and media (4) groups.
Client and semantic-validator exits are zero; the results grant compatibility credit only to those finite selections.
Source, executable, runtime and retained-artifact identities remain in their nested receipts.
The native/Docker conformance, live-reference, dependency, provenance and similarity checks also passed.

The first-initialization setup-code output and matching web hint are integrated and tested.
Docker entrypoint/log delivery, final packaged-artifact validation, the retained Meguri peer and actual AnkiMobile checks remain later work.
Backup/restore and production deployment operations are explicitly deferred by the owner's 2026-11-16 scope decision.
No final application, Mobile, restore or production-deployment acceptance is claimed here.

Earlier failures and cancelled receipts remain unchanged.
The previously observed maintenance statistics mismatch did not recur in this full gate.
Its cause remains unproved; this passing result establishes current verification without claiming a repair.
