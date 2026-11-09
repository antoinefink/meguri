# Cloud reader lifetime contract

Starting commit: `7452b2144330e29a4f6f2f8891ccd6fd4bf7559e`.
Spec-writer session: `s-20261109T084309Z-3e8bd3ea`.
Session ledger: `4046ecd6`. Anki: none.
Scope: `engine/lifecycle#L3`, `#L8`, `#L11` only.

Independent review cleared the full proposal and the exact spec text. A second
review cleared this reader-only scope with no P0-P2 findings. It preserves the
current jobs and suspend rules. No implementation or unit acceptance is claimed.

The reader proof must show the same connection and generation, no held slot
mutex, one reader per role, and unchanged thread ownership. Replacement and
close must wait until the pin returns the reader. Deadline and unwind paths
must retain these rules. A lease emptied by capture_sync must be returned to
its caller on transfer failure; keep the owned snapshot alive while proving
replacement still waits for that lease. Cover a non-Cloud lease too. A waiting
reader must recheck generation and admission. Existing reader tests remain.

The full prepared amendment and review log remain under build. Privacy runs
93881, 95015 and 95816 retain their failures. Reader text passed the last scan.
Only the reader prerequisite proceeds. Production submission conversion and
resource-category work remain held until their remaining contracts land.
No privacy guard, test assertion, deadline or acceptance gate changes here.
