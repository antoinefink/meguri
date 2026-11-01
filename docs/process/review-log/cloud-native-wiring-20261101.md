# Native conflict delivery and asset executor

This work starts at `662ba6efebc855fcd3e13488e7617def957972f7`.
The integration batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Native conditional errors now retain the actual server record through Swift
and FFI. Collection metadata is checked before handler entry; core admission
checks original SDK, file, batch and retained-flight headers without loading
payload bodies. The core returns its actual Saved count, including historical
own saves. Swift uses that count. Media failures keep their original behavior.

The asset executor drains one opaque pass serially. Queue admission stays with
the original native operation, even when accounting fails after enqueue.
Cancellation joins the original terminal and completes the original core work.
Soft read failures retain small SDK facts and do not prevent later entries from
running. The completed pass reports failure explicitly. No policy classes or
acknowledgements are computed in Swift.

Independent metadata review found a full-image preflight and changed media
fallback. Both are repaired. A test supplied a built media request as Saved;
it now uploads through Service and uses the actual reply. The live Note is
ACKed before its dependent Card. Independent executor review found that soft
failures looked successful; the pass now returns an explicit result. A
test-only observer reads the real ownership map: one request before its
terminal, zero after completion, while the accounting fence remains.

Source exports and repairs:

- `cleanroom/20261031T224637Z-9731`: native conflict delivery.
- `cleanroom/20261031T225940Z-13732`: bounded preflight, media fallback and count.
- `cleanroom/20261031T230916Z-16725`: actual media receipt and legal baseline.
- `cleanroom/20261031T224920Z-10659`: finite asset executor and fixture hooks.
- `cleanroom/20261031T230358Z-14843`: failed-round and native error facts.
- `cleanroom/20261031T231605Z-20597`: owned request count check, compiler
  assertion repair and comment cleanup.

Verification:

- `core-test/20261031T231754Z-22650`: seven actual Service/FFI cases pass.
- `core-test/20261031T232013Z-24162`: all 191 FFI library tests pass.
- `core-test/20261031T232421Z-25098`: all 55 incoming/reply tests pass.
- `swift-test/20261031T231751Z-21780`: 86 of 87 pass, with zero Swift compiler
  warnings. All executor cases, including the owned request count, pass.
  The partial-error longest-delay case remains failed: it constructs different
  per-record and aggregate errors for the same record. SDK-delivered facts need
  characterization before changing the fixture or its strict delay assertion.
- `core-lint/20261031T232429Z-25712`: five cloned-reference slice warnings in
  the new conflict tests remain to fix. Production layering passes.
- `private/20261031T231750Z-21520`: all changed source passes.
- `similarity/20261031T232424Z-25349`: all changed source passes.

Eight retained flights use about 1 MiB each; actual FFI intake reads no payload
bodies in this test. It does not establish a new
full-scale peak measurement; the existing eight by 40 MiB stress test remains
enabled and unchanged.

This is a WIP checkpoint. Fix the remaining checks, then wire the retained
per-SDK executor, exact retry commands and original-pass activity reporting
after the true native event-bookkeeping boundary. Signed cross-device
collection/review/media proof, other open sync requirements and the batch gate
remain required. U113 and U114 remain unaccepted.

Follow-up diagnostics:

- `cleanroom/20261031T232834Z-26813` replaces the five cloned-reference slices
  and adds SDK callback characterization plus direct helper checks.
- `core-lint/20261031T234254Z-30990`: all four checks pass.
- `swift-test/20261031T233608Z-28746`: the direct helper preserves the exact
  180000 and 240000 millisecond maxima. The SDK diagnostic fails because its
  terminal failure capture is absent. This does not establish whether the
  callback delivered success or did not run. Capture its invocation and branch
  before correcting the fixture. Production arithmetic remains unchanged.
- Independent read-only review agrees that the next diagnostic must observe
  both callback branches. Keep the original strict assertions and the direct
  nested-error coverage.

Native integration, source base `dc7be3f7`:

- `cleanroom/20261031T233356Z-27824` connects original-SDK asset work to the
  private database, event bookkeeping, issued retry commands and lifecycle
  drains. Original-pass activity shares the Rust error classifier.
- `core-test/20261031T235355Z-34479`: all 456 selected tests pass.
- `swift-test/20261031T235352Z-33601`: 58 of 59 pass, with no compiler warnings.
  The first conflict download merges and queues its resend; accepted and
  blocked lifecycle drains pass. The queued-retry case ends before its expected
  transport arrival and remains failed.
- Independent review R1 finds P1: the exchange can advance before its first
  asset pass finishes. Its new test manually waits and sends again, so does not
  prove that Sync Now performs the merge and resend within the same exchange.
  Repair must join outside SDK event delivery and cover the actual exchange.
- `private/20261031T235511Z-36233` flags three new comment spans in two files;
  replace those comments without changing the privacy guard.
- `doctor/20261031T235417Z-34932`: 11 pass, three warnings, no failures. Signing
  and final device evidence remain pending. The dedicated VM is available.

The R1 fixer uses the exact preserved source seed from this base. The original
integration batch base and U113/U114 acceptance status remain unchanged.

R1 repair and R2 review:

- `cleanroom/20261031T235816Z-37321` joins asset work before send completion
  and adds an actual Sync Now exchange test. The queued-retry test now waits
  for each terminal within its owning execution; all three commands remain.
- `swift-test/20261101T000327Z-38752`: all eight focused tests pass. Sync Now
  stays unfinished while the asset terminal is held, then merges and resends
  within that exchange and records the actual acknowledgement.
- Installed SDK callbacks report code 11 at 120000 milliseconds, then terminal
  success. The native converter receives no aggregate error in this case.
  Direct recursive-error checks retain the 180000 and 240000 millisecond maxima.
- `core-lint/20261101T000356Z-40370` and
  `similarity/20261101T000352Z-39904` pass.
- `private/20261101T000356Z-40175` still flags two rewritten comment lines;
  remove the two comments. The other files pass.
- Independent R2 review closes P1 but finds P2: cancelling the exchange does
  not cancel its independent asset operation. Repair must cancel the original
  prepared read, retain its completion drain and preserve unrelated retries.
  Add a held-terminal cancellation case through the actual exchange.

R2 repair and R3 review:

- `cleanroom/20261101T000534Z-41128` captures first-read request identities
  for the send join. Cancellation stops only that captured work and joins its
  original completion. Queued retry commands retain their separate owners.
- The actual exchange cancellation test holds the original terminal, cancels
  through the issued policy command and requires prepared-read cancellation.
  Only after terminal release may the exchange record cancellation; a late
  successful read cannot produce a resend or successful exchange result.
- The installed-callback fixture uses the same error for its per-record and
  partial aggregate facts. Exact delay assertions and direct recursive-error
  coverage remain intact. Production delay conversion is unchanged.
- R3 review closes the known P2 and finds no new P0 or P1. Separate tests for
  already-cancelled join entry and a queued first read behind an unrelated
  retry remain absent; review found no source defect for those cases.
- `private/20261101T001059Z-44121` and
  `similarity/20261101T001103Z-44389` pass after the repair.
- `swift-test/20261101T001042Z-42933`: all 75 affected native tests pass,
  with no skips or compiler warnings. Both actual exchange tests, exact retry
  ownership, lifecycle drains and all four installed delay cases pass.
- Rust sources did not change in R1 or R2; their 456-test and lint receipts
  above still cover those inputs.
- `private/20261101T001311Z-45057` passes for the updated review log.

These results close the findings in this wiring slice. They do not establish
signed multi-device behavior, complete U113 or U114, or close the integration
batch and milestone gates.

App-package check:

- `swift-test/20261101T001322Z-45322` fails compilation before tests run:
  `InitialCloudAdoptionTests` has an own-transport fake whose switch lacks the
  new asset-read case. Update that fake, then run the selected app tests.
- `cleanroom/20261101T001544Z-47534` adds an explicit unsupported-command
  rejection to that fake. Independent review finds no required fixes.
- `swift-test/20261101T001622Z-48054`: all 24 selected app tests pass, with
  no skips or compiler warnings. The package compiles the native backend;
  coordinator, adoption, presentation and local review lifecycle checks pass.
- `private/20261101T001704Z-50047` and
  `similarity/20261101T001702Z-49780` pass for this test repair.

Final component checks at `5923a52d`:

- `core-test/20261101T001752Z-50564`: all 23 selected asset-pass, activity and
  conflict tests pass with Apple builds idle.
- `build-app/20261101T002014Z-51131`: the standalone Debug macOS app builds
  and passes its artifact checks. The build does not launch the app. It uses
  ad hoc signing and does not prove signed-account iCloud behavior.

Master contains this component and its focused evidence. The original batch
base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Its gate remains open;
retain the earlier failure evidence and repair the pending technical checks.
U113 and U114 have no acceptance trailers from this work.
