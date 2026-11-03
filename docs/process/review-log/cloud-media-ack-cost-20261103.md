# U113 media ACK cost

Status: WIP; no unit acceptance.
Start:d64c317f; source:131bc639.
Original U113:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T111007Z-85555bd0.

A signed Mac run waited at review preparation during publication. Its CPU
sample reached media reference scans in the ACK handler. Each media ACK
materialized every name twice. A receipt now owns one reference-count map;
each committed row updates its counts. Both age checks and each transaction
remain. Conflicts use the existing standalone path. No native/Resume change.

| Receipt | Result |
| --- | --- |
| Export10879 | Production change and regressions |
| Rust16682 | Tests-only RED:2 passed; scaling failed |
| Rust18027 |33 passed; new measurement assertion failed |
| Export21979 | Test-only measurement correction |
| Rust24554 | Corrected tests-only RED:2 passed; scaling failed |
| Private24943 |5 files passed |

For250 Saved records, the old full event read150850 bodies at300 media names
and301450 at600. The repair read1150 and2050. These are body counts, not
elapsed-time or complete-sync results. Ledger serialization still repeats.

| Measurement | Scope |
| --- | --- |
| Event | ACKs and two eligible calls |
| Saved | Actual ACK loop counter |
| Bound | M+6B for both; Saved delta at most300 |
| Error | None for an unfinished Saved loop |
| Assertions | Existing authority, retention and write-failure checks kept |

Independent R1 production, R2 measurement and R3 merged reviews are clear of
P0–P2 findings. The merge preserves both reviewed loop changes. The privacy
hook rejected two new test lines; the filtered correction rewords both regions.
No guard exception was added.

Rust29131 passes34/34. Private30140 passes7 files. Similarity30363 passes7 files.
Lint31827 failed formatting only. Format33197 has independent review;
lint33699 passes4/4 and similarity34239 passes7/7 without exemptions.
Log privacy32778 passes. Signed-app latency remains pending. Preserve
the older native Resume failure and its repair cap. Full integration, signed
media convergence and device round trip remain open.

Build35490 passes from clean d0b17189. The signed executable SHA256 is
522cf970ecd09be2e4dc33d0925959c621dea2971e651651342c0c00df05ca30.
The old preview quit normally. The new preview reaches Syncing Media, but
review preparation still waits. A one-second sample of PID38142 reaches
policy_media_facts_locked through policy_projection on the maintenance lane.
This is separate from the repaired ACK loop. No latency pass is claimed.
Static35260 passes3/3; master was pushed through d0b17189.
