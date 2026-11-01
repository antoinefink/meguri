# Card contrast probe

- Implementer: `s-20261025T134357Z-235af9a2`.
- Base: `54af9f9a5cfdb815f61645961e63bc5a111d4551`.
- Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Original failure: `ui-test/20261031T204027Z-43047`.

The diagnostic runs contrast once with the synthetic actions sheet at its
largest text size in light appearance. Refresh must be visible and hittable.
Fixed geometry is recorded before, during a finding and after the audit.
Only the app window is captured. The full original audit remains required.

Independent review clears the retained audit semantics and reports one P2:
add an after-audit window capture, including the throwing path. Capture at
every issue too, since existing finding captures cover counted findings only.
This evidence addition is pending. No product styles or checks are changed.

The initial VM diagnostic is running. No runtime pass or unit acceptance
is claimed. Privacy and similarity checks passed for the source packet.
