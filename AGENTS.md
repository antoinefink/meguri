# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`. Dated decisions and exceptions: `docs/process/standing-decisions.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.
- Every agent session has one role; read `docs/process/roles/<role>.md` first. Implementers and fixers never read `oracle/`, `refs/`, `local-data/` or any Anki source (R1955). Trailers and the similarity gate: `docs/process/provenance.md`.
- Routine ledger and public-file privacy admissions follow `docs/process/provenance.md#routine-privacy-admissions`; independent complete-content review and exact hashes remain required.
- No AnkiWeb contact. The legacy Anki server UI stays hidden and its work frozen; keep its code and tests.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.
- Never force-push.
- Nothing drives this Mac's cursor or keyboard, and unattended runs never take over its screen.

## What comes first

- Usable daily reviews with our own Meguri Sync server come first (`docs/process/review-first-delivery.md`, `specs/sync/meguri-provider.md`).
- iCloud Sync work is paused; bug fixes only. Its screens still meet the design standard. Parked items: `docs/process/verification-status.md`.
- Meguri should look and behave like an app designed by Apple: native controls and platform defaults; customize only for a concrete functional, compatibility or verified accessibility need, and document the exception. Preserve user-authored study-card styling. Authority: `docs/design/native-interface-contract.md`. UI work starts with `docs/design/workflow.md` and the screen contract; look at the built app, a passing build is not design agreement.

## How to work

- Use a task branch and worktree (`tools/mg worktree new`). Open a pull request against `master` early and ready for review, so GitHub Actions runs the Linux suites and the AI reviewer checks it while you continue. Integration on `master` is for authorized changes: `docs/process/merge.md`.
- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds run on the host; simulators and Mac UI suites run in the macOS test VM.
- Run locally the smallest checks that cover the change, plus what CI cannot do: Apple builds, Swift suites, VM jobs. Leave the broad Linux suites to GitHub Actions. Full gates belong to batch and milestone boundaries (`docs/process/tiers.md`).
- A failure on a loaded machine is inconclusive until that test has been re-run alone. Never weaken an assertion or add a retry to get green.
- Each unit gets an independent review, its own commits, and the provenance, privacy and similarity checks.
- Read `docs/process/verification-status.md` on resume. Repair required failures first, report inherited failures on the pull request, and keep the register current.
- Add an `AGENTS.md` in a subdirectory for conventions or traps the code doesn't make obvious. Keep every `AGENTS.md` terse: readers are highly capable agents, so write only what they can't infer.

## Running tests and apps on this Mac

This Mac is also Antoine's workstation. Anything that would disturb it runs in the macOS test VM (`docs/process/mac-ui-vm.md`), with the Mac locked or not.

- Simulators live in the VM's pool: `tools/mg sim acquire <kind>`, release when done. Reach them only through `tools/mg` jobs. Never use `xcrun simctl` on this Mac, never create, erase, delete, shut down or reboot a simulator, and never open Simulator.app. iOS simulators may run on this Mac through MG when the VM slows work.
- The VM is small: run one simulator or UI job in it at a time.
- Mac UI suites, app-hosted tests, snapshot renders and launch probes run only in the VM. Never run macOS XCUITest directly. When a running Mac app is unavoidable, launch it hidden in the background as `tools/mg run` does.
- A run that reports queued (`mac-vm`, `host-session`, `mac-input`) is recorded as queued and never blocks a merge; `tools/mg catch-up` runs it again later.
- The orchestrator may restart `meguri-mac` when it stops answering: stop only that VM once its jobs have ended, start it headless, then check `tools/mg doctor`.
- On a migrated Mac, source `.local/activate` before running jobs (`docs/process/new-machine.md`).
- "Meguri" (daily-driver ids, red icon) is Antoine's real copy; "Meguri Dev" (`.dev` ids, blue icon) is the test copy. Agents never launch, read or modify the real copy, nor read or modify the data of any copy on his devices. They build the real copy only on his explicit request or inside `tools/mg xcode-check`. Only the coordinator installs or replaces it, at his request, through the owner-requested routes (`app/AGENTS.md`). App launches and UI runs follow the VM rules above.
