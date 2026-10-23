# RF4 — explicit self-hosted server transfers

Starting commit: `480fc88681143a143a2ee146378fe260512dc961`.

Delivery: manual Import from Anki / Export to Anki, with preview, confirmation,
safety copies, recovery, cooldown and explicit media scope. No AnkiWeb contact.
Full first-version media support remains required. Parent units stay open.

Reuse: U105/U106 code, prepared U109 contracts, existing backup and replacement.
Requirements: R118–R121, R126, R1548, R2060–R2064.

RF3 full gate: pending host setup. RF4 acceptance: pending.

## Contracts and engine prerequisite

Canonical manual/one-way contracts: `87672292`; prepared media tracking/sync
contracts: `72bb8d6a`. Spec-writer session `s-20261023T081747Z-7505bb3d`.
Prepared U109/U110/U111 authorship is retained in those commit messages.
Privacy passes: `20261023T083517Z-18494`, `20261023T084059Z-22559`.
Contract static/provenance check: `20261023T083618Z-20050` passed.
Catch-up: `20261023T083554Z-19580` passed, no queued entries. Disk: 68 GB free.

Engine work starts at `87672292`; recorded implementer session
`s-20261023T083533Z-1769cf18`, filtered source `6eac53cc`, run
`20261023T083545Z-19227`. The patch separates package stamps from publication
decisions: FullDownload requires a fresh pending decision; FullUpload preserves
the prior decision. Verified manual transfers refuse active review sessions.

Focused replacement/recovery tests `20261023T084454Z-24357`: 24 passed, no skips.
Privacy `20261023T084457Z-24600`: passed. Similarity
`20261023T084509Z-24867`: 5 files, no flags or allowlisted matches.
Formatting `20261023T084434Z-23939`: passed. Lint
`20261023T084528Z-25281`: failed on a complex test evidence tuple; repair open.
Independent round 1 review is in progress. No UI changed in this prerequisite.

Round 1 found the test tuple lint and a misleading admission comment. The public
check is valid after drain, but exclusive entry does not call it automatically;
transfer callers must repeat it before preparation or remote mutation.
Fixer session `s-20261023T084708Z-2448b237`, isolated run
`20261023T084720Z-26476`, corrects the comment and names the same test evidence.
Round 2 review found no P0–P2 issues. No assertion or executable guard was reduced.
Privacy `20261023T084854Z-27836` passed. Final full engine-package verification
and lint are pending; the earlier failed lint receipt remains retained.

This is incomplete RF4 work, without a Unit trailer. Transfer orchestration,
native configuration/login/preview, collection/media execution and recovery,
focused server/native evidence and the required full gate remain open.
