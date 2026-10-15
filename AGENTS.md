# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.

## Roles (clean room)

- Every agent session has one role; read `docs/process/roles/<role>.md` first. Implementers and fixers work only in worktrees from `tools/mg worktree new` and never read `oracle/`, `refs/`, `local-data/` or any Anki source (R1955). Trailers and the similarity gate: `docs/process/provenance.md`.

## Tooling

- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds run on the host; simulators and Mac UI suites run in the macOS test VM.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.

## Agent docs

- Add or update an `AGENTS.md` in a subdirectory when it has conventions or traps the code doesn't make obvious.
- Keep every `AGENTS.md` terse. Readers are highly capable agents: write only what they can't infer. No restating code, no tutorials, no obvious advice.

## Running tests and apps on this Mac

The Mac is also Antoine's workstation. Unattended runs must never take over the screen. Everything that would disturb it runs in the macOS test VM (`docs/process/mac-ui-vm.md`), unthrottled: run it as soon and as often as useful, with the Mac locked or not.
- Simulators live in the VM, in a pool: `tools/mg sim acquire <iphone|ipad>` (UDID at `.details.udid`), or `tools/scripts/sim-pool.sh acquire <kind>` inside scripts (it runs itself in the VM). Release them when done (`release <udid>`); a lease expires after 60 min unless renewed. Reach them only through `tools/mg` jobs (`run`, `drive`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`), never with `xcrun simctl` on this Mac. Never create, erase, delete, shut down or reboot a simulator, and never open Simulator.app.
- Nothing drives this Mac's cursor or keyboard. Mac UI-driving suites (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) run only in the VM. Never run macOS XCUITest directly.
- Without the VM (T9 drive disconnected), simulator jobs and Mac UI suites report queued (user step `mac-vm`): record the verification as queued, continue, and run them again once `tools/mg doctor` shows the VM.
- Check Mac screens with in-process snapshot tests, which render off-screen. When a running Mac app is unavoidable, launch it hidden and in the background (`open -g -j`), capture its window with `screencapture -l <windowid>`, and quit it afterwards.
