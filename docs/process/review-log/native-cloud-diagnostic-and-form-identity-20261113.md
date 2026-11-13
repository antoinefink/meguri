# Native cloud diagnostics and form identity

Starting commit: `95ed9f44f979510c8b09996574a7dabe7f51c36b`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Role: fixer; session `s-20261110T214025Z-20fa15fd`.
Authority: sync/policy P21 and the native screen contracts for sync.connect,
sync.publicationChoice, sync.replaceLocal and sync.replaceCloud.

DEBUG diagnostics distinguish six public source messages with constant tags.
Every other message keeps the generic classification. Coverage includes each
literal and variants with extra or changed text. Existing tests are preserved.
Runtime decisions, account handling and media operations are unchanged.

The existing native Form gains a page-qualified accessibility identifier:
`sync.connection.form.<page>`. It retains the standard Form and its children.
The observed phone preview had no content scrolling identifier, and its choice
was below the viewport. The signed phone now scrolls that Form successfully.
No test that merely repeats the modifier was added.

Filtered authors: `cleanroom/20261113T141926Z-53390` and
`cleanroom/20261113T142406Z-58924`. Independent review:
`cleanroom/20261113T142609Z-59625`, no P0, P1 or P2 findings.
Review covers the three-file patch, privacy, contracts and native Form semantics;
Root owns exact-product rendered checks.

Evidence:

- New test against old production: `swift-test/20261113T142802Z-61239`,
  expected failure, 1 selected and 1 failed. Retained.
- Full diagnostic class: `swift-test/20261113T142856Z-62368`, 31/31 pass.
- Existing sync presentation tests and Kit compilation:
  `swift-test/20261113T142932Z-63835`, 8/8 pass.
- Static checks: `static-checks/20261113T142933Z-64396`, 3/3 pass.
- Source privacy: `private/20261113T142933Z-63959`, all 3 files pass.
- Similarity: `similarity/20261113T142933Z-64204`, all 3 files pass, no exceptions.

Before this patch, the signed Mac imported the existing four-card media fixture,
recorded one answer, undid it and redid it. Its image rendered. Publication
reached an active binding, but the next exchange exposed the generic error.
The phone subsequently adopted the collection and received its first review.
Two further phone answers reached the Mac. The three saved review rows survived
a normal Mac quit and reopen. Both devices show one new card remaining.
The phone cold launch and subsequent review drive passed without audit issues.
Earlier media and replacement drives retain their audit failures.
No Unit or P24 closure is claimed. Device evidence stays in ignored build files.

## Native follow-up

- Signed products at `04a54be4`: Mac build `70750`; phone normal run `72072`.
- Form scroll and preview: `drive/20261113T143537Z-73927`, all 14 actions
  succeeded; overall failed its accessibility audit. Explicit adoption:
  `drive/20261113T143904Z-77393`, all 6 actions succeeded; audit remained red.
- The actual image rendered in `drive/20261113T144216Z-83717`. That run and
  `drive/20261113T144504Z-89272` retain missing audio-link identifier findings.
  The latter completed its 11 grade, Undo and send actions.
- Normal phone cold launch: `run/20261113T150014Z-561`, pass.
  Saved schedule inspection: `drive/20261113T150143Z-2113`, 2/2 pass, no issues.
  Another answer and send: `drive/20261113T150324Z-3706`, 12/12 pass, no issues.

Read-only comparisons of the actual Mac database preserve the original review
row and show both additional phone rows. All three remain after normal restart.
These local comparisons are operational evidence, not integrity gates.

Incoming delivery occurred after native activation. A completed manual exchange
while the Mac was inactive delivered no new collection callbacks. The native
Window command then activated the exact preview and the additional review
arrived. Foreground state was checked separately; an open window alone proved
insufficient. Background receipt and the earlier warm publication error remain
open. Audible playback, fresh asset transfer, full integration and release
acceptance remain unproved. Existing assets were already available on both peers.
