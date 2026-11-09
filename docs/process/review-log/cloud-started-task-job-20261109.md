# Started activity inside retained SDK tasks

Component start: `71e9f931`. Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The parent units U113, U114 and U151 remain unaccepted.

## Scope and design review

Independent Swift, Rust and contract reviews agree on a Started-only conversion
under engine/jobs J2.1.1 and sync/policy P4 and P15. The final synchronous
closure must contain compact authority checks only. Full Started projection
runs as an existing-pool typed job inside the original retained network task.
After that await, the native engine rejoins event and returned-work bookkeeping
before contiguous final checks and the captured SDK call.

The existing invocation output buffer and lifetime UUID retain ownership.
Existing Ended, cancellation and exact send/fetch finalization paths stay intact.
The aggregate active flag and overlapping-send behavior predate this change.
No new Core attempt token, compensating cleanup, reservation or rollback is
required solely for this move. The previous rollback/token requirement was a
stronger assumption, not an approved contract clause.

Ended work is deliberately outside this component: an ordinary job registry
can discard queued cancelled work, so converting mandatory completion reports
requires its own treatment. Other CoreBridge waits remain open.

## Required focused evidence

Retain existing guard, callback, readiness, latency and cancellation assertions.
Cover queued and contended Started cancellation and generation replacement;
retain exact multi-zone outputs and typed errors after effects and owner close.
Prove preparation failure prevents SDK entry and post-await event work is joined.
Check cancelled and retired preparation with zero SDK calls.
Keep per-invocation outputs and the existing original-operation drain behavior.
Listener failure must drain and recover actual effected outputs.

Implementation and review are in progress. No new verification or acceptance
is claimed in this entry.

## Round 1

Filtered export `37429` passed source isolation. Rust and production Swift
reviews found no substantive production issue. Swift review found one P2
coverage gap: both callback cases held an active event, leaving returned work
after event-idle unproved. The fixer uses the existing returned-work gate.
Original patch privacy `40047` also flags one public Swift test expression;
the fixer preserves full action-payload comparison with another public form.
No privacy exception or assertion change is authorized.

Rust source is integrated for focused checks while that test-only repair runs
in its isolated copy. No acceptance or passing test is claimed yet.

## Focused checks and retained fixture failure

Format passed. Source privacy `43190` passes after the two test corrections.
Core `42428` passes 123 of 124 checks. The new multi-zone test fails its
synchronous baseline nonempty-command assertion; its projection returns an
empty command list. The fixture is under diagnosis. The failed receipt is kept;
no assertion has been removed or relaxed. Native compilation and independent
checks are in progress. This WIP remains unaccepted.
