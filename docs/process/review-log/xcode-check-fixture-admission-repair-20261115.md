# Xcode-check fixture source admission repair

Starting commit: `5fe698f3c6677c05a2240f23998607d6023bbd63`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The full host selftest `20261115T000604Z-30708` executed 896 cases:
894 passed, two failed. Both failed before their intended build or VM stage:
the synthetic source tree no longer satisfied the real input admission check.

The fixture setup now commits its completed synthetic source and supplies the
existing strict producer doubles through the generated dispatch wrapper.
Production admission, ownership validation and test assertions remain enabled.
The two cases additionally check the resulting input artifact and producer calls.

Filtered author: `cleanroom/20261115T001639Z-71370`.
Independent source review: `cleanroom/20261115T002703Z-81203`, no P0–P2 findings.
Integrated source matches the reviewed export in all three changed files.
Focused verification: `selftest/20261115T005827Z-88820`: 2 PASS, 0 FAIL, 0 SKIP.
Coverage: the synthetic admission and stage dispatch paths. Actual Apple builds
and VM behavior require the full integration gate.

The original full gate `20261114T230301Z-6779` was cancelled after its Swift
package run stopped making progress. It remains incomplete. No unit, milestone,
or release is accepted by this focused repair. The next required full gate keeps
the original pending integration base.

Privacy: `private/20261115T005933Z-90159` PASS, four files.
Similarity: `20261115T005901Z-89481` PASS; all three shell files are outside
the scanner languages (zero compared). No similarity coverage is claimed.
