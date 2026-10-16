Real exports from `AgentDriverTests` on an iPhone 17 Pro simulator (iOS 27.0), captured with `capture.json`:

    tools/mg drive tools/scripts/hierarchy-check/tests/fixtures/capture.json --device <udid>

The capture fails because `sample/defects` holds deliberate defects. Copy `steps/03-snapshot-sample-defects.json` to `ios-sample-defects.json` and `steps/06-snapshot-sample-sheet.json` to `ios-sample-sheet.json`. Recapture them when the export format or the sample routes change.

`mac-sample-screen.json`, `mac-sample-sheet.json` and `mac-sample-alert.json` are the snapshots of `tools/drive-scripts/examples/sample-routes.json` from `tools/mg mac-ui` (macOS 27.0). Exports taken before the agent driver dropped the menu bar had it removed by hand: its Apple menu lists the user's recent files.

`ios-shell-typical.json` and `ipad-shell-typical.json` are `tools/mg ui-tree --route shell/root/typical` on the iPhone and iPad pool simulators (iOS 27.0): the tab layout and the sidebar layout with the sample decks. Recapture them when the shell's structure changes.

`mac-shell-deck-page.json` is step `11-snapshot-deck-page-dark` of a `tools/mg drive --platform mac` run (macOS 27.0, test VM) of the shell's routes: launch `shell/root/deck-page` with `-MeguriAppearance dark`, wait for `testhooks.shell.deckPage.title`, snapshot. It holds an AppKit scroller and lazy rows that hang past the scroll view into the state probe's strip.
