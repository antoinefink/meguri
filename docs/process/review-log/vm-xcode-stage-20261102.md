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

Focused selftest/20261102T143250Z-59034 passed all 10 selected wrappers,
including host smoke checks. Static-checks/20261102T143436Z-59473 passed 3/3.
Actual vm-xcode-stage/20261102T143513Z-60255 then returned error before the
worker could start guest control. Its private temporary path used a system
alias that strict Directory admission rejects. This is not a queued activation
or staging pass.

Content-free proof retained at build/vm-xcode-stage-60255-admission-proof.json
checks source53bc57ac and the exact holder, both original processes dead,
strict admission blocked before guest work, no receiver evidence, and the
empty owned guard. The system alias predates the run on a read-only root.
Independent review cleared removal of only that guard under the lease mutex.
Root performed that narrow cleanup and retained every result and diagnostic.
Normal MG stale-holder reclamation owns the remaining lease cleanup.

Filtered bootstrap repair cleanroom/20261102T144140Z-61600 resolves the OS
temporary base before mktemp. Directory admission remains strict. Private
diagnostics now retain fixed error classes and own function/line frames only.
Independent review found no required issue; staged privacy63117 passes.
Focused checks and actual execution after this repair remain pending.

Focused selftest/20261102T144627Z-64013 passed 8/8 wrappers after the
bootstrap repair. Actual vm-xcode-stage/20261102T144741Z-64375 then refused
the selected host Xcode's group-write metadata during source inventory,
before guest copy or activation. Fixed private diagnostics identify walk242.
The owned worker produced its safe-release proof; no guard cleanup was needed.
Read-only diagnosis found 673 root-owned wheel-group entries with group-write
bits, no world-write or set-id bits. No installed bundle or permission changed.
This source-authority issue remains open; no staging or activation pass.

Filtered repair cleanroom/20261102T150107Z-71495 distinguishes the verified
root/wheel-owned installed source from the owned private guest copy. It pins
local system identities and credentials. The guest retains owned 0700 root,
work and bundle boundaries and checks every copied entry's owner; archive
ownership grants no authority. World-write/set-id and all other guards stay.
Independent review found no required finding. Integration changes only two
invalid-group fixture names; their wrong-name predicate is unchanged.
Staged privacy81912 passes. Focused checks and actual maintenance are pending.

P2 diagnostic-retention repair resumes reviewed source-check seed patch86308
in existing fixer s-20261025T162405Z-9dc656e4. Each original failed command
stream retains at most the existing 1 MiB; private metadata records its original
byte count, retained count, explicit overflow and unchanged command exit status.
Size admission uses original file sizes after retention rather than read offsets.
The added generated failed-child regression exceeds both stream caps and checks
bounded private prefixes, original exit/size/overflow and public redaction.
Source-check design, exact signature predicate, staging and join behavior stay
unchanged. One source consistency pass only; no tests, native source checks,
builds, VM, SDK or account operations run here. Root owns independent re-review,
focused tests and the read-only actual source check; no verification pass claimed.
