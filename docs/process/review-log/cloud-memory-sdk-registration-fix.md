# Memory-fixture reopened SDK registration — source handoff

Role: fixer, following implementer restrictions. Supplied candidate base:
`d9df5e6ce59a6c60de93eda65f1dc208d745acf7`; filtered-copy starting HEAD:
`481f078aeb77735ca31b819b6217aed909c39ba0`. Original batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Complete supplied candidate preserved.

Reported `core-test/20261031T170017Z-93947` passed file-IPC lifetime coverage
and the four-payload workload (169648570-byte child Rust peak), but the
eight-revision recovery stopped before its memory assertion. Independent review
identified a host token installed as SDK authority after reopen; StartSDK does
not register an SDK, so the legacy incoming branch correctly rejects asset paths.

Only the eight-revision fixture's post-reopen preparation now uses a focused
SDKRig helper: refresh identity, reconcile, RegisterSDK(None), install the
returned SDK id and collection zone, then confirm the actual root after the
actual database/no-state listing. As with SDKRig's existing full-fetch behavior,
returned serialization does not resume a cursor. Compact reopen proofs stay
before registration; every memory, flight, ACK, newer-local and row assertion
is unchanged. Existing legacy fixtures, production guards/lifecycle/clock,
file-backed callback ownership and all frame/payload/operation limits remain
unchanged.

One scoped source/callsite consistency check only; no forbidden-source reads,
network, builds, tests, formatters or commits. Root owns compilation, unchanged
real workloads, actual idle 502-entry case, native policy/media checks,
independent review and required lint/provenance/privacy/similarity/batch gates.
First-adoption/structural assets and full native RSS acceptance remain open.
No Unit or full-sync acceptance is claimed.
