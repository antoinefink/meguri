# RF1

Starting commit: `5d6517071681878d6ad7fd6f22cfa587961cf230`.

Antoine requested a faster path to a minimal review app on 2026-10-22.
`docs/process/review-first-delivery.md` records the revised execution priority.
This item connects existing production collection, scheduling and rendering APIs
through the app shell. It is a coherent part of the listed plan units; it does
not claim those units are complete or that local import already works.

No source implementation or prepared work has been applied for RF1 yet.

Prepared U51 specs reused before implementation:
`f344504c` → `415775c5`, `0684fade` → `10913245`,
`dc8b780f` → `5727b42c`. Original authors, sessions and Prepared-Commit
trailers remain. The spec writer removed one optional provenance line and
rewrote the timer sentence after privacy-hook rejection; timing facts stay.
The clean prepared worktree was removed normally; its spec ref remains.

Spec-writer session `s-20261022T054036Z-254feb0f` added the bounded contract
`docs/process/review-first-contract.md`, commit `ccb933f1`. It identifies the
existing production APIs, two scheduling bridge seams and app-owned state.
No new Anki behavior or oracle scenario is needed. All source exclusions
remain mandatory for the filtered implementer and fixer.
