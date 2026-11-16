# Compact tab observation — 2026-11-16

Source: clean `f5fe5466e2c47ce3f512fa4e43a5d873d574afe0`.
Original failure: `ui-test/20261116T035027Z-83215` reported four empty native
tab-button identifiers. Labels and order were correct.

Before changing the view, `ui-tree/20261116T093831Z-47191` captured the actual
iPhone simulator hierarchy in the dedicated VM. The four native buttons have
the expected Decks, Browse, Stats and Settings identifiers. No misplaced copy
of those identifiers was needed to explain the capture. The app screenshot was
also inspected. Its body is the test route's Home stand-in; the capture proves
the native tab arrangement and identifiers, not a complete app workflow.

The first capture, `20261116T093623Z-38914`, showed a hook error because the
diagnostic command duplicated the driver's animation option. It supplies no
navigation evidence. The corrected command removed that duplicate.

The original test then ran once as a diagnostic check of the new observation:
`ui-test/20261116T094127Z-64628`, one executed pass and zero skips on clean
`f5fe5466`. Neither `CompactRootView.swift` nor `NavigationLayoutTests.swift`
changed between the retained failing source and this check.

The earlier failure's cause remains unproven. No UI or timing change was made.
This selected pass grants no full-gate pass or design acceptance. Keep the
original failure and include the case in the next required gate.
