# Original edit commit facts — 2026-11-10

Start: `8e8e1ec2`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`.

OpResult now retains storage's actual requested collection stamp. It remains
present when fixed-clock bytes already match. The original edit retains its
actual same-write note image and requires exact readback, including metadata.
Later unrelated commits may change the collection stamp without changing that
original result. Normal stamping and FFI wire records are unchanged.
The healthy Service fixture closes its owner before stopping transport.

Independent source review found no required finding.
`core-test/20261110T063313Z-79224`: 23 executor and original-role tests passed,
none skipped. Covers clock boundaries, fixed stamps, later writes and malformed
note metadata. Native peer verification and the full gate remain separate.
No signed exchange or unit acceptance is claimed.

`core-lint/20261110T063555Z-82265` passed.
