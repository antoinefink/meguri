# Exact native record observation

Starting commit: `55ddb95789e93c9c7241c459528fa34b507de60d`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.

The phone retained three accepted review acknowledgments while the Mac's manual
fetch returned success with no zone callbacks. The records later arrived through
an automatic fetch. The disposable fixture now has matching card and review rows
on both devices apart from compatibility USN; both media files match. Delivery
was observed after the earlier check, so prompt warm delivery remains unproven.

Add an on-demand signed development Snapshot selector for one exact-ID read of
at most eight original acknowledgments. It compares encrypted bytes and change
tags. Input and native output stay private; public results contain fixed counts
and error classes. No writes, state reset, query, retry or acceptance authority.
The original scope diagnostic remains separate. The owned temporary input and
host lease remain fenced until the original native children are joined.

Filtered authors: `cleanroom/20261112T192636Z-91000`,
`cleanroom/20261112T193811Z-94931`.
Independent review: `cleanroom/20261112T194548Z-97078` found one P2: opening
special files could block before type validation. Repair
`cleanroom/20261112T194800Z-98590` adds nonblocking opens and bounded FIFO tests.
Second independent review `cleanroom/20261112T195020Z-99118` reports no P0–P2.
Three tooling comment/test-name scan matches were reworded without changing
assertions or guards.

Verification:

- `selftest/20261112T195308Z-2316`: focused controller suite passes.
- `lint-ui/20261112T195222Z-1199`: 146 files pass.
- `private/20261112T195256Z-2013`: six source files pass.
- `static-checks/20261112T195221Z-615`: three checks pass.
- `similarity/20261112T195310Z-2871`: three files pass, no exceptions.
- `cloud-record-read-host/20261112T195309Z-2583`: dry route only.

Native compilation and the exact read follow this checkpoint commit. This is
WIP integration evidence, not unit, milestone, signed-lane or full-gate evidence.
Pending batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
