# Normal-app review checkpoint

Source: `f21d635532401a10a4a0bf1eb080fb27ae835249`, clean master. The pending integration gate keeps base `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Diagnostic feedback only: no Unit, signed-lane, milestone or release acceptance.

The normal signed Mac Release preview built successfully in `build-app/20261115T225622Z-73821`. The agent opened the exact product, checked its executable hash and process, and inspected only that app through public Accessibility and scoped window capture. No fixture route, replacement collection or debug launch override was used. The existing generated test collection stayed in place. A stale native termination observer initially reported a timeout; a direct process check confirmed normal exit. No force quit was used.

Observed on this build:

- The deck retained 3 new, 0 learning and 0 due cards after the initial restart.
- The question and answer rendered. The generated PNG decoded visibly; an audio Play control appeared. This is not proof of audible playback.
- A Good answer advanced to the next card. The enabled native Undo Answer command returned to the original card. Revealing and grading that card again, then closing the session, left 1 answer recorded, 2 new, 1 learning and 0 due.
- Normal Sync Now actions updated the displayed last-sync time. The overall state stayed Unknown, consistent with incomplete attention producers. A timestamp is not convergence proof.
- Normal quit and cold launch created a new process of the same binary. The saved deck counts and 1 answer recorded matched exactly.

The current signed iPhone Debug app also built, installed and launched normally: `run/20261115T231044Z-86553`, clean source above. Its installation receipt explicitly leaves preservation unproved.

After the owner's permission and unlock changes and the current normal installation, one fresh preserving inspection was attempted: `drive/20261115T231509Z-91895`. The runner timed out enabling automation mode before executing the script. It attached no drive log or current phone snapshot and could not validate continuity. This is failed test setup, not a passed peer check or a diagnosed app failure. No unchanged inspection retry followed.

Device identifiers and detailed native evidence remain in ignored local receipts. Actual Mac-to-phone review convergence, the reverse review, phone cold retention and current bidirectional media checks remain open. The normal preview remains open for feedback; automated integration verification continues separately.
