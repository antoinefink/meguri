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

## Window-capture repair handoff (2026-11-01)

Fixer session: `s-20261025T162405Z-9dc656e4`. Starting master: `1e08ce40`
(root binds the full hash). Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The independent P2 is addressed in the diagnostic method with a local app-window
screenshot helper. Fixed phases capture before the audit, at every `onIssue`
callback, and alongside after-geometry in the existing defer, including audit
throws. The shared counted-finding capture remains untouched, even where it
duplicates this diagnostic's window capture. The single `.contrast` call and
the original full `.all` audit remain unchanged. No controls, styling, retries
or budgets change.

The contrast VM run was still pending at prompt writing. This filtered repair
has only one bounded source consistency check and adds no runtime evidence.
Root owns focused MG VM receipts, independent review, privacy, provenance and
similarity checks. No runtime pass, product repair or acceptance is claimed.

Independent review clears the latest source diagnostic changes (no P0-P2).
The initial ui-test/20261101T102216Z-77692 failed fixture readiness before
any audit. It establishes neither a contrast finding nor a pass. Failure-state
and app-window evidence are the next diagnostic; budgets remain unchanged.
