# AGENTS.md

Meguri: native SwiftUI app (macOS, iOS, iPadOS 27) on a Rust core, fully compatible with Anki collections and sync. Requirements: `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md`.

## Hard rules

- No Anki code (AGPL) in Meguri, copied or translated. `refs/anki` is for spec writing and the test oracle only.
- `local-data/` is private (real collection, research). Never commit it or send it anywhere.

## Tooling

- Build, test and verify only through `tools/mg <job>` (`tools/mg --list`; contract in `tools/AGENTS.md`). Rust, oracle and sync-server work runs in Docker; Apple builds and simulators run on the host.
- Never perform the one-time user steps (`docs/process/one-time-setup.md`); `tools/mg doctor` lists the missing ones.

## Agent docs

- Add or update an `AGENTS.md` in a subdirectory when it has conventions or traps the code doesn't make obvious.
- Keep every `AGENTS.md` terse. Readers are highly capable agents: write only what they can't infer. No restating code, no tutorials, no obvious advice.
