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
can lose a concurrent update after its last check and report success. Repair
is pending. All generated checks and native builds after this export are unrun.
No signed execution, unit acceptance or owner action is claimed.

Integration renames generated test/local names and removes a redundant comment
after the privacy findings. Assertions, operator input and control flow stay.
