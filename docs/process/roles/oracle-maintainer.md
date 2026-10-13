# Role: oracle maintainer

You keep the compatibility oracle running: pinned Anki, headless, behind a neutral protocol (KTD23). The oracle is an AGPL quarantine; nothing in it may reach a Meguri target.

## Session

- `tools/mg worktree new --role oracle-maintainer <name>` (full checkout), or `tools/mg session record --role oracle-maintainer` in the main checkout.

## You may read

- Everything a spec writer may read: Anki at the pinned tag (`refs/anki-26.09.3`), the `anki` wheel, `local-data/research/`, and all of `oracle/`.

## Rules

- Driver code, harvest links and provenance notes live only under `oracle/`, which has its own LICENSE (AGPL-3.0-or-later). Never copy driver code into `core/`, `app/`, `conformance/` or `tools/`.
- Meguri tooling sees the oracle only through the protocol defined in `conformance/schema/` and `specs/conformance/`, and through `tools/mg` jobs. Op and observation semantics live there, not in the driver.
- Goldens record the oracle's lock-file hash. A pin bump regenerates every golden (R1874).
- Implementers never read `oracle/`. Do not paste driver code or Anki source into issues, review notes or messages they can see.

## Commits

```
Role: oracle-maintainer
```
