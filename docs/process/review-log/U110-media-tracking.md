# U110 media tracking — in progress

> **2026-10-26: Anki server work is frozen indefinitely.** Earlier next steps
> and acceptance requirements below are retained history. Do not resume them
> without Antoine's explicit request. See [the delivery scope](../review-first-delivery.md#anki-server-freeze--2026-10-26).

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
integrated with additive shared-contract updates and independent intake review.
All 12 scenario/golden files match their prepared hashes. Runtime and freshness
checks are not yet certified here. Quick-check/reset grammar and adapters,
remaining edge cases, measured T10/T12 budgets and the
required original-base batch gate remain pending. No Unit trailer or U110
acceptance is claimed.

## Large upload resume check

Focused Docker run `20261025T193326Z-15565` passes the enabled 19,000-file
regression. It cancels after 9,500 acknowledgements, resumes the remaining
9,500, checks every uploaded name occurs once, and checks one inventory per
push. This uses the production media stage with the controlled local test
transport. It does not supply reference-server parity, device budgets or UI
acceptance. Source was clean at `8a9cf700` for this run.

Prepared intake review `meguri-u110-intake-review-r1.json` has no P0–P2
findings. Original source-session records were recovered through the session
store in `729430fa`, `a674aac7` and `56b1f153`. No Unit trailer.

## Reference support dependency

The prepared Oracle media operations were recovered in an isolated experiment.
Run `194334Z-24591` executes 95 checks: 94 pass, one existing backup interval
check fails with the prepared global file-stat setting. All six recovered
tracking cases and the same-time rewrite case pass. The seven experimental
paths are saved outside main, then restored to their exact earlier hashes.
The failed result remains recorded; no backup assertion or contract is changed.
Reference fixture execution needs a compatible stat mode before this intake
can land. U107 transport verification proceeds with the existing stat mode.
