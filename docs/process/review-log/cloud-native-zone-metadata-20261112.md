# Native zone metadata and normal reviews — 2026-11-12

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No unit, milestone or signed-lane acceptance is recorded.

The phone completed a real review and Undo. A bounded native repair then allowed collection records to flow in both directions. Complete peer contents, reliable cold discovery and media rendering remain under verification.

## Source and review

`8c89663b` adds the bounded Debug native-fetch trace. Filtered author `12403` and independent reviewer `14385` cover it. Focused Swift run `13432` passes 32 tests. `a2ddf86c` adds the existing curated fault beside that trace, without taking caller error text. Author `21782`, reviewer `22479` and Swift run `22775` cover it; 36 tests pass.

Repair start: `a2ddf86c`. Commit `825625be` changes the database-deletion handler. It validates all owners first, accepts only unrelated current-owner metadata, and preserves refusal for reserved zones, the bound zone and all epoch-zone names. The Debug U tag contains no name or payload. A nonempty accepted deletion list supplies it. Old assertions and all native lifetime and scope guards remain.

Filtered author `cleanroom/20261111T234008Z-25429` exports the repair. Independent review `234323Z-26992` reports no new P0–P2 findings. It also confirms the preexisting gap: recognized zone loss still lacks its full Core translation under K12.5. The patch does not close U114.

Focused `swift-test/20261111T234329Z-27352` passes 57 tests. Static checks `234507Z-30306` pass three checks. Similarity `234508Z-30305` passes. These receipts cover the scoped changes from `bdbd2168`; they do not replace the required integration gate.

## Original device evidence

At `a2ddf86c`, phone inspection `drive/20261111T234056Z-25883` shows a database callback, subsequent empty fetch scope and native release. All six interaction steps pass; 40 hierarchy findings keep the receipt failed. Continuity is verified. The latest fault is stale-command. This does not identify the original native failure.

At `825625be`, normal preparation `run/20261111T234352Z-28497` passes. Retained inspection `drive/20261111T234503Z-30011` shows two original collection modifications, two Core forwards and two outgoing Saved records. Pending reaches zero; no native scope cut or save failure is recorded. All six steps pass with verified continuity; 36 hierarchy findings remain. The U tag is no longer in the bounded tail, so the exact earlier cause is not claimed.

Signed Mac preview `build-app/20261111T234632Z-31675` passes. Computer use opens that exact product after the previous path-verified preview stops. A later normal Settings observation shows four received collection records and four Core forwards. Two records are saved; an earlier single conflict resolves, and pending reaches zero. Last completed advances to a new succeeded result with one selected item. Actual peer deck visibility is still being checked.

## Review evidence and remaining checks

Phone drive `20261111T232822Z-20287` completes all 22 actions: question, answer, Good, next question, Undo, restored question and end. Counts return exactly to `[18, 0, 17]`. Undo enables after Good, then disables after Undo; Redo enables. Continuity is verified. Its 58 hierarchy findings remain failures. The audio-only question has no static text; an empty text comparison proves no card identity. The answer still shows a missing image. Playback is untested.

The original deck still reports 2,389 missing files. Existing exact-byte media-arrival evidence is recorded in `cloud-normal-media-probe-20261111.md`; a completed exchange alone never proves file arrival. Full zone-loss recovery, reliable cold discovery, accessibility repair, integration gates and final acceptance remain open. Private captures and all account values stay outside this log.


## Fresh peer marker and diagnostic freshness

The phone-created deck appears on the Mac after its normal restart. The same saved collection remains open. A fresh Mac rename then supplies one original native Saved record, no save error, and a transition from one pending item to zero. The receiving phone starts this comparison with zero pending items.

`drive/20261112T000019Z-39551` completes all six steps but does not show the fresh name immediately. Its 63 hierarchy findings remain red. `000155Z-40351` then completes all eleven steps, including one Home/return. Before that cycle, the phone has a new successful completion at `1794441701065`, but its native and Core collection counts remain two. Immediate post-cycle captures remain old; they prove no lifecycle outcome.

`drive/20261112T000421Z-41382`: 6/6 actions, continuity verified, 45 hierarchy findings. The renamed deck appears. Native modifications and Core forwards: 2 → 5. A newer completion is recorded, alongside a connection-tracking fault. This confirms receipt. Startup behavior and the error remain open.

The Form's short sequence stays unchanged while the nested counters advance. That alone does not prove a cached view: repeated events can yield an identical bounded tail. Remount observation `000759Z-43033` completes ten steps with verified continuity and the same text; 99 hierarchy findings remain red. The trace lacks an append count to distinguish those cases. Its render snapshot cannot establish later current SDK state. A diagnostic improvement follows; no transport change is inferred from this ambiguity.


## Counted trace verification

Diagnostic start: `6f52b422`. Commit `e5aae23f` adds an append count with saturation and a separate overflow flag. Every append counts, including configuration. Ordinal overflow remains independent. The 16-entry tail and the fault fragment remain intact. The new exact maximum is 306 bytes for the sequence and 494 combined; the native 512-byte limit is unchanged. A repeated tail now has a distinct count until saturation. This remains render-time observation.

DEBUG: the existing Sync Now control samples the recorder when status renders. Production UI and transport are unchanged. No new control, network operation or periodic refresh is introduced. Discovery remains open.

Filtered author `cleanroom/20261112T001045Z-44695` and exact-diff independent review `001327Z-45208` cover the combined patch. Review reports no introduced P0–P2 findings. Focused `swift-test/20261112T001353Z-45773` passes 39 tests. Static checks `001357Z-46769` and similarity `001358Z-46768` pass.

Signed normal phone preparation `run/20261112T001439Z-47766` passes. Retained drive `001609Z-49140` completes eight actions with verified continuity; 63 hierarchy findings remain red. The native Sync Now button exposes count 14. The Form no longer exposes the trace. Its exact screenshot was visually inspected: standard native controls remain intact, and diagnostic text is not drawn.

Later read-only drive `001756Z-50031` completes three actions with verified continuity; 36 hierarchy findings remain red. The button now exposes count 24 and a newer successful completion. No additional request or restart occurs between these captures. This verifies native exposure and status-driven refresh. It establishes no broader sync or accessibility pass.
