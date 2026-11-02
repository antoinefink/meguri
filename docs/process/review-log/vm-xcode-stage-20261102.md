# Dedicated VM Xcode staging

Status: independently reviewed; focused tests and actual maintenance unrun.
Integration start: 5e58e13a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Prepared session: s-20261102T133945Z-073138aa, source c147f244.
Fixer: s-20261025T162405Z-9dc656e4.

Original filtered export cleanroom/20261102T134437Z-14700 stages the complete
selected host Xcode under an exclusive lease. Current VM data and the old
Xcode remain in place. Activation requires existing readiness and prompt-free
guest permission. License, first-launch and account actions remain user-owned.

Independent reviewer cloud_smoke_review found P1, incomplete original copy
cancellation/join, and P2, insufficient post-copy disk reserve and output bound.
Repair39747 resolves both. The second review found P1, a later guest control
operation could outlive the earlier copy join proof. Repair53376 retains each
active operation's unresolved guard. The third review found no required issue.
Full export: cleanroom/20261102T142409Z-53376/changes.patch.

Staged privacy scan private/20261102T143121Z-57574 passes after internal
helper/test renames and redundant comment removal. Checks and process behavior
are unchanged. No actual VM action, app acceptance or signed execution is
claimed by source review.
