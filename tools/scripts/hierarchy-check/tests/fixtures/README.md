Real exports from `AgentDriverTests` on an iPhone 17 Pro simulator (iOS 27.0), captured with `capture.json`:

    tools/mg drive tools/scripts/hierarchy-check/tests/fixtures/capture.json --device <udid>

The capture fails because `sample/defects` holds deliberate defects. Copy `steps/03-snapshot-sample-defects.json` to `ios-sample-defects.json` and `steps/06-snapshot-sample-sheet.json` to `ios-sample-sheet.json`. Recapture them when the export format or the sample routes change.

`mac-sample-screen.json`, `mac-sample-sheet.json` and `mac-sample-alert.json` are the snapshots of `tools/drive-scripts/examples/sample-routes.json` from `tools/mg mac-ui` (macOS 27.0). Exports taken before the agent driver dropped the menu bar had it removed by hand: its Apple menu lists the user's recent files.
