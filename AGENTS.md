# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.

## Roles (clean room)

- Every agent session has one role; read `docs/process/roles/<role>.md` first. Implementers and fixers work only in worktrees from `tools/mg worktree new` and never read `oracle/`, `refs/`, `local-data/` or any Anki source (R1955). Trailers and the similarity gate: `docs/process/provenance.md`.

## Tooling

- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds and simulators run on the host.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.

## Agent docs

- Add or update an `AGENTS.md` in a subdirectory when it has conventions or traps the code doesn't make obvious.
- Keep every `AGENTS.md` terse. Readers are highly capable agents: write only what they can't infer. No restating code, no tutorials, no obvious advice.

## Running tests and apps on this Mac

The Mac is also Antoine's workstation. Unattended runs must never take over the screen.
- Simulators come only from the pool: `tools/mg sim acquire <iphone|ipad>` (UDID at `.details.udid`), or `tools/scripts/sim-pool.sh acquire <kind>` inside scripts. Release them when done (`release <udid>`); a lease expires after 60 min unless renewed. Never create, erase, delete, shut down or reboot a simulator, and never open Simulator.app. Capture with `xcrun simctl io`.
- Never run macOS XCUITest, or anything that drives the real cursor or keyboard, directly. Mac UI-driving suites (accessibility audit, keyboard flows, UI perf) run only through `tools/mg mac-ui`. It holds a global lock and refuses to start within 60 minutes of the previous run's start, so the screen is taken at most once an hour. When it refuses, record the verification as queued and continue. An hourly sweep runs the queued suites on the integration branch.
- Check Mac screens with in-process snapshot tests, which render off-screen. When a running Mac app is unavoidable, launch it hidden and in the background (`open -g -j`), capture its window with `screencapture -l <windowid>`, and quit it afterwards.
