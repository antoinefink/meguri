# App owner causal trace — 2026-11-12

Role: fixer, following `roles/implementer.md`. Supplied upstream start:
`741f4211` (main clean). Filtered starting commit: `2d18bda` (clean).
Scope: passive DEBUG/test observations for TwoFetches, Mixed/flush and ResumeOwn.
No repair or acceptance claim. The authoritative prior run remains
`swift-test/20261112T102105Z-99936`: four failing groups, 22 issues; publisher
(two rows) passes, TwoFetches (five rows) fails expiry outcome 3, Mixed (six rows)
fails retiring triggers 0/1/2, ResumeOwn (two rows) fails priorAccount=true.
No prior evidence was rerun or researched here.

## Source trace plan

One shared, synchronous, bounded Service harness aggregate spans coordinator,
provider, expectation and cleanup observations, including successor backends.
It retains the first 512 ordered entries and reports the omitted count. A local
entry ordinal supplies order; fixed event names, source assertion sites,
booleans and local read/command/provider ordinals are the only values. No task,
account, collection, device, backend, guard or generation identifiers are emitted.
Failed expectations set the aggregate's emission flag. Helper cleanup emits the
same aggregate at entry and exit; ordinary case unwinding emits on observed
failure. If the original signal wait fails, its source site is recorded.
No new catch or test issue is added.

* The original expectation expressions are evaluated once, passed through an
  identity observer immediately before their original Testing macros. Optional
  requirements observe presence and return the original optional. Service
  command Issue.record and reopen's original predicate are observed too.
* Intentional expiry and close are recorded immediately before their original
  calls. Harness final close and cleanup process-cancel/owner-close boundaries
  have separate fixed event names. No ownership is inferred from backend close
  state or the input task's TaskLocal.
* DEBUG completeAccount and afterOutputs observations capture the exact read,
  closing/suspended/replacing, accountTask?.isCancelled as used by the unchanged
  guard, the actual completion admission boolean, backend/generation/revision
  equality and pending-read mismatch. Held admission produces no invented
  complete(true/false) observation; the existing drain observation still runs.
* DEBUG result metadata carries the original issued command sequence or exact
  account read to result reduction. The observer records CancellationError,
  the supplied shutdownCancelled value, and failureRevision before/after the
  unchanged reducer. Metadata never changes outputs, error, ACKs, admission,
  classification, execution or follow-up results. Unattributed synthetic/output
  reductions retain nil ordinals rather than claiming a command/read origin.
* Resume records the preceding read's final existing accountVerified/error
  facts, coordinator cancellation refusal, entry to the original native call,
  its normal successful return, or whether its original error is authorityChanged.
  The test separately records confirmResume's normal return (which alone is
  not success, since it can internally handle a failure). The real provider
  records each invocation's existing accountReadCount ordinal before its
  unchanged hold. There is no additional provider invocation or Core query.

Limit: the existing Resume result does not independently expose an adapter
retained-context guard change or which internal guard refused admission. Native
call entry is coordinator admission only; authorityChanged is the original
returned error, not proof of a particular retained guard mismatch. Successful
native return establishes native acceptance. No semantic API was introduced to
fill the missing guard facts. The isolated-preview contract question remains
unresolved; this pass chooses no production or fixture rule.

All original inputs, expectation predicates/outcomes, time limits, retries,
provider/native-return holds, commands, result ACKs and original errors remain.
The reducer only invokes synchronous observers; they perform bounded appends,
without suspension, waits, native calls or emission. No FFI, public API, Core,
protocol or production guard/classification change is included. Publisher test
rows and sticky-error tests are untouched.

## Review and checks

The single source consistency review passed `git diff --check` and inspected
trace data flow, identity-observer predicates and observer consumers. It found
one exhaustive observer switch in InitialCloudAdoptionTests; the new observation
cases now join its existing ignored cases, with all assertions unchanged. No
builds or runtime checks in this filtered pass. No forbidden sources, history, private inputs, network or
agents accessed. Integration must independently review, compile, and run the
same four retained groups once, including the unchanged publisher rows. Retain
all failures, original holds and native return gates. Provenance/privacy/
similarity checks and any required integration gates remain integrator work.
Do not interpret the trace pass as a fix or acceptance of the existing failures.

## Integration observations

- Independent source review `cleanroom/20261112T104229Z-36356`: no P0-P2 findings. Assertion predicates, optional requirements, timeouts and holds remain unchanged. The observer returns the original values.
- `swift-test/20261112T105039Z-56300` compiled and ran all four selected groups. Resume passed both rows after the separate Core correction; publisher passed both rows. The overall result remains fail, 21 issues in the other two groups.
- In all three mixed/flush close rows, the trace shows intentional close, then account completion with closing true, original account task canceled, admission false and matching backend/generation/revision. The generated cancellation result carries shutdownOwned false; failureRevision changes 0 to 1. This precedes the failed reopen expectation and helper cleanup.
- The two-fetch traces show distinct causes: the no-account and changed-account rows report a stale dependent RequestFetch and original cancellation; expiry precedes close in its own row; replacement keeps failureRevision unchanged but the harness still reports command errors. These are separate repair inputs, not passing tests.
- The trace does not prove a safe cancellation-classification repair. Original errors, outputs and ACKs remain intact. Similarity `20261112T105043Z-57453` passed. The privacy guard flagged two log sentences; they were rewritten and the focused log scan passed. No guard or exception changed.

Swift lint `lint-ui/20261112T105631Z-63156` passed. Final Rust formatting `core-format/20261112T105631Z-63267` passed without changes. No full-gate pass is claimed.
