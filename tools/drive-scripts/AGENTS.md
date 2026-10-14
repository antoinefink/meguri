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
- `"transition": "<name>"` (lowercase letters, digits, `-`) on a step marks it for motion capture. With `--record` the run gets a contact sheet `motion/<NN>-<name>.png` of the frames that change around that step; a sheet that cannot be made makes the drive an error.
- What `--record` captures: simulators, `simctl io recordVideo` around the whole drive. The Mac with Screen Recording (user step `screen-recording`), `screencapture -v` of the main display, cut to the app's window per marked step; the full-screen movie is deleted. The Mac without it, `FrameSampler` screenshots at about 10 per second from the start of the marked action to 1.5 s after it (`frames/`); marked launch, resize and snapshot steps are not sampled (`unsampled_transitions` in the manifest). Motion checks (U252) on the Mac work from that rate until Screen Recording is granted; the screen-recording path has not run on a Mac with the permission yet.
- Mac drives with a `tap`, `type`, `swipe` or `scroll` end `blocked: mac-input` while macOS drops XCUITest's synthesized input (macOS 27.0.1; `docs/process/one-time-setup.md`). Their failed steps then say nothing about the app.
- `platforms` is read by `tools/mg mac-ui`: it runs every example listing `mac` in the hourly Mac slot, with `--record` when the script marks a transition. Mac drives always go through the Mac UI gate.
- Examples must pass on every platform they list: `hierarchy-check` issues fail a drive.
