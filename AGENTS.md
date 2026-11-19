# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.

## Roles (clean room)

- Every agent session has one role; read `docs/process/roles/<role>.md` first. Implementers and fixers never read `oracle/`, `refs/`, `local-data/` or any Anki source (R1955). Trailers and the similarity gate: `docs/process/provenance.md`.

- Routine ledger and public-file privacy admissions follow `docs/process/provenance.md#routine-privacy-admissions`; independent complete-content review and exact hashes remain required.

## Tooling

- On a migrated Mac, source `.local/activate` when present before running jobs. Machine-local VM storage and tool setup: `docs/process/new-machine.md`.
- Prefer an isolated task branch and worktree for feature work. Reuse prepared work and choose the checkout that fits the task. A request for a PR authorizes pushing its task branch and opening or updating the PR against `master`. Reserve integration on `master` for authorized changes. Never force-push. See `docs/process/merge.md`.
- Antoine (2026-10-22): implement working reviews next. Follow `docs/process/review-first-delivery.md` for execution order. Keep all milestone gates and complete the full plan.

- Antoine (2026-11-17): prioritize usable daily reviews with our own Meguri Sync server for integration. Independent work may proceed in an isolated worktree and reach draft PR review while repairs continue.

- Verification (updated goal, 2026-10-25): keep focused checks, independent review, separate commits and provenance/privacy/similarity checks per unit. Group 3–5 related units in dependency order, including Swift/FFI, with one full gate at the coherent batch boundary. Pure Rust internals may cover at most 20 units per full gate. Always gate milestone boundaries; record the original batch base and resolve technical failures before the next batch. See `docs/process/tiers.md`.

- For each edit, run the smallest `tools/mg` checks that cover the change. After an authorized push, use GitHub Actions for the broader public Linux suites it covers. Reserve full local suites for required gates or failures that focused checks cannot diagnose. Use each hosted receipt only for its verified scope in `docs/process/tiers.md`.

- Failure discipline: read `docs/process/verification-status.md` on resume and before a batch. Keep required repairs first in the integration order. Report inherited failures on affected draft PRs and use full gate evidence for integration. Maintain the register under `docs/process/merge.md`.

- Antoine (2026-11-14): continue as many repair/review rounds as needed for this goal; do not ask for round-count extensions. Keep independent review, failure evidence and all required checks. See `docs/process/review-log/repair-rounds-authorization-20261114.md`.

- Cosmetic code lint may be relaxed to avoid delaying delivery (Antoine, 2026-11-12). Retain checks for correctness, safety, privacy, layering, accessibility and native interface requirements.

- Interim exception (Antoine, 2026-10-21): only the recorded R554 audio failure may remain red until U69, with every other required check passing. Preserve the failed gate, enabled test and limits; obtain skipped T1 separately. M1R and final acceptance still require the fix. Exact scope: `docs/process/tiers.md`, approval `E-20261021T134124Z-a07a`.

- Remaining VoiceOver cursor and speech work is optional follow-up after the app goal (Antoine, 2026-10-20). Preserve its tests and failure evidence; it does not block units or milestones. Keyboard ownership, system accessibility audits, labels, contrast and hit targets remain required. Independent Rust work may proceed while U70 awaits its other checks.

- Manual device and reference checks are deferred until the final session (Antoine, 2026-10-20). Continue with automated VM tests. Final evidence remains required.

- Temporary macOS-first pass (Antoine, 2026-10-25): use Xcode MCP on this Mac for Meguri build/run/debug and UI feedback. Defer VM and mobile iteration checks during this pass; retain their required integration and final evidence. This explicitly overrides the VM-only and MG-only rules for that local feedback loop, not clean-room or privacy rules.

- Antoine (2026-11-16): if the VM slows local work, iOS simulators may run on this Mac through MG. Keep the required VM and signed acceptance checks.

- Antoine (2026-11-16): offer iCloud Sync and Meguri Sync using our own server under `specs/sync/meguri-provider.md`. Legacy Anki server UI stays hidden and its work frozen; retain its code/tests. No AnkiWeb contact is authorized. This grants no technical acceptance. See `docs/design/native-interface-contract.md` N12.4.

- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds run on the host; simulators and Mac UI suites run in the macOS test VM.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.

## Agent docs

- **Design objective:** Meguri should look and behave like an app designed by Apple. Use native controls and platform defaults for typography, colors, sizing, spacing and interaction. Customize only for a concrete functional, compatibility or verified accessibility need; document minimal exceptions. Design tokens alone do not justify overrides. Preserve user-authored study-card styling. Authority: `docs/design/native-interface-contract.md`.
- UI work starts with `docs/design/workflow.md` and the active screen contract; a passing build alone is not design agreement. Verify the exact built app when showing a preview.

- Add or update an `AGENTS.md` in a subdirectory when it has conventions or traps the code doesn't make obvious.
- Keep every `AGENTS.md` terse. Readers are highly capable agents: write only what they can't infer. No restating code, no tutorials, no obvious advice.

## Running tests and apps on this Mac

The Mac is also Antoine's workstation. Except for the authorized macOS-first pass above, unattended runs must never take over the screen. Everything that would disturb it runs in the macOS test VM (`docs/process/mac-ui-vm.md`), unthrottled: run it as soon and as often as useful, with the Mac locked or not.
- Simulators live in the VM, in a pool: `tools/mg sim acquire <iphone|ipad>` (UDID at `.details.udid`), or `tools/scripts/sim-pool.sh acquire <kind>` inside scripts (it runs itself in the VM). Release them when done (`release <udid>`); a lease expires after 60 min unless renewed. Reach them only through `tools/mg` jobs (`run`, `drive`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`), never with `xcrun simctl` on this Mac. Never create, erase, delete, shut down or reboot a simulator, and never open Simulator.app.
- Nothing drives this Mac's cursor or keyboard. Mac UI-driving suites (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) run only in the VM. Never run macOS XCUITest directly.
- Antoine (2026-10-22) authorizes the orchestrator to restart `meguri-mac` when it stops answering. Stop only that VM after its jobs end, then start it headlessly. Check `tools/mg doctor` and run `tools/mg catch-up`.
- Without the VM (T9 drive disconnected), or while it does not answer or is near its process limit, simulator jobs and Mac UI suites report queued (user step `mac-vm`), and so do runs blocked by a locked Mac (`host-session`) or by macOS dropping XCUITest's input (`mac-input`): record the verification as queued and continue. A queued run never blocks a merge; `tools/mg catch-up` runs it again on the integration branch once `tools/mg doctor` reports its step clear (`docs/process/merge.md`).
- Mac app-hosted tests, snapshot renders, fixture launch checks and Xcode launch probes run in the VM. Host builds need no unlocked login session. Check Mac screens with in-process snapshot tests, which render off-screen in the VM. When a running Mac app is unavoidable, launch the Debug build hidden and in the background with `open -g -j -n <app> --args -MeguriBackgroundLaunch YES` (as `tools/mg run` and `screenshot` do: no Dock tile, never active), capture its window with `screencapture -l <windowid>`, and quit it afterwards.
