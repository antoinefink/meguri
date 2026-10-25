# U110 media tracking — in progress

Starting commit: `c7f3d340a975886f41578ae70aaa857350ad0325`.
Recorded before the audit in `build/unit-starts/U110-20261025.json`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9` (U107/U110/U111).

## Reuse and narrow change

The audit found working U31 and RF4 media state, journal and scan code. Reuse
that code. The missing stage path always forced a full comparison instead of
using its existing marker. Session `s-20261025T172143Z-1e9723c8` produced
`173807Z-24224`. The marker check now accepts the stage's callback. The old
method calls the new method. Changed folders still
scan and hash with cancellation/progress callbacks. Reset/rebind, unchanged
folders and pending journal entries retain their existing rules.

Independent review found one P2: early folder metadata failure could reuse the
previous successful report and add old counts to the failed stage. Filtered
fixer `175508Z-36482` saves a fresh failed zero-count report. Storage and actual
stage regressions cover folder removal, no remote requests and restore/resume.
Re-review has no P0–P2 findings. Suspended refusal and partial counts remain.

## Focused evidence and remaining work

Combined run `180714Z-48860` passes all 199 executed cases, including media
scan, state and reconciliation checks. It covers the marker/failure/resume
changes with related core changes. Filtered test-only repair `183724Z-79187`
later removes a rejected comment; no test condition changes.

The six prepared scenarios/goldens and related neutral contracts have been
recovered byte-for-byte from `0387ff4d` into a separate intake packet. They are
not integrated or certified here. Quick-check/reset grammar and adapters,
remaining edge cases, measured T10/T12 budgets, the 19k resume check and the
required original-base batch gate remain pending. No Unit trailer or U110
acceptance is claimed.
