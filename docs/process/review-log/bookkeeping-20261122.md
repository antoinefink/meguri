# Plan owner's bookkeeping (2026-11-22)

Branch `unit/bookkeeping-20261122`, PR #132, spec-writer session `s-20261122T180650Z-53054cda`. Spec wording is approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs; each spec change gets an independent review, recorded here.

## What changed

- The Statistics plan records PR #121's approval and the units' progress (U98, U99, U101 done in PRs #125 to #127).
- `docs/process/milestones.yaml` and the parent plan's Delivery Milestones table, Unit Index and a new Superseded Unit Index take the Statistics plan's units in place of U225, U226, U230, U231 and U236 (U288 and U293 in M1, the rest in M3).
- The parent plan notes the 42 requirements of the Statistics plan's §5, R1363 and R1838's clause as superseded, and adds R2067 to R2079 for the speed rows of `stats/history-reads` H19 and `stats/statistics-page` S19.
- Spec fixes: `stats/graphs-common` G4 (`usn`, following `model/row-records` R1), `stats/graphs-revlog` V8 (U104 owns the observation), G6 (each record's member order, recorded from the code of PRs #126 and #127 this first time), `review/answer-flow` F9 (PR #120's tap zone rulings).
- `docs/process/verification-status.md`: six follow-ups queued.

## Review

Reviewer: an independent Claude (Opus) subagent, read-only, given the four spec commits (G4, V8, G6, F9), the sources each was taken from, and two light checks (T21 of `sync/status-strings`; the H19 and S19 requirement columns). It read the specs, the plans, `docs/design/decisions.md`, the record and encoding sources of `meguri-stats`, the tap pairing source and the sync string catalog; no `oracle/`, `refs/`, `local-data/` or Anki source.

Findings, all fixed:

- G4: should fix, the same out-of-range `usn` example remained in `stats/studied-today` T3.5 and `stats/card-info`'s `unreadable_record` row; both now follow R1. Nit: "Corrected" became "Changed", the format's word.
- V8: should fix, the Status paragraph of `conformance/stats-ops` H10 still pointed Meguri's side at an adapter file that does not exist and named no owner; it now names U104 and the testkit's stats executor. Nit: two other Meguri paragraphs of that file pointed at the same missing file; they now point at the stats executor.
- G6: every row matches the code, and so do the general rules (zigzag for run counts, keys, distances, run and list lengths; unsigned only for text lengths; binary32 little-endian; no record-level tag or length). Fixing the order in the spec is right, since U103's Swift decoder is built from specs only. Nits taken: the Calendar sentence says what V3.4 already fixes; `difficulty` names its member `average`; `card_counts` warns that its order is not C4.4's listing order.
- F9: matches the decisions row and the merged code, and records nothing of PRs #128, #129 or #130. Should fix: the fail-closed pairing rule is PR #120's code and review, not part of the decisions row; it is now its own sub-bullet attributed to PR #120, stating the four rules an implementer needs (each side waits at most 1 s from its arrival; within 8 pt of the lift; after an unmatched touch, refused without a baseline; at most 0.15 s of clock drift between matches, else refused and the baseline dropped).
- T21: no findings. Every key exists in `Sync.xcstrings` with the same English, none of the withdrawn keys remains, and the file needs no Status line (no requirement asks for review before implementation).
- H19 and S19: no findings; each row names the right id. Nit: R2077 now states the per-box progress as S10.3 does (determinate where the core reports a fraction).

Not taken: T21's "removed from the catalog when T21 ships" wording (outside this change; it stays as PR #116 wrote it).

Left for a later pass, behaviour of open PRs: PR #129 (a tap in the answer area that misses its buttons does nothing), PR #128 (the note type editor's Flip is one undo step), PR #130 (the sync control on the closed foldable phone).
