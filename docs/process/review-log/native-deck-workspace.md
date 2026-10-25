# Native deck workspace

Start: `0616f7651906b7daef35d66ca6097791adbe4f84`.
Integration base: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
No unit acceptance is claimed.

Antoine rejected the production Mac sidebar and deck detail screenshots.
The rows overrode system surfaces and text colors. Long names wrapped; the
previous-session block had a separate alignment from the study content.

Fixer session `s-20261024T174115Z-511e3ad6` exported the two-file change in
`cleanroom/20261025T073132Z-30736`. Follow-up
`cleanroom/20261025T073601Z-36151` adds the saved-session deck-ID guard.
Both runs passed their isolation probes. Only the cumulative second patch
was applied. Mac rows retain full accessibility labels and hover help while
using one visual line. Touch layout, review guards and saved data are unchanged.

This is a layout repair. Existing navigation and saved-answer journeys remain
the behavior checks; no new test mirrors the styling modifiers. Native renders
must exercise the production views with the public local-review fixture.
The existing sample overview is insufficient evidence for this layout.

Lint `073708Z-38161`, privacy `073752Z-39875` and similarity
`073820Z-41959` passed. Native drive `073750Z-39500` failed compilation:
`HomeSidebarDecks.body` exceeded Swift's type-checking budget. No UI steps ran.
The source is retained as WIP for a focused compiler repair.
Independent source/visual review and the full integration gate are pending.
Human visual and baseline approval remain pending.
