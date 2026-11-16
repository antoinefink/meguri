# Current Desktop candidate incremental lane

Ten actual Desktop SDK groups pass against the Rust candidate. Source 4e34620f
uses one full-only clean build for both full 11-case and normal 10-case selections.
The original producer remains s-20261115T203832Z-6ecc17e8; it matches both
client invocations and the build receipt. Reference runtime identity stays
separate from candidate source, lock and executable identities.

Selected cases cover no-change, reviews, notes, note-type CSS, deletions and
deck/preset/tag/config exchange in both directions, plus 250/502-row and
251-grave boundaries. Every credited normal SDK call must complete without
pending sync work. Wire and final-state assertions remain required.
Auxiliary-crafted matrix traffic receives no SDK credit.

Source author: s-20261116T001757Z-8fe546bc. Independent combined correctness
and five-rubric reviewer: s-20261116T004450Z-8b23265d. Original session commits
remain visible. One P2 found that metadata-only traffic could pass while the SDK
requested full transfer. The failing proof used a real metadata exchange with
completion 0, then injected the public full-transfer result 2; the old helper
silently returned 2. The repaired shared seam checks completion and retains
actor/raw decisions while preserving all wire/count/state assertions.

Targeted closure:
/private/tmp/meguri-sync-normal-candidate-p2-independent-review.md.
Author v1/v2/v3 exports remain unchanged. Final v3 patch SHA256:
20bf2fcfeb16a335f157c4dae96d48bd392eafae6bf0785a6b27cc91b33d573d.
Of 15 reviewed paths, 13 match exactly and two preserve separately reviewed
web preparation and the copied-registry adapter.

Canonical focused receipts under build/mg:

- selftest/20261116T014102Z-50011/result.json: 30 boundary/registry checks.
- sync-server-reference/20261116T014143Z-50389/result.json: 47 selftests.
- static-checks/20261116T014008Z-44379/result.json: 3/3.
- private/20261116T014058Z-49268/result.json: no matches.
- similarity/20261116T014059Z-49612/result.json: two supported files passed; 16 excluded/unsupported paths skipped. Quarantined source is not scanned.

Actual candidate build 014409Z-51245 and client results 014426Z-51784 (full 11-case),
014436Z-52593 (normal 10-case) pass with both validators, zero exits, true compatibility
credit and checked cleanup. The incremental review log records full identities.
These are debug, no-feature results. Media, Mobile, retained-Meguri, GUI, release
and whole-family acceptance remain separate. Empty or partial runs earn no credit.
