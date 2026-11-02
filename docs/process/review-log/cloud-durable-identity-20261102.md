# CloudKit durable collection identity

Status: partial U113; no Unit or milestone acceptance.
Component start: 0a5806f844ad4ef5e3d86d72e6c57e7ace1151e4.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer session: s-20261102T225401Z-cb28dce4.
Spec-writer session: s-20261102T213243Z-4ad02362.

An independent source audit finds that ordinary active-binding reopen compares
the saved device number with the current one. If a remount changes that number,
the unchanged file is isolated. V8.2 currently prescribes this comparison, so a
contract correction must precede implementation. No remount was performed.

Keep all live replacement guards. Use the retained writer descriptor for a
separate stable Darwin identity, with exact birthtime and strict legacy rules.
The first independent contract review finds three P2 issues: an older-reader
fence, invalidation of previous-open scan proof, and pairing the captured file
with the live birthtime before bind. The second review clears all three after
the contract changes. Capture failure blocks CloudKit while local reviews
remain available. Privacy17976 passes both amended specification files.

Live Mac verification awaits an unlocked session. The saved collection stays
intact. Signed SDK evidence and the batch gate are outstanding. This component
changes neither Swift nor the U114 repair limit.
