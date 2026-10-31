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
