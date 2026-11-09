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
