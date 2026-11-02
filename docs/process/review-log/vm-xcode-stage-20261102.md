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

Independent re-review accepts the bounded diagnostic repair88807. Its first
Docker selftest94432 failed six new Python cases because simulated Darwin
selection also selected a Darwin-only publication primitive on Linux. Repair
98136 keeps the actual OS publication primitive and all admission assertions.

Original source-only check95765 retained codesign exit1 for a missing
requirement file. Apple's local codesign documentation requires a leading
equals sign for literal source.
Repair98136 changes only the argument to `=anchor apple`, preserving the
expression and deep, strict and all-architecture verification. All exact argv
assertions match. Independent review found no required repair. Privacy152 passes.

On cleanf6e0b036, selftest/20261102T161929Z-563 passes12/12 wrappers,
including57 generated Python cases. Actual source-only check
vm-xcode-stage/20261102T161931Z-998 exits3: integrity checks succeeded but the
explicit Apple-anchor requirement did not. This is distinct from the earlier
argument error. No VM probe, copy, lease or activation occurred in either
source-only run. Original output stays private and bounded.

Independent review found no Apple primary source supporting a replacement
publisher identity pin. A generic Apple anchor admits other publishers; the
observed bundle identifier, team and self-designated requirement alone do not
establish publisher authority. Staging remains refused with its guards intact.
Ordinary generated app-hosted tests can still run on the existing VM toolchain;
they do not satisfy the stricter signed-lane toolchain requirement.
References: Apple TN3127, TN3161 and DTS forum thread694904.

## Documented App Store identity branch

A bounded primary-source check resolved the earlier authority gap. Apple
TN3127 documents its production Mac App Store signing certificate field
1.2.840.113635.100.6.1.9 with the Apple generic anchor. TN3161 explains the
App Store signature. Apple's app497799835 record confirms the source identity.
Sources: https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements
https://developer.apple.com/documentation/technotes/tn3161-inside-code-signing-certificates
https://itunes.apple.com/lookup?id=497799835&entity=macSoftware
https://apps.apple.com/us/app/xcode/id497799835

Implementer264d produced filtered37753 from67787244, original94887 exit0.
The requirement now binds the exact Xcode signing identifier to either the
original Apple-internal anchor or the documented production App Store branch.
The generic anchor alone, an observed team and developer signatures remain
insufficient. Deep, strict and all-architecture verification, signed metadata,
source pins, ownership, copy limits and guest cleanup fences are unchanged.
Independent review found no required repair. Selftest39184 passes12/12
wrappers, zero skips. Privacy passes. Native source acceptance, staging and
activation remain unrun under this correction; no app or SDK result is implied.

## Original copy failure evidence

Native source-only run40736 passes the corrected signature requirement. Full
stage41513 then refuses in the original copy command. Its local worker and
guest receiver are both joined; no activation or matching toolchain is claimed.
The original command discarded its stderr, so its copy failure cause remains
unknown. No retry or copy behavior change was used to label that failure.

Filtered implementer264d export47446 adds private retention of the original
failed-copy return code and separately capped 1 MiB streams, with byte counts
and overflow flags. Exclusive no-follow files remain mode0600 within the
retained current-user mode0700 directory. Source evidence has separate names;
original worker and receiver joins and uncertain-operation fences are unchanged.
Independent review found no required repair. Selftest49837 passes12/12 wrappers,
zero skips; privacy50076 passes. Earlier selftest49432 could not reach Docker
inside the restricted command sandbox and ran zero tests. The authorized
OrbStack run49837 supplies the focused evidence. Actual failure diagnosis under
this correction remains pending.
