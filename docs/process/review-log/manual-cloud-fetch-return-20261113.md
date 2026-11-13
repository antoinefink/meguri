# Manual cloud fetch completion at API return

Start: `1c6ed3d4`. Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Authority: `sync/policy#P3`, `#P12.3`, `sync/cloudkit-records#K12.5`, `#K17.4`,
`sync/cloudkit-recovery#V26.1` and Q11's in-force no-state test row.

The Service app test delivers didFetch, holds the original fetch API return,
then delivers a same-identity sign-in after deleting the root zone. The core
committed these receipts inside that callback. The reset arrived after that
commit, leaving the old receipts in storage. Clean baseline
`swift-test/20261113T105957Z-90738` failed the root-drain assertion and reported
an unnecessary bridge error at the later exchangeFetched call. It reached a
terminal result in 3.914 seconds; there was no timeout.

The repair retains an owned manual boundary until successful API return
processing, then checks zone facts against the current no-state point.
Scheduled callbacks keep their prior completion path. Failed native calls
consume the provisional boundary without committing completion, through a
separate abort export with its own registered instrumentation identity.
There is no schema change or fabricated SDK result.

After a fetch returns false, the exchange no longer calls the side-effect-free
exchangeFetched(false). That call only checked the SDK and could reject it
after the real Active read had correctly isolated the binding. Successful
fetches still clear the exchange phase immediately before sending. Original
terminal completion keeps its captured ownership and records isolation as
Failed. Real provider and transport errors keep their original paths.

The app test keeps its original hold, safety assertions and timing. Added
checks observe the original command's mapped runtime ID, require its retained
failed result and require unchanged SDK batch and empty-batch counts. The
rootless media helper checks unchanged completion before return and keeps its
old durable drain/absence assertions after return. Its earlier batch failure
still prevents runtime coverage of that later helper; no pass is claimed.

## Review and repair

- Author `cleanroom/20261113T110110Z-92364`: manual return/abort boundary and
  Rust regressions. Independent R1 `20261113T110815Z-7118`: no P0/P1, two P2s
  for instrumentation identity and the rootless helper's completion timing.
- R1 app test `swift-test/20261113T111042Z-25442`: every existing safety
  assertion passed, but the stale exchangeFetched call remained a failure.
- Fixer R2 `cleanroom/20261113T111316Z-39244`: both P2s and the stale call.
  Independent R2 `20261113T111700Z-72971`: no P0/P1/P2.
- `swift-test/20261113T112040Z-85322` did not compile: the test observation's
  await inside the require macro crossed actors with an inferred closure.
  No tests ran. Fixer R3 `cleanroom/20261113T112210Z-87754` moved the lane call
  outside the macro and made the closure Sendable. All other patch sections,
  including every production section, are byte-identical to reviewed R2.
  Independent delta review `20261113T112420Z-92582`: no P0/P1/P2.

All review was read-only in filtered implementer-role runtimes. The main
checkout applied the exported patches against the recorded start and owns
verification. No optional source cleanup was added.

## Focused evidence and retained failures

- `core-test/20261113T110828Z-8060`: all six selected Rust return/no-state,
  scheduled/joined and root-refusal regressions passed.
- `core-test/20261113T111712Z-73972`: 101/103 passed in the affected Host
  absence, policy, media-ACK and FFI selection. Both failures are the rootless
  media tests' initial batch size, before the changed completion helper.
  Clean-start comparison `20261113T112218Z-88211` reproduced both at the same
  assertion, zero saves versus one. That comparison ran four tests: two passed,
  two failed. Neither failure is waived or counted as passing.
- `swift-test/20261113T112436Z-93081`: all three selected app methods passed,
  including both cases of the no-state/Copy Debug Info method. The Q11 method
  proves isolation, no root drain, unchanged entities, no absence completion,
  accepted zone-not-found read, actual failed exchange and no new batch.
- Broader native selection `swift-test/20261113T112547Z-95364` remains red:
  three methods report seven drain-count issues. Clean-start comparison
  `20261113T112730Z-15812` reproduces the exact same seven issue records.
  The instrumentation method passed in both selections, but neither overall
  selection passed. Other selected native methods passed. No baseline claim
  extends beyond these named comparisons.
- R2 source privacy `private/20261113T111749Z-74986`: all twelve source paths
  scanned. Similarity `20261113T111748Z-74591`: twelve passed, zero exemptions.
  Static `20261113T111749Z-74806`: 3/3 passed. R3 changes only the one Swift
  test section; final delta and staged-document checks are recorded below.

## Final checks

`core-lint/20261113T112953Z-22349` passed 3/3 correctness and layering checks.
`static-checks/20261113T113101Z-26469` passed 3/3. Standalone registry check
`swift-test/20261113T113005Z-22714` passed one test, zero skips. Final Swift
source delta similarity `20261113T113103Z-26820` passed, zero exemptions.
The twelve-file R2 receipt still covers every other unchanged source section.

The first staged privacy scan `20261113T113104Z-27094` rejected one prose
sentence in this log. Its wording was replaced; no source or guard changed.
Direct document scan `20261113T113136Z-28052` passed. The final index scan
receipt is kept in local build evidence and the checkpoint commit metadata.
These focused checks do not replace the full pending batch gate.

## Ordinary VM peer diagnostic

`drive/20261113T105726Z-86945` built the signed Debug app without compiler
errors or warnings. The VM refused launch before the driver captured the app:
RunningBoard error 5 and launchd spawn failure. No drive step or sync check
completed. A private profile comparison supports a device-coverage lead;
no profile, account or capability was changed. This fallback stops there.
`doctor/20261113T105825Z-88463` reports the VM running and reachable, matching
host Xcode, the pinned simulator runtime present and enough process capacity.

This is a bounded repair checkpoint, not Unit or P24 closure. Other recorded
failures, the full pending integration gate, live peer receipt, media proof,
milestone gates and final acceptance remain open. No release is accepted.
