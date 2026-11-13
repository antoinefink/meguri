# Normal cold-peer review check

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Source: `3973eb519646b94c926aa94e3feee22a5c8ebd21`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The generated collection has four notes and four cards. The Mac and phone had three reviews before this check. Normal phone drive `20261113T180526Z-74077` passes both actions with no hierarchy issue. It uses no launch override and closes the app at teardown.

With the phone app closed, the Mac displays the final question and answer. An actual Easy grade finishes the session. Read-only local storage then has four reviews and total repetitions four. Native transport reports three collection records sent, no pending, held-back, in-flight or incoming-waiting work. A later manual collection exchange succeeds. Its retained media read fault is separate and does not prove media completion.

Normal phone launch `run/20261113T181012Z-79176` passes with normal launch verified. Preservation drive `20261113T181138Z-80906` passes both actions with verified process continuity and no hierarchy issue. Its first observation reports `0 new · 0 learning · 0 due`. No manual phone Sync Now or Home-and-return action occurs before that observation.

Read-only count inspection `drive/20261113T181820Z-86817` completes all thirteen actions, including Cancel and return to Decks. Its This Device section displays four notes, four cards and four reviews. This matches the Mac. The overall receipt fails with 102 hierarchy issues, which remain open; successful actions are not a passing audit. The first cold screenshot has incomplete pixel labels, so its AX count and the later visible local count are retained separately.

The older route `drive/20261113T181558Z-84500` fails at the Settings landing page. The corrected route follows the observed Account & Sync link. Neither run submits a collection replacement. The final preview is cancelled. No Unit, signed lane, milestone gate or release acceptance follows from this normal-app diagnostic.

## Fresh media transfer

Normal Release development preview `build-app/20261113T182123Z-89634` passes on clean source `3973eb51`, with signing/provisioning verified and test hooks and fixture scope disabled. The Debug app quits normally before this product opens. All four saved reviews remain.

The Debug-only attempt `cloud-media-probe/20261113T181142Z-81180` refuses that unsupported product before staging. The unchanged probe then admits the normal Release product. Stage `cloud-media-probe/20261113T182608Z-94205` creates one fresh 10,240-byte synthetic file with transfer unverified. A native Mac Sync Now media cycle completes.

Phone drive `20261113T182815Z-94727` performs all five normal actions, including Sync Now. Four hierarchy issues retain its failed overall status. No Home-and-return action or copy-to-device command is used.

Read-back `cloud-media-probe/20261113T182949Z-96284` passes with `peer-byte-match`, `transfer_verified: true`, size 10,240 and SHA-256 `ce135fa356aa1cb2c23fc857040591d6f3fc172a723ca705a5b6f63d161b775d`. It reads only the staged filename from the originally prepared normal phone app. This proves this fresh file's actual arrival, beyond a sync timestamp or local write. Signed-lane acceptance stays false; it proves neither audible playback nor every media/error path. No network, SDK-scope or production rule was changed for this transfer.
