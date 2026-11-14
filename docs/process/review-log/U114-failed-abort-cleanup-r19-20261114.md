# U114 failed-abort cleanup, R19 — 2026-11-14

Starting commit: 67fca5e0. Integration base:
1416a520542a8c5daa38a23bfe398da704c3d6c9.
Authority: sync/cloudkit-recovery V14.3. This repairs reply-path cleanup;
it does not accept the whole sync unit or release.

The four retained diagnostic failures came from incorrect test ordering.
Cancellation consumed the pending reply before the provider barrier, so the
original reply correctly refused to claim it. A separate production defect
then prevented the intended test: after a claimed reply's mandatory abort
failed, both later cancel and close saw a consumed claim and returned Busy.

The owner now distinguishes pending work, an active claim, and cleanup-only
work. After the reply body, abort and diagnostic capture settle, a failed
abort can expose only cleanup authority for that exact retained owner.
Replies cannot claim it. Cancellation consumes that state atomically and
restores exactly the same authority on another cleanup failure. Uncertain
activation stays excluded, active claims stay Busy, and existing owner and
generation checks remain. Host close still drains jobs before cleanup.

The old diagnostic matrices now reach their actual faults before requesting
public cleanup. All input variants, exact errors, diagnostic categories,
retained-resource checks, result stability and limits remain. A new control
proves that cancellation before claim still returns AuthorityChanged with no
diagnostic and no claim-hook entry. Another regression exercises failed
abort, another cleanup fault, public cancel or drained close, and refusal to
replay the original reply. Fixture teardown always attempts close; during
an existing panic it reports a second close failure without hiding the first
failed assertion. A close failure after a successful body still panics.

Filtered author 20261114T222535Z-90998 produced the candidate. Independent
review 20261114T223450Z-92334 found no P0, P1 or P2 issue. Integrator comparison
confirmed that all three verified source files match the reviewed candidate
byte for byte.

Test-first run 20261114T223508Z-92854 applied only tests and support against
unchanged production: one cancellation control passed and five cases failed
at Busy, including the new cleanup recovery case. With production applied,
20261114T223651Z-93523 passed all six. Full FFI run
20261114T223953Z-94276 passed all 392 tests, with none skipped.

Native Swift run 20261114T224411Z-95434 passed all 44 selected model tests;
20261114T224641Z-95433 passed all 13 selected bridge tests. Rust lint
20261114T224343Z-94739 and static checks 20261114T224415Z-96559 passed all
three checks each. Privacy 20261114T223657Z-93801 passed all three files.
Similarity 20261114T224407Z-95064 scanned those files with no findings;
only its safe implementation report was read.

Review limits remain recorded: tracking-commit faults, concurrent cleanup,
account or generation changes during cleanup, and suspended close from this
state need wider coverage. Local-work abort error paths are unchanged and
need their own targeted investigation. Required batch gates, CloudKit lanes,
other platforms, native identifier evidence and final acceptance remain open.
