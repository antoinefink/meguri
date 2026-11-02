# Owner test-account setup

Status: WIP; no actual setup or approval performed.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Source start: ea64c937; integration start:62b10a9f.
Implementer: s-20261101T235351Z-264d9cd0.
Spec: conformance/cloudkit-agent J2.1, J2.2, J2.2.1.

Filtered export cleanroom/20261102T152721Z-81163 adds the owner utility,
setup-only app-hosted entry and generated checks. It reuses actual independent
product admission and private transport. Normal agent headers and account
guards remain required. The entry opens no Core host or collection and uses
only native account reads. Only Antoine may invoke and approve actual setup.

Independent reviewer cloud_smoke_review found one P2: existing-source replace
can lose a concurrent update after its last check and report success. Filtered
repair89048 adds a stable writer lock and validates the actual prior entry
after an atomic exchange. A conflict retains private evidence and reports
an uncertain result. Independent review found no required repair after89048.

Focused selftest/20261102T160214Z-89822 failed 1/22 wrappers: receipt-byte
binding occurred after consuming product fields. Mac compile89823 also failed
with a Swift compiler error. Filtered repair91356 restores the byte
check immediately after the same receipt read and adds the required import.
Review clears those changes but identifies a generated test that forbids that
import; its narrow correction remains pending. No assertions are discarded.
All verification after these repairs remains unrun.
No signed execution, unit acceptance or owner action is claimed.

Integration renames generated test/local names and removes a redundant comment
after the privacy findings. Assertions, operator input and control flow stay.

Repair95512 permits the required standalone type import while retaining checks
against runtime and database construction. Independent review found no required
repair. Privacy99391 passes. Selftest/20261102T161855Z-99389 passes all22
focused owner and signed-agent wrappers, zero skips. Mac CloudAgent compile
build-app/20261102T161521Z-96048 passes on23ceb29d, compile-only.
The separate startup predicate executes in the VM at
mac-hosted-test/20261102T162208Z-2868: one pass, zero skips or failures.
No owner setup utility, account approval or native CloudKit lookup was invoked.
