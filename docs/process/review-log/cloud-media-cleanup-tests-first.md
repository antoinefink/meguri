# Media cleanup test inputs

Author: implementer `s-20261025T134357Z-235af9a2`.
Repair session: fixer `s-20261025T162405Z-9dc656e4`.
Base: `3f241c19b77db754b71c0d8c433f25fd674903ae`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: K9.5, P10.3, P24 and Q11. No specification edits.

The original three cases in `meguri-sync/tests/cloud_media_cleanup.rs` retain
real publication, media storage/trash operations, Service quota responses,
original ACK envelopes, daily fetches and process reopen. They require an
actual original SDK deletion, a successful persisted Service outcome, removal
from the pending SDK state and durable Deleted state after reopen. They contain
no synthesized ACKs or pending lists, retries, skipped failures or relaxed limits.

The quota cases keep the earlier live ACK for 31 daily drains, then obtain a
successful replacement/removal ACK before testing the retention cutoff. The
second client stays unbound so it cannot supply an extra server name. The
original no-quota case also requires an SDK deletion after its retention wait.

Observed failures from the earlier integration remain evidence: the Service
blob-presence assertion failed in the no-quota case; both quota cases stopped
in drive_budget before reaching that assertion. An earlier failure is still a
failure. No later source change constitutes a successful execution.

The initial seed added only pending-state forwarding and read-only persisted
blob observation to its test harness. Later implementation and R1 additions
are documented in `cloud-media-cleanup-implementation.md`. The complete source
still requires integrator compilation, execution, review, privacy/provenance
and similarity checks. Full reviews and mediaCloudKit remain unaccepted.
