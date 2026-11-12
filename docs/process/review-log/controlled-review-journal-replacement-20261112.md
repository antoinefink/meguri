# Controlled review journal replacement

Starting commit: `077c791f67e858bbc71c95ad0c97589433e47782`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Contract: `sched/review-session` C12.5, C12.7 and C12.8.

## Problem and repair

Managed Darwin opening can preserve a resolved journal's historical path while
using a separately verified current file binding. Controlled replacement then
writes the retired marker, but ordinary provider reopening rejected that old
path before reading the marker. A native service regression reproduced the
resulting `AfterActivation` / `pending_answer_identity_mismatch` failure.

The old journal worker now captures its exact retired file descriptor and bytes
before replacement. Provider reopening may consume that source only with the
same core, current privileged replacement admission, valid generation and
current live file identity. The complete codec, exact source, directory,
archive and persistence checks remain required. The strict decoder is unchanged.
Cold opening of a same-path migrated retired journal validates its archive
before resetting; old native equality no longer hides that valid retired case.

Cold opening with a historical path and no surviving worker authority still
refuses. This change introduces no general path or identity exception.

## Authorship and independent review

Filtered clean-room runs:

- `20261112T180901Z-34710`: original real-service Darwin regression.
- `20261112T181730Z-40913`: real migration/archive positive coverage.
- `20261112T182147Z-45143`: bounded repair and actual process-crash coverage.
- `20261112T183100Z-53480`: independent source review; no P0–P2 findings.
- `20261112T183815Z-59875`: names, comments and assertion-message cleanup only.

The final mechanical diff was checked against the independently reviewed source.
It changes no guards, assertions, ordering, time limits or fault boundaries.
The existing named C12 proofs remain unchanged. No prohibited source or private
input was supplied to the filtered authors or reviewer.

## Verification

- `native-durability/20261112T182716Z-50704`: retained RED, 256 passed and one
  failed. The moved resolved-journal replacement failed at real provider rebind
  with `pending_answer_identity_mismatch`; the same-path migration test passed.
- `native-durability/20261112T183358Z-55777`: 259 passed after the repair,
  including a real abort after the replacement directory sync, missing-archive
  refusal, no old-answer replay and durable fresh journal reopening.
- `native-durability/20261112T184030Z-61581`: 259 passed on final source after
  the mechanical cleanup; no ignored tests or changed timing limits.
- `core-lint/20261112T183719Z-58457`: three checks passed before the mechanical
  cleanup. `core-lint/20261112T184404Z-64245` passes all three final checks.
- `similarity/20261112T184414Z-64817`: all four final source files pass, with
  no exceptions.
- `private/20261112T183719Z-58757`: scan refused new text;
  the guard was not changed. `private/20261112T184013Z-61301` passes all four
  final source files after the wording cleanup.
- `private/20261112T184457Z-65287`: final source and review record pass.
- `static-checks/20261112T184457Z-65285`: three checks pass.

This is a repair commit, not a completed unit or milestone. The pending batch
base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`; no full gate is claimed.
Review changes still need to reach the other device, and sync status needs
repair. Fresh initial adoption and native media display do not establish a
complete review round trip.
