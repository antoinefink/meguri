# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.

## Roles (clean room)

- Every agent session has one role; read `docs/process/roles/<role>.md` first. Implementers and fixers never read `oracle/`, `refs/`, `local-data/` or any Anki source (R1955). Trailers and the similarity gate: `docs/process/provenance.md`.

## Tooling

- Work directly on `master` in the main checkout; do not create branches or worktrees. Resume prepared work without redoing it. Record each unit's starting commit and review/fix log. Commit WIP with co-author attribution; push only `master`, never force. `tools/mg merge` is not required (Antoine, 2026-10-18).
- WIP verification (Antoine, 2026-10-20): per unit, run focused tests, affected-platform builds, independent review, and provenance/privacy/similarity checks. Resolve known failures before acceptance. Run full gates at milestones, app handoffs and after shared build/test tooling changes. Final suites and required human evidence remain mandatory. See `docs/process/tiers.md`.

- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds run on the host; simulators and Mac UI suites run in the macOS test VM.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.

## Agent docs

- Add or update an `AGENTS.md` in a subdirectory when it has conventions or traps the code doesn't make obvious.
- Keep every `AGENTS.md` terse. Readers are highly capable agents: write only what they can't infer. No restating code, no tutorials, no obvious advice.

## Running tests and apps on this Mac

The Mac is also Antoine's workstation. Unattended runs must never take over the screen. Everything that would disturb it runs in the macOS test VM (`docs/process/mac-ui-vm.md`), unthrottled: run it as soon and as often as useful, with the Mac locked or not.
- Simulators live in the VM, in a pool: `tools/mg sim acquire <iphone|ipad>` (UDID at `.details.udid`), or `tools/scripts/sim-pool.sh acquire <kind>` inside scripts (it runs itself in the VM). Release them when done (`release <udid>`); a lease expires after 60 min unless renewed. Reach them only through `tools/mg` jobs (`run`, `drive`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`), never with `xcrun simctl` on this Mac. Never create, erase, delete, shut down or reboot a simulator, and never open Simulator.app.
- Nothing drives this Mac's cursor or keyboard. Mac UI-driving suites (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) run only in the VM. Never run macOS XCUITest directly.
- Without the VM (T9 drive disconnected), or while it does not answer or is near its process limit, simulator jobs and Mac UI suites report queued (user step `mac-vm`), and so do runs blocked by a locked Mac (`host-session`) or by macOS dropping XCUITest's input (`mac-input`): record the verification as queued and continue. A queued run never blocks a merge; `tools/mg catch-up` runs it again on the integration branch once `tools/mg doctor` reports its step clear (`docs/process/merge.md`).
- Mac app-hosted tests, snapshot renders, fixture launch checks and Xcode launch probes run in the VM. Host builds need no unlocked login session. Check Mac screens with in-process snapshot tests, which render off-screen in the VM. When a running Mac app is unavoidable, launch the Debug build hidden and in the background with `open -g -j -n <app> --args -MeguriBackgroundLaunch YES` (as `tools/mg run` and `screenshot` do: no Dock tile, never active), capture its window with `screencapture -l <windowid>`, and quit it afterwards.
