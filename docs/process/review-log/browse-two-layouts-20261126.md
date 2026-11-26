# Browse in two layouts review

Base commit: `d98894622b2d966fb5caeac9a03cb2e89ef052c1`.

Plan: [docs/plans/2026-11-26-0903-feat-browse-two-layouts-plan.md](../../plans/2026-11-26-0903-feat-browse-two-layouts-plan.md). Four independent plan reviewers (coherence, feasibility, scope, design) checked the plan before implementation; their corrections are in the plan.

The implementer session `s-20261126T081226Z-5a94a9d7` changed the web client, its browser tests and the W12 wording. `sync-web-check` and `sync-web-verify` (115 tests, run `20261126T084244Z-41034`) passed on these bytes, and the browser captures in that run come from the generated fixture.

An independent non-author reviewer read the complete candidates, including the synthetic Spanish sample cards already admitted for the online Browse work, opened no private inputs, and approved these exact bytes for routine privacy admission under the standing delegation.

- `docs/plans/2026-11-26-0903-feat-browse-two-layouts-plan.md` — `49a6d391cfdac35a88d2e746391dc27f281102680afa1e1ceb47939ed8eedd04`
- `server/web/src/browse.tsx` — `2d5e21502ebb5c850d922a18b742fee33300bcdc6ff126d18ed833ba432bc711`
- `server/web/src/style.css` — `cb87d2475ac77d53db3fbe6ea2d0c51fb9e7dfcc4b858193007882e09a4d5ee8`
- `server/web/tests/browser/browse.spec.ts` — `22ff413d3fd71c2357e247c963ff32d5ca86bb58aa03f5a4a8b155fd5b8bd363`
- `server/web/tests/browser/browse-real.spec.ts` — `3ff13664df064adc095a2558404f0a7113a0a98725481dfe31033ccc68cc44ad`

The simplification pass (shared icon and scope helpers, one Save and Load server copy element, delete failures routed through the shared error handler, a guard against restarting an opening card) and the web input note changed these files. `sync-web-check` and `sync-web-verify` (115 tests) passed again on them. The independent reviewer read the complete revised files and approved these exact bytes.

- `server/web/src/browse.tsx` — `0fdba79f1fcf698bff56c0591492577eea5056dc5b13cf89afe9e02cfaa90682`
- `server/web/src/style.css` — `01b95c7628651cbe0401bbb5df3c623dfe5980eaefdd97ae5fd2a63d65d471ed`
- `server/web/AGENTS.md` — `e2914f8ba8237ed9ba58cef06b35b51ad875e02e0192dd53b59808e05388c7b6`
- `server/web/scripts/audit-assessment.json` — `9ab8f116176d337a8fe06da25519342f3cb6f2954194060adf832e8b5f753650`
