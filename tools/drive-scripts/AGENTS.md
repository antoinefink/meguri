# tools/drive-scripts/

Drive scripts for `tools/mg drive <script> [--platform iphone|iphone-small|ipad|ipad-small|foldable|mac] [--device <udid>] [--record]`, run by `app/MeguriUITests/AgentDriver/AgentDriverTests.swift` (parser: `DriveScript.swift`, which rejects anything not listed here).

```json
{"name": "open-sample-sheet", "description": "...", "platforms": ["ios", "mac"],
 "steps": [{"launch": {"route": "sample/sheet", "animations": false}}, {"tap": "sample.sheet.done", "transition": "dismiss-sheet"}]}
```

- The first step is `launch`. Its keys are the U7 launch options (`app/Meguri/TestHooks/LaunchOptions.swift`): `fixture` (relative to the repo root), `now`, `timeZone`, `locale`, `animations`, `reduceMotion`, `reduceTransparency`, `windowSize`, `ankiConnectPort`, `migakuPort`, `route`, `arguments` (raw extras). A later `launch` relaunches.
- Actions, one per step: `tap: <id>`, `type: {id, text}`, `swipe: {id, direction}` (the finger's direction), `scroll: {id, direction}` (the content's), `wait: <id>` or `{id, timeout, gone}`, `snapshot: <name>`, `resize: minimum|default|large|<w>x<h>`.
- Elements are found only by accessibility identifier, and an identifier must name exactly one element; taps go to the hit point the hierarchy export gives.
- `resize` relaunches the Mac app with the last launch options at the new size. On simulators it is reported `unavailable` (not a failure).
- `"transition": "<name>"` on a step marks it for motion capture. With `--record` the run gets a contact sheet of the frames that change around that step.
- `platforms` is read by `tools/mg mac-ui`: it runs every example listing `mac` in the hourly Mac slot. Mac drives always go through the Mac UI gate.
- Examples must pass on every platform they list: `hierarchy-check` issues fail a drive.
