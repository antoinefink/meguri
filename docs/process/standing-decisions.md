# Standing decisions

Dated decisions and exceptions from Antoine that still apply. `AGENTS.md` states the current rule; this page keeps the record, in his dated form, moved here unchanged on 2026-11-20. Add new entries at the end. Remove an entry only when Antoine withdraws it, and say so in the commit.

## Entries moved from AGENTS.md

- Antoine (2026-10-22): implement working reviews next. Follow `docs/process/review-first-delivery.md` for execution order. Keep all milestone gates and complete the full plan.

- Antoine (2026-11-17): prioritize usable daily reviews with our own Meguri Sync server for integration. Independent work may proceed in an isolated worktree and reach draft PR review while repairs continue. Superseded in part 2026-11-20: pull requests open ready for review, not draft (the 2026-11-20 entry below); the priority stands.

- Superseded 2026-11-20 for when full gates run, by `AGENTS.md` "How to work" (run locally the smallest checks that cover the change; the broad Linux suites run on GitHub Actions; full gates at batch and milestone boundaries, `docs/process/tiers.md`). The per-unit review, commit, provenance, privacy and similarity rules stand. Verification (updated goal, 2026-10-25): keep focused checks, independent review, separate commits and provenance/privacy/similarity checks per unit. Group 3–5 related units in dependency order, including Swift/FFI, with one full gate at the coherent batch boundary. Pure Rust internals may cover at most 20 units per full gate. Always gate milestone boundaries; record the original batch base and resolve technical failures before the next batch. See `docs/process/tiers.md`.

- Antoine (2026-11-14): continue as many repair/review rounds as needed for this goal; do not ask for round-count extensions. Keep independent review, failure evidence and all required checks. See `docs/process/review-log/repair-rounds-authorization-20261114.md`.

- Cosmetic code lint may be relaxed to avoid delaying delivery (Antoine, 2026-11-12). Retain checks for correctness, safety, privacy, layering, accessibility and native interface requirements.

- Interim exception (Antoine, 2026-10-21): only the recorded R554 audio failure may remain red until U69, with every other required check passing. Preserve the failed gate, enabled test and limits; obtain skipped T1 separately. M1R and final acceptance still require the fix. Exact scope: `docs/process/tiers.md`, approval `E-20261021T134124Z-a07a`.

- Remaining VoiceOver cursor and speech work is optional follow-up after the app goal (Antoine, 2026-10-20). Preserve its tests and failure evidence; it does not block units or milestones. Keyboard ownership, system accessibility audits, labels, contrast and hit targets remain required. Independent Rust work may proceed while U70 awaits its other checks.

- Manual device and reference checks are deferred until the final session (Antoine, 2026-10-20). Continue with automated VM tests. Final evidence remains required.

- Superseded, no longer in force: `AGENTS.md` now says nothing drives this Mac's cursor or keyboard and Mac UI runs only in the macOS test VM; the pass this entry allowed has ended. Kept as history. Temporary macOS-first pass (Antoine, 2026-10-25): use Xcode MCP on this Mac for Meguri build/run/debug and UI feedback. Defer VM and mobile iteration checks during this pass; retain their required integration and final evidence. This explicitly overrides the VM-only and MG-only rules for that local feedback loop, not clean-room or privacy rules.

- Antoine (2026-11-16): if the VM slows local work, iOS simulators may run on this Mac through MG. Keep the required VM and signed acceptance checks.

- Antoine (2026-11-20): iCloud Sync work is paused; bug fixes only. Its screens still meet the design standard. Meguri Sync comes first. Parked items: `docs/process/verification-status.md`.

- Antoine (2026-11-16): offer iCloud Sync and Meguri Sync using our own server under `specs/sync/meguri-provider.md`. Legacy Anki server UI stays hidden and its work frozen; retain its code/tests. No AnkiWeb contact is authorized. This grants no technical acceptance. See `docs/design/native-interface-contract.md` N12.4.

- Antoine (2026-10-22) authorizes the orchestrator to restart `meguri-mac` when it stops answering. Stop only that VM after its jobs end, then start it headlessly. Check `tools/mg doctor` and run `tools/mg catch-up`.

## Added 2026-11-20

- Antoine (2026-11-20): open pull requests early and ready for review, so GitHub Actions and the AI reviewer run while local work continues; keep local runs to what covers the change. The orchestrating session may merge a pull request once its checks pass and the review is addressed.
- Antoine (2026-11-20): he is the only user, so keep things plain; instructions for agents stay brief and leave out what they can work out.

## Added 2026-11-21

- Antoine (2026-11-21): the three entries above marked superseded were marked at his word ("Go with your recommendation"); the R554 audio exception was left as it is until U69's state is checked.
- Antoine (2026-11-20): Claude (coordinator) may approve and update specs without asking first; each change still gets an independent review and is recorded with that attribution (`docs/design/decisions.md`, rows of 2026-11-20).
- Claude (coordinator), under Antoine's delegation (2026-11-21, in the session where he asked for the real copy for his daily reviews with Meguri Sync): exception to KTD22 for the owner-requested real copy only, on the Mac (`tools/mg build-app --platform mac --real-copy --owner-requested`) and the iPhone (`tools/mg run --platform iphone --physical-device <id> --real-copy --owner-requested`).
  - It signs `aps-environment` = `development`, so ordinary development profiles authorize it. Its CloudKit settings stay production.
  - Reason: it is Antoine's personal copy, iCloud Sync is paused, and Meguri Sync does not use push.
  - Every other build keeps its push environment, including Release as built for distribution and by `xcode-check`.
  - Revisit before `MEGURI_CLOUD_SCHEMA_CONFIGURED_production` becomes `YES`, because iCloud Sync relies on push; `cloud_build_configuration.py` refuses the exception while readiness is `YES`.
- Antoine (2026-11-21): the coordinator installs the real copy on his Mac and iPhone at his explicit request, with the app quit, through those routes (`--install` on the Mac). Nothing launches it. No other agent installs it.
- Claude (coordinator), 2026-11-21, deciding escalation `E-20261121T155116Z-246e` under Antoine's delegation of that day ("I will let you decide and carry on with your best judgment"): text in a golden that is Anki's own output, written by `tools/mg oracle record`, is allowed and is not a similarity finding, because reproducing that output is what the compatibility tests are for. Limits: goldens only, never scenario inputs, comments, names or spec prose; only goldens tied to an oracle recording; values from Anki's tests or code in a scenario are still findings and get rewritten; no path pattern. Mechanism: `docs/process/provenance.md`, "Similarity scan".
- Antoine (2026-11-21): unsigned Swift package tests may run on GitHub's hosted macOS runner; the Docker-only rule stays for everything else in the cloud. The other exception is the unsigned Core XCFramework build on that runner (2026-11-19, `tiers.md`).
