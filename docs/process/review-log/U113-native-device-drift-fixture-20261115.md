# Native historical-device fixture correction

Starting commit: `92998120`. Integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The earlier native run `20261114T233735Z-21748` passed 310 cases and failed one.
The device-only drift fixture changed the runtime file tuple without changing
its classification evidence. Ordinary tracking open therefore refused the
fixture before production binding selection.

The existing test-only historical-image helper now permits device-only Darwin
drift with unchanged durable identity and creation time. It validates the
original ledger, writes the coherent image atomically, and preserves every
nonidentity fact. The case checks refusal and rollback, then uses ordinary
tracking open before exercising pending/ACK preservation and reconciliation.
Production identity selection is unchanged.

Filtered author: `cleanroom/20261114T235235Z-25609`.
Independent review: `20261114T235809Z-28431`, no P0–P2 findings.
`native-durability/20261115T010504Z-94770`: all 311 passed; zero failed or skipped.

After that execution, two fixed diagnostic label values were renamed in five
literals to satisfy the unchanged privacy guard. All other bytes remain equal
to the tested export; loop order, branches, mutations and assertions are equal.
Filtered label correction: `20261115T010931Z-96485`; independent check
`20261115T011214Z-99654`, no P0–P2 findings. The 311-case run precedes this
label-only change; final-source integration verification remains required.
The final export passed privacy preflight `20261115T011105Z-98788`.

This focused repair does not accept U113, P24, a milestone or the release.
The original full gate remains incomplete and must run from the recorded base.
