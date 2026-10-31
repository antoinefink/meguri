# Payload memory follow-up

Start: `4e8b18c675b6b75807a1bcf10454d484714b126c`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Two guarded recovery reads now use headers for flight and ACK metadata.
Neither consumer reads, validates or sends a payload. Independent review is clear.

`core-test/20261031T173934Z-13394` preserves every recovery data check.
Peak Rust heap falls from 378,015,193 to 336,049,209 bytes, still above 300 MB.
Cumulative samples place the remaining jump during fetch; before it, peak is
297,304,566 bytes. No counter reset or limit changed. Native RSS remains unverified.

This is partial progress. The memory test stays enabled and red; no Unit acceptance.

## Final reconciliation read

Repair starts from `3eaaa849a64fb27d903831b52b47221d88e70e72`.
Stage readings show the extra allocation after the last hook. The final scan
now takes revision and name from tracking headers. It keeps the same lookup,
paging and checks. Independent review found no P0–P2 issue in this repair
or its fixed-size measurements.

`core-test/20261031T181325Z-26352` retains the failed 336,049,449-byte result.
`core-test/20261031T182210Z-34376` passes both selected tests: actual paged
coverage without payload reads and the unchanged eight-revision recovery workload
under 300,000,000 bytes. Every data assertion remains. Nine unrelated tests were
filtered out. The test counter has a positive control and covers both fetch phases.

Full native RSS and integration evidence remain outstanding. No limit, retry,
counter-reset boundary or acceptance rule changed. No Unit trailer is added.
