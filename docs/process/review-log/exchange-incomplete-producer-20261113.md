# Incomplete exchange evidence

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Start: `5f7baafa89423ba265f961c02fc8483f2424fbc2`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P12.6.1`, P21; `sync/cloudkit-records#K10`, K11.4, K17.1.

The tracking extension records original work for accepted incomplete exchanges. Completion uses actual ACKs, applied entries or never-sent removal. Original exchange order survives reopen. Older facts stay unknown, and `attention_known` stays false.

Pending work has name and entry indexes, exact remaining counts and compact receipts. Losing a pending row or a migrated legacy condition cannot prove completion. A generic header update supplies no ACK. Final handoff image facts serve both active captures and later durable results. Scope checks and future-schema refusal remain enabled.

A separate regression exposed a missing scan case: undo of a newly created, never-sent deck left reconciliation blocked. The existing guarded Revlog removal now also handles Decks. It retains the snapshot, WAL, account, file, opening, generation, cursor, sent-state and flight checks. Review fixtures now wait for the existing answer flush before making dependent edits. No timeout, assertion, SDK refusal or uploaded-undo protection was relaxed.

Filtered exports: `cleanroom/20261113T154520Z-25868`, `162217Z-29382`, `164811Z-32429`, `170854Z-34942`, `172814Z-41383`. Independent read-only reviews: `161344Z-27324`, `164044Z-30883`, `170029Z-33429`, `171654Z-36261`, `173641Z-46497`. The final review finds no P0/P1. Earlier repair findings and failed receipts remain retained.

Old production fails the warning regression at `core-test/20261113T161551Z-28014`: absent attention instead of count 1. It also fails `core-test/20261113T173702Z-47450` after the deck's actual absence and never-sent state are proved, at reconciliation. With the final code, `core-test/20261113T174128Z-50324` passes all 46 focused cases, including actual publication and adoption paths. Source privacy `private/20261113T174132Z-50605` passes all 22 code files.

Open P2: same-entry Asset to Generation needs the owning generation-asset lane and a real Host test. Existing selection, result and staging guards prevent that path today; setter and replacement cases do not prove it. Those guards remain. Full U113/P21, global K10, native regressions and required integration gates are still open. No Unit trailer, signed-lane result, milestone or release acceptance is recorded here.


Broad comparison: `core-test/20261113T174445Z-52624` passes 746 of 760 cases. `core-test/20261113T174743Z-54682` runs the 14 failures on unchanged master: 13 fail there too; the new failure is the expiry table inventory. These existing failures stay open, including review-roundtrip, adapter, recovery and large-payload cases. The final repair keeps all seven new tables in the expiry inventory and adds actual last-copy expiry/reopen coverage. `core-test/20261113T175136Z-58829` passes all 53 focused cases.

Source checkpoint: `41659621`, explicitly WIP. Static checks `static-checks/20261113T175435Z-62257` pass three checks. Similarity `similarity/20261113T175204Z-59915` passes all 24 code files without exceptions. Privacy `private/20261113T175153Z-59554` passes the new expiry code and this log's earlier text. Final retention review `cleanroom/20261113T175427Z-61645` finds no P0/P1. The first lint receipt, `core-lint/20261113T175430Z-61958`, retains three cosmetic diagnostics; layering and its other checks pass.


The retention review records one P2: the accepted-incomplete branch inserts a result directly without the policy receipt and reopens without runtime validation. It proves storage retention, not admission through a fresh Runtime. The separate history-only branch has no policy row that could mask the inventory omission. Add a consistent accepted-result fixture and runtime installation in follow-up; no full Runtime reopen claim is made for that branch.

Mechanical export: `cleanroom/20261113T175758Z-66014`. Two calls use `active(end)`. One test uses `std::slice::from_ref`. Only `acknowledge_sent_locked` allows `clippy::too_many_arguments`. Assertions, limits and behavior stay unchanged.


Final lint: `core-lint/20261113T175901Z-67065`, all three checks pass.
