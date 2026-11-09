# Live CloudKit resume contract

Starting commit: `9efbe290`. Integration start: `db52dddf`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Spec-writer session: `s-20261109T165344Z-77e618c8`. Anki: none.

Native `20261109T164758Z-23220` retains the already-active resume failure.
The real command drain fixed fixture ordering, but not Runtime authority loss.
A foreground drain would wait for network work and is not the chosen repair.

The written proposal received independent technical review.
Initial review found a P1: automatic rebind after any failed preservation check could repeat the active-owner race.
The revision separates unchanged active retention, active refusal and genuinely drained reopen.
Current SDK validation, full descriptor/native/publication checks and final original-owner checks remain required.
Read failures cannot become positive mismatch evidence or a fabricated durable isolation reason.
Original cleanup remains retained. No active path may rotate authority to a successor.
Final independent contract review reports no remaining P0–P2 finding.

The edit adds the reviewed rule under engine/suspend S2. It closes no P24 item.
Required implementation witnesses cover held network/media, prompt local review, Available ordering, external invalidation, true suspension and R11 proof reuse.
This records technical review, not owner approval, source verification or unit acceptance.
