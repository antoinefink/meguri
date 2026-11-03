# U113 internal policy jobs

Status: WIP; no Unit acceptance.
Original unit start:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start:512e902dc5095db69f3250bf2ba6c40636291021.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer:s-20261103T131440Z-8df2fdb2.
Fixer:s-20261103T133750Z-f276f58e.

Approved engine/jobs J2.1.1 and sync/policy P2.1 require short lane calls.
Eleven asynchronous policy facade calls now start registered, no-slot jobs on
the original Host's pool. A shared async permit preserves order without worker
waits or a retained task chain. Typed cells retain exact errors and required
failure follow-ups before generic job failure handling. Subscription precedes
start; listener failure explicitly cancels and drains admitted work.

Cancellation follows the first actual change through existing Driver, Runtime,
writer rollover and media paths. Queued or pre-change cancellation leaves state
unchanged; after a change, the original result and follow-ups survive. Existing
synchronous callers retain their behavior. Native Swift ownership is unchanged.

## Independent review

R1 found cancellation could pass its check, wait for Driver ownership, then
change state despite cancellation. Export6229 added explicit controlled paths
and positive contention tests. R2 confirmed that repair and found four issues:
an unguarded offer invalidation; idle polling latching on None-to-None; journal
admission before a read-only probe; and an import preceding module documentation.

Export16899 fixes all four issues. R3 has no P0-P2 findings. The layer changes
follow J2.1.1 because these calls already change state before publication.
Export26971 then rewrites three comments and one local test name. A separate
review finds no change in calls, tests, order, limits or guards.

An additional integration review confirms internal JobFinished events cannot
create a policy-refresh loop, disqualify quiet evidence or move the committed-op
WAL target. Event consumers filter them appropriately.

Automatic approval refused the first apply while review was pending. Master
stayed unchanged. R3 and the32-file privacy scan passed before the next apply;
that apply was approved. No guard or exception was changed.

## Verification

| Receipt | Result |
| --- | --- |
| Swift4465 | Tests-only RED on original facade; intended admission assertion fails |
| Private25498 /26140 | Failed; four regions rewritten in Export26971 |
| Private28053 | Final candidate32 source files pass |
| Format28723 | Rust formatting passes |
| Private29956 | Actual formatted32 source files pass |
| Similarity29955 | Pass, no exceptions added |
| Core29361 | Compile fails: callback Result error type ambiguous; no tests run |

The compile failure is at policy/driver.rs:318. The accepted-identity callback
needs an explicit error type; three diagnostics describe the same inference
failure. Keep this failed receipt and correct the type before rerunning focused
checks. Source review is not compilation evidence.

Export31017 adds the matching RuntimeError type. Narrow review is clear. Core32893
then compiles:13 of15 selected tests pass. Two tests fail at clock-based holds.
The shared Host clock has other callers, so those holds do not prove Driver
ownership or Settings changes. The reviewer recommends the existing Driver
projection hook, retaining all checks and limits. No production defect is
established by these two results; the exact other clock caller was not recorded.

Export35415 uses the Driver's one-shot projection hook for those two holds.
Narrow review is clear, with every assertion and5s/10s limit retained. Core38143
passes all15 selected tests. Private38618 and Similarity38863 pass the changed
test file. Core39078 reports one lint error: move a block out of an if condition
in policy_runtime.rs while keeping its effect check at the same point.

Export39789 makes that one-region style correction. Independent review finds
the same effect, insertion, branches and error order. Core41920 could not reach
Docker from the sandbox; it ran no lint. A permitted host run is in progress.

A later lifecycle review finds a separate suspended-close regression. The new
status job is correctly cancelled while the registry is paused, but native close
requests that status only to validate its original owner and ignores the value.
S4 permits closing after suspension. The proposed repair is a short original-Host
lifetime and generation check with no Driver lock, projection or job admission.
Ordinary cloud jobs must remain cancelled during suspension. Repair and focused
regressions remain pending; native Resume ownership is outside this correction.

Core42677 completes production lint but finds two nested-if warnings in the new
Rust tests. Export43159 adds the narrow owner-metadata check and three real Rust
lifecycle regressions, a bridge regression and an active-SDK native-close test.
Independent review clears production behavior but finds unsafe teardown in the
new native test: it could close Core and delete storage after failed native drain.

Export46789 preserves Host and storage on failed drain and mechanically collapses
the two test conditions. Independent review has no P0-P2 findings. No adapter,
Resume, registration or coordinator ownership code changes. Format47915 passes;
it leaves the exported patch identical. Private48188, Similarity48592 and
CoreLint48547 pass. Core49187 passes all18 selected tests;314 tests are outside
the selection. Swift and native lifecycle checks remain pending.

Full synchronous final-enqueue responsiveness and retained-execution contention
remain open. The existing native U114 Resume failure and repair cap remain.
No full gate, milestone, complete lane isolation or signed round trip is claimed.
