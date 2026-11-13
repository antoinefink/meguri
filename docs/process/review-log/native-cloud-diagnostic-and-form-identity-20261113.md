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
was below the viewport. Actual phone scrolling with this change remains pending.
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
The phone detected the replacement; adoption has not been confirmed. The live
UI failures and earlier native ownership failures remain. A new review on both
peers, audible playback, full integration and release acceptance remain unproved.
No Unit or P24 closure is claimed. Device evidence stays in ignored build files.
