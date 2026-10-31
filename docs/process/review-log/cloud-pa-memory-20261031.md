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
