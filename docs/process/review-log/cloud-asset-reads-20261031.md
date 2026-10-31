# Collection payload recovery checkpoint

Implementation starts at `332cfb29e29bf51aec54e92f6f8934dfa29da6e7`.
The original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The filtered implementation adds explicit record reads for payload recovery.
Ordinary duplicate fetches retain their existing version rule. Each pass pins
entry identity and original authority. Accepted work retains terminal ownership;
failed accounting prevents further uncharged admission. File preparation and
cleanup run outside the engine and tracking guards.

Independent review required repairs for accounting failures, reused inbox IDs,
authority transitions and terminal cleanup. The corrected exports are clear of
required findings. Compiler repairs add missing imports and obtain the test
zone from the actual retained runtime binding.

`core-test/20261031T204408Z-46738` and `20261031T204541Z-47835` fail compilation.
After those repairs, `core-test/20261031T204816Z-48998` runs 46 tests:
38 pass and eight fail. Six failures lose the child connection during a later
asset update. Two concern duplicate progress and the retry count after resume.
This failed run stays in the evidence. Unit acceptance remains pending.

Orchestration now reads publication control state through a small snapshot.
Full diagnostic APIs keep their original fields and limits.
The tests now consume the original paused operation before duplicate delivery
and observe the retry reset at reopening before delivering stale results.
`core-test/20261031T210042Z-54617` passes all 46 cases.
`private/20261031T210038Z-54359` passes the revised staged source.
Review finds no P0 or P1 issue. One P2 concerns oversized inbox replies.
The diagnostic now hashes each complete stored value.

`core-test/20261031T210225Z-55546` passes both the header-only coverage check
and eight 40 MiB lost-revision recovery under the unchanged heap bound.
The streaming digest repair removes complete values from inbox replies.
Independent review cleared its corrected fixture and diagnostic-only follow-up.

- `core-test/20261031T210756Z-58243`: 46 pass; the added size case fails.
- `core-test/20261031T211203Z-60613`: failure at index 5, phase `drive_budget`.
  All assertions stay enabled. The cause still needs verification.
- `private/20261031T211207Z-60892`: pass.
- `core-lint/20261031T211309Z-61313`: fail; the parent digest trait import
  makes an existing child-module import unused. No lint suppression was added.

Keep the two technical failures open. The next repair must preserve message,
payload and heap limits and native callback ownership.

Native execution, complete media and device proof, structural generations and
the required integration gate remain outstanding. This is WIP; U113 and U114
remain unaccepted, without Unit trailers.
