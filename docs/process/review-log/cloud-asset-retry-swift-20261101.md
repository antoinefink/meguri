# Swift asset read recovery — source handoff

Role: implementer. Session: `s-20261025T134357Z-235af9a2`.
Supplied starting master: `5875adc7` (executable clean base `015807f3`).
Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Scope: P24 K13.1/P4, Q11, K22.2; tests first, no behavior fix.

The test retains one Service while AppModel owners A and B run in sequence. The public Note service writes both edits. A keeps automatic sync disabled so its local edit stays pending before B uploads the remote change.

Each SDK fetch delivers real Service pages through its registered delegate. The coordinator receives the resulting events and runs the retry. The final upload uses Sync Now. No direct retry or issued-command call drives this test.

The opt-in wire seam withholds one matching payload delivery after B has closed and returns five failed asset deliveries only after actual accepted Read frames have drained. It verifies the selected server envelope/system fields/payload before injecting failure. Actual Read receipts, real drain records, production backend retry command starts/completions and the existing read-only FFI inbox observer supply the assertions. Failure counts 1–5, a sixth drain with no read, same-files reopen count 0, and the first post-open drain's successful same-version real lease/apply are required. Service Saved body, conditional base, ACK digests/system fields, subsequent reopen and unrelated Note/media preservation are asserted.

Authorship is UNRUN: no build, test, Git, history, network or forbidden-source access in this filtered session. Root owns regenerated bindings, Rust-first verification, focused native compilation/run, independent review and all batch/provenance/privacy/similarity gates. No P24 closure, Unit/release or Apple SDK claim.

Root must compile and execute this source. The existing ten-second command barrier remains in place, and absent dispatch must fail. The shared support behavior changes only when the new fault is armed. Production code and FFI exports are untouched.

The single source check found balanced Swift delimiters/conditional guards, no trailing whitespace or conflict markers in the four owned paths. Source review then tightened terminal close assertions and records unexpected accepted reads before fault validation, so a late sixth request cannot disappear from the ledger. No compilation or execution evidence is inferred from this check.

## Independent review, round 1

Reviewer `native_bounds_review` found one P2: the large Note payload exceeds
the synthetic stdio bridge's 1 MiB JSON frame cap. Its integer-array encoding
needs several MiB. Keep that cap and the production asset threshold; provide
a bounded fixture transport before executing this test. No other P0–P2 was
found in the source review. Native compilation and execution remain UNRUN.

## P2 fixture transport fix — source-only handoff

Role: fixer under implementer clean-room rules. Supplied starting master
`d5ff8fa0`; original U113 start and batch base above remain authoritative.
This filtered session used no Git/history, network, forbidden trees, build or
test execution. No production sync, Service/model, spec, threshold, guard,
retry, timeout or workflow assertion changes. The workflow file changes only
its process open to opt into `.binaryAssetsV1`.

The CLI explicitly opts in with `--binary-assets-v1`; Swift
`CloudServiceProcess.open(stateDirectory:transport:)` defaults to `.legacy`.
Legacy JSON+LF framing and its existing negative/cap tests remain. Binary mode
uses `{version:1,lengths:[...],message:...}` JSON (at most 1048576 bytes before
LF), followed immediately by the advertised raw bodies in index order. Only
schema-defined request/response Record Asset.bytes leaves become
`{attachment:index}`. Service.request runs once per valid original request;
response extraction uses only its actual return, including Saved, Read,
FetchZone and conflict server records. There is no compression/base64.

Both implementations bound bodies to 4194304 bytes each, at most 16 bodies,
and 8388608 bytes total: sufficient for remote and merged ~1 MiB payloads and
bounded fixture pages. Header lengths, all indices, uniqueness and complete
body use are validated before allocating/reading bodies. Rust also validates
the decoded request skeleton and supported operation before reading bodies.
Malformed/limit/truncated failures use fixed classifications without content
or paths. Every prepared Swift request owns its complete header and Data
bodies before admission. The same serial queue writes and drains the complete
original reply before releasing the next exchange, including accepted
cancellation; it never replays an exchange. Existing terminal/error/diagnostic
join guards remain.

`ServiceOriginalReply.originalRequestJSON` / `originalResponseJSON` continue
to mean the original *logical* Service values, normalized after binary body
reconstruction. They are not raw pipe evidence. The actual completed exchange
ledger, `transportExchanges()`, separately owns header/Data-body packets;
headers exclude their LF delimiter. The focused Swift test checks real
ledger request IDs, exact binary bodies and queue alignment after cancellation.

All authored tests are UNRUN. Integrator must run Rust first, through MG,
including the complete `meguri-cli` integration target `cloud_service` (all
existing legacy tests), with these new exact test names:

- `binary_assets_v1_large_arbitrary_pa_conditional_save_read_fetch_and_request_ids`
- `binary_assets_v1_refuses_invalid_headers_limits_and_truncation_before_dispatch`

Then compile and run native MeguriKit AppModelTests through MG, retaining the
CLI fixture environment, using these exact Swift Testing filters:

- `CloudServiceProcessTests` (all existing tests plus
  `binaryAssetsPreparedCancellationDrainsBodiesAndConcurrentFramesStayAligned`
  and `binaryAssetsPreparationRejectsBoundsBeforeAcceptanceOrIO`)
- `ReviewCollectionStartupTests/realServiceAssetInboxAutomaticallyRetriesOncePerDrainAndResetsOnReopen`
- `ReviewCollectionStartupTests/realServiceReviewAndRegisteredMediaSurviveReopenAndFreshAdoption`
- `CloudServiceInventoryTests`
- `ReviewMediaBindingTests`

The asset-retry and review/media methods live in their dedicated source files
but extend `ReviewCollectionStartupTests`; do not filter by the source filename
as if it were a suite. Preserve the full retry workflow and existing deadlines.
Root owns formatting/compilation, focused verification, independent review,
separate commit attribution, provenance/privacy/similarity checks and the full
batch gate at the original batch base. No unit/milestone closure is claimed.

Single source consistency checker run: lexical delimiter/Swift conditional
balance, final newlines, whitespace/conflict markers across all ten owned
paths, matching transport limits, legacy default/versioned flag, the single
actual dispatch site and retry opt-in presence passed their assertions. The
checker then stopped on an incorrect checker assumption that the original
workflow contains a literal `960_000` or `960000`. Direct source inspection
confirmed its unchanged original expression is
`String(repeating: "remote", count: 160_000)` (960000 UTF-8 bytes). This is a
checker assertion failure, not compilation or execution evidence. No second
check was run. All builds/tests and independent review remain UNRUN. Export
ready; no root report or unrelated edits authored.

## Independent review, round 2 — dependency and privacy source fix

Role: fixer under implementer clean-room rules. Supplied current master
`f447889f`; the runner had already applied the own-code seed authored at
`d5ff8fa0`. Original U113 start
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a` and batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` are unchanged.

Reviewer `native_bounds_review` identified only P2: the binary fixture module
imports serde and derives Serialize/Deserialize without a direct CLI dependency.
Added `serde.workspace = true`, inheriting the workspace's existing derive
feature, and added only the matching CLI dependency edge to Cargo.lock. No
package versions, checksums or workspace features changed. This addresses the
reported source defect; Rust and native compilation/execution remain UNRUN.

Rewrote the flagged Swift input setup and comments. Assertions and fixture
bytes stay the same. No scanner rule changed.

No protocol/parser redesign or changes to actual Service dispatch, typed
leaves, hashing, body/header limits, pre-IO validation, legacy default,
semantic/raw ledgers, cancellation, joins, workflows, thresholds or timeouts.
No builds, tests, network, history or forbidden-source access in this runner.
Root retains the Rust CLI protocol tests first, then the Swift workflow and
support checks listed above, independent review and all provenance/privacy/
similarity and batch gates. This is a source fix only, with no gate evidence
or unit/milestone closure claimed.

The single source consistency check passed across all 12 owned paths. Six
files differ from the applied seed: the CLI manifest and lockfile, the Swift
process test, the two binary-helper comment regions and this log. The check
confirmed inherited serde derive, only the CLI lockfile dependency edge,
comment-only changes in binary helpers, unchanged surrounding tests and
assertions, retained fixture values, rewritten-region delimiter balance,
final newlines and absence of trailing whitespace/conflict markers. No second
check was run. Source export is ready; all execution and scan evidence remains
UNRUN.

## First protocol execution

`core-test/20261101T134456Z-90223`: 7/8 passed. The positive binary case
failed at its first response header. Independent review confirmed that the
test helper's mutable JSON indexing inserts a Modify branch into CreateZone.
Use checked mutable lookups for existing Modify and Save fields. Keep all
positive assertions and the actual Service dispatch. Swift remains UNRUN.
