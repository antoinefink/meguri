# Entry export fixture startup

- Role: fixer, implementer-view clean room; followed `docs/process/roles/implementer.md` and applicable AGENTS. Requested main baseline: `df308d23`; filtered runner starting commit: `96b1c4b88b082ff0d9f6e192600c7ea90383cf69` (no upstream history available).
- Reported failed gate: `core-test/20261112T141536Z-17693`, `public_initial_entry_imports_all_media_and_initializes_own_journal`, `collection_entry.rs:654`: raw package preflight unwrap returned `ExportSourceChanged`. The failed gate remains evidence; no passing run is claimed.
- Own-source inspection confirms the fixture opens CoreHost, seeds four media entries and calls raw package preflight without joining derived startup. `HostPort::run` queues `Msg::Bind` and returns; the derived writer later opens/creates the sibling derived database in WAL mode. E7 `ProtectedInventory` freezes and validates the parent stamp, so startup namespace changes can correctly invalidate preflight. This supports the reported race; no runtime reproduction was attempted here.
- Added only the existing asserted `derived().wait_idle(Duration::from_secs(5))` pattern after media seeding and before the reader/preflight, matching `collection_export.rs`. It joins known fixture startup before capture; it does not retry preflight or add sleeps.
- Zero production changes. Both fixture variants and all original media, original-byte, activation, journal, close/reopen and public export assertions/checks remain unchanged. Export validation, source stamps/limits, retry rules, job behavior and semantics are unchanged. No duplicate test added: the existing case is red.
- Source consistency: `git diff --check` passed. Builds and focused tests: **UNRUN** in this runner, as requested. No forbidden sources, network or agents used.
- Integrator: apply only after the active full gate ends; independently review, run focused tests for both fixture variants, and complete required provenance/privacy/similarity checks. No acceptance claim.

## Integration evidence

- Applied after gate `20261112T141110Z-8802` and its Rust child terminated through MG cancellation. The partial gate remains incomplete.
- Independent review `cleanroom/20261112T143558Z-39946` found no P0–P2 issues. It confirmed FIFO bind/flush ordering and retained the caveat that source inspection cannot prove the failed run's exact interleaving.
- Both original fixture variants passed in `core-test/20261112T144929Z-42469`, alongside seven refusal tests. The nine-test selection used serial compilation without incremental reuse after two retained build failures; no assertion or timing limit changed.
- Static checks passed in `20261112T145057Z-42816`. Original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`; full integration acceptance is pending.
- Rust lint passed in `20261112T145507Z-43397`; similarity passed both source files without exceptions in `20261112T145521Z-43710`.
