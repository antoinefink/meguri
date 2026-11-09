# Native media, fetch and send admission jobs

Status: focused Rust checks pass; native verification pending. No acceptance.
Component start: `29eabb01`. Original open batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Session: `s-20261103T133750Z-f276f58e` (fixer).
Contract: engine/jobs J2.1.1, sync/policy P4, cloudkit-recovery V3.4.

## Scope

Only beginMedia, beginFetch and beginSend move to retained original-Host
jobs. They reuse the existing pool, owner/generation authority, ordered
Swift helper, typed cells and drained resource-result recovery. Original
synchronous Rust callers retain default-work wrappers where needed.
Final enqueue validation and Started reporting do not change.

Media preparation and selection share the same first-effect latch. Fetch
checks after State acquisition and commits before its first counter change.
Send retains prior pending-update effects and clears retry state before the
eight-call limit. Its completed optional call is wrapped in a record: a
successful NoCall remains distinct from an unfinished job.

## Review and evidence

Filtered export `11882` passes boundary probes. Independent round-1 review
has no substantive P0–P2 finding. Existing real-capability tests are extended
for all five admission calls rather than replaced. Positive Driver/Runtime
hold witnesses, cancellation/retry, real replacement after entry flush,
close/drain, exact errors and retained values stay covered. Old assertions
and timing limits stay enabled.

The prior component's 117 Rust and 15 Swift checks provide characterization.
New APIs cannot produce a meaningful old-code RED before they exist. Added
cases exercise media false/true, fetch/send IDs, completed NoCall, actual
post-effect errors, pending-update retention and retry clearing at the send
limit. Swift expands the existing real listener-failure result proof.

Core `25368` passes all 122 focused checks. Format `25013`, privacy `25675`,
lint `26614` and similarity `26898` pass. Native build/tests and the original
batch gate still need their receipts. U113/U114/U151 remain unaccepted;
no Unit trailer.
