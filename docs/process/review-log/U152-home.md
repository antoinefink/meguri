# U152 Home — saved deck state

Resume starting commit: `e4a3241ae92b827304ff8300a60b746f0e088d85`. Recorded before source work in
`build/unit-starts/U152-resume-20261025.json`. RF1's original base remains
`5d6517071681878d6ad7fd6f22cfa587961cf230`. The pending integration gate
base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Reuse RF1, the native List and the existing U28 bridge. Connect selection
and study disclosure to saved core state. Keep browser disclosure separate.
Focused checks cover repeat and rapid input, stale collection generations,
failures and reopening. Preserve native navigation and existing reviews.

No new Anki behavior or contract is needed. The audit is saved at
`/tmp/meguri-review-app-delivery-audit.md`. Parent U152 requirements, including
the cache, attach registry and remaining row actions, stay open. No Unit
trailer or platform acceptance is claimed. U107/U110/U111 and their technical
gaps remain recorded while app delivery takes priority.

## Source and first review

Implementer session `s-20261025T134357Z-235af9a2`, export `202805Z-53230`,
reuses the existing owner and native tree. It adds ordered saved actions,
study disclosure and focused state/reopen cases. The public bridge can save
but cannot read current deck selection for fresh navigation; that gap remains.
No second persistence store was added.

Round one has two P2 findings. A failed save can show an error after leaving.
Native Back can allow an old selection to open. Swift `203925Z-57856` has
three compiler errors; no tests run. Private check `204047Z-59722` flags test
lines. Similarity `204046Z-59500` passes.

Fixer session `s-20261025T162405Z-9dc656e4`, export `204348Z-60924`, repairs
the intent checks and callback initialization, adds gated regressions and
rewrites the flagged regions. Two Home preview declarations use the existing
production DTO fixture. Tests and rendered evidence for this source are pending.
The original failed checks remain saved.

## Focused and visual evidence

Round two source review clears both P2 findings. Swift run `204806Z-62752`
passes all 64 tests but fails the job on an unhandled guidance file. The note
is moved out of the Swift target. Run `205211Z-65667` passes all 64 with no
warnings, including real collection close/reopen, existing review behavior
and navigation tests. Similarity `205235Z-66946` passes. The source privacy
scan clears all code; the one remaining log region is rewritten and its
focused privacy check passes.

Official Xcode previews render the production tree in typical dark/light and
overflow dark states. Independent visual review has no P0–P2 findings. Images
stay under `build/unit-intake/U152/visual/`; no baselines are changed. This
proves appearance of the DTO-backed component, not full sidebar interaction
or narrow-width wrapping.

Signed standalone Mac build `210042Z-70528` passes. One older development
copy quits normally; one copy of the latest signed product is running. No
fixture launch replaces the user's development collection. Guidance for that
trap now lives in `app/AGENTS.md`, outside the compiled package target.

Full sidebar/keyboard interaction, remaining platform checks, saved current
deck readback, the rest of U152 and the original-base integration gate remain
open. This is a partial app delivery checkpoint, with no Unit trailer.
