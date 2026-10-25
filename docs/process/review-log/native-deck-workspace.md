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
The source was retained as WIP `9b74443e` for a focused compiler repair.
Filtered fix `cleanroom/20261025T074220Z-46642` separates the row's due count
and localized accessibility value into typed values. Independent source review
found no P0–P2 in the final change.

Review also found the explicit sidebar-style modifier unnecessary and in
conflict with the outer flat-render style. Integration dropped that hunk,
restoring SidebarView exactly to the starting source. Chrome is unchanged.
Live navigation keeps automatic system styling; flat snapshots keep their
existing inset style. The opaque deck-row background removal remains.

Native drive `074404Z-48610` passed all 14 steps with no hierarchy issue.
The public fixture shows system sidebar surfaces and an enabled Study action
after selection at default and minimum sizes. Visual review found the count
row spread across the readable width. Keep that layout finding open for a
content-sized count grid before the full integration gate.
Human visual and baseline approval remain pending.
