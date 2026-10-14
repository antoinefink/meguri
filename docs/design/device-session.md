# Monthly device session

About once a month Antoine spends a short session on his own devices judging what simulators cannot show: feel, motion, glass and haptics (R1990). Agents prepare the session and record its outcome; they never hold it or write its result without Antoine's report.

## Before the session (agent)

- Build the current integration branch for his iPhone, iPad and Mac (personal build, his collection).
- List what changed since the last session: screens that moved to verified or approved in `screen-status.yaml`, new gallery approvals, and the snapshot cells marked `deferred` (device-only: tinted glass, glass opacity on the Mac, foldable layouts) that the session can look at.
- Put the list, with the screens to visit in order, in the session file's `plan` (below).

## The checklist (Antoine, about 20 minutes)

For each signature screen (Home and deck list, deck page, review question and answer, browser, editor, deck options, stats) on each device:

1. **Feel.** Does it respond at once? Scrolling, tapping a deck, answering a card: any wait, stutter or dropped frame?
2. **Motion.** Card reveal, answer to next card, undo, sheets and the sidebar: is each transition continuous, quick and calm? With Reduce Motion on, does it cross-fade instead of moving?
3. **Glass.** Toolbars, the answer bar and floating controls: legible over busy content while scrolling, with both glass looks (clear and tinted) and the transparency setting at both ends? Any glass on content, or glass on glass?
4. **Haptics.** Where the app gives haptic feedback (answer buttons, destructive confirmations), is it present, light and consistent?
5. **Anything else** that looks or feels wrong.

The deferred snapshot cells are checked here too: switch the device to tinted glass, the opacity extremes or the foldable posture and look at the screens the plan names.

## Recording it (agent, from Antoine's report)

One file per session, `docs/process/milestone-records/device-sessions/<YYYY-MM>.json` (U254 operates it):

```json
{
  "date": "2026-11-03",
  "devices": ["iPhone 17 Pro, iOS 27.1", "iPad Pro 13-inch, iPadOS 27.1", "MacBook Pro, macOS 27.1"],
  "build": "<commit>",
  "plan": ["home.deckList", "review.answer"],
  "findings": [
    {"screen": "review.answer", "device": "iPhone", "area": "motion", "note": "Next card slides in a little late after Good", "severity": "medium"}
  ],
  "deferred_cells_checked": ["home.deckList/typical/light-standard-iphoneLarge+tinted"],
  "reported_by": "antoine"
}
```

Each finding becomes a task in `screen-status.yaml` (medium) or keeps the screen out of verified (high), as critic issues do. A U256 milestone device run's feel checklist counts as that month's session.
