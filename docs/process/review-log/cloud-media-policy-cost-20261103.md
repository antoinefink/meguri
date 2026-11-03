# U113 media policy lookup cost

Status: WIP; no unit acceptance.
Start:0c251177; source:3af18d31.
Original U113:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T114457Z-4427392e.

Signed build35490 still waited at review preparation. A bounded CPU sample
reached policy_media_facts_locked. Its two find loops searched every blob
for each name. The repair builds one temporary index of SHA256, SHA1 and size.
The first ledger row wins for duplicate metadata. Entity loading and errors,
inbox precedence, name flags, sums and every MediaFacts field retain their rules.

| Receipt | Result |
| --- | --- |
| Export41480 | Index, counter and six tests |
| Rust44612 | Tests-only RED:5 passed; cost failed |
| Private44883 |4 files passed |
| Format45219 | Formatting applied |

RED used the old find predicates with a test-only visit counter. At100 names
and100 blobs it visited10100 candidates; the regression permits300.
The fixed counter includes index rows and selected candidates. Tree-key
comparisons and elapsed time are outside this count. Complete entity reads
remain separately asserted. Every bound and assertion stays enabled.

Independent R1 and final integration reviews are clear of P0–P2 findings.
Coverage includes a real projection, shared content, inbox and missing media;
storage tests cover duplicate metadata, mismatches and held flags. Corrupted
local, base, ACK and flight bodies remain errors. Byte-overflow arithmetic is
unchanged and has no new direct witness here.

Rust45549 passes64 selected cases, including all six regressions. Lint46889
passes4/4. Private45819 passes4 files; similarity45834 passes4/4 with no
exemptions. Log privacy46587 passes. Signed-app latency awaits verification.
The original batch, older native Resume failure and U114 repair cap remain open.

Build47349 passes from clean f57ad8f7. Executable SHA256:
4ea27e278a3f8ab7d3742d559903a59e28ca6c2eb66888fb180982c56e7f8e80.
Swift50017 passes20/21 functions; the same two Resume assertions remain red.
Static48262 passes3/3; master was pushed through f57ad8f7.

The live preview opens a review and first answer. A second review opens during
Syncing Media, but its answer transition waits. The CPU sample still reaches
full entity reads in policy_projection. It does not prove why Reveal waits.
Independent source review traces Reveal through WebKit and shown-hook waits;
those do not directly call MaintenanceLane. Preserve C6 hook requirements.
No live latency or complete-sync pass is claimed.
