# Online Browse review

Base commit: `24f4dd0d63eed18a0b83588258cac8d49fcaa83f`.

The independent plan reviewer read the full public candidates, including the synthetic examples, and approved these exact bytes for routine privacy admission under the standing delegation. Only the first two paths require whole-file pins.

- `docs/plans/2026-11-25-1512-feat-online-browse-plan.md` — `42ece08d046176f26f02f6e6f6e2d68f26f5029d9ddca6f5b7ff77f38b81d798`
- `specs/servers/meguri-sync-web.md` — `51efa64d58253ad3b092d70c50be807dc30823bd069256875ab7975b21c6f852`
- `docs/process/provenance/sessions/s-20261125T141722Z-04a248bf.json` — `c43d75d9efcc503dbb9c0f5852f77655487bace88a34ab70babd760af87e780e`

The review reconciled compact account navigation, bound Browse requests to the displayed account and collection generation, and required current sync stamps for deletions of recreated IDs.

The server reviewer inspected the complete Rust patch exported by cleanroom run `20261125T141926Z-84121`. The new server test uses generated accounts and synthetic vocabulary. Its exact approved bytes for the initial implementation are `07151331ac0489f0bf4b9ce42ca6fadf87ee0b555d8c1f5bac99c5678b0c69ea`. The first check found a fixture integer type error. The independent review also requested fixes for required account identity, visible text projection, large notes and logout contention.

The independent web reviewer inspected all changed web sources and tests, their synthetic collection and the MIT adaptation records. The following initial bytes passed complete-content review.

- `server/web/src/browse.tsx` — `095a096d756cc03d00dccd61548d1d81bf9f32afde9910fd49512f94793bd832`
- `server/web/tests/browser/browse-real.spec.ts` — `e8b068d84678caa5654dcdd7d425100354442c33beb492c5399770be2a4c95a9`
- `server/web/tests/browser/browse.spec.ts` — `069269d7a09c0d48abe640490f2e486287453f588040ee4125f9160690e9526f`

The first typecheck found a catalog declaration error. The review requested corrections to draft ownership, sparse search and list restoration. The web dependency graph and the build-time matcher inputs remain subject to the existing fixed assessment.

Fixer run `20261125T144821Z-18347` exported the identity, projection, logout and draft fixes. Independent Rust and web reviewers inspected the revised files and regenerated frontend manifest. The real HTTP and incremental sync selection passed 20 tests in `sync-server-test/20261125T145953Z-25765`. The real server browser journey produced desktop and compact captures. Reviewed operational metadata and synthetic file admissions for this round follow.

- `docs/process/provenance/sessions/s-20261125T144613Z-16dfc2f0.json` — `d09ccad0311df29bd8cc27c050574d64e27eedc73f6420d629e7885773c47910`
- `server/tests/browse.rs` — `a4ac2b397dc035bedcc7ed8ae221fe8e872293f4bf0e71e6218ff976542e845a`
- `server/web/src/browse.tsx` — `6248591f5718c951e888a783bc14eb800ba407dc5d4e22c2cba749e717beb961`
- `server/web/tests/browser/browse-real.spec.ts` — `4f018f173ceea78f439a885e5526f7485c0265ffbd94968f4423d2e8d3e7a261`
- `server/web/tests/browser/browse.spec.ts` — `c4d33db8a44eea9fc10c9e238e02538389b09cfb90f7f52babdcafa9bfa5c1c0`
- `server/web/scripts/audit-assessment.json` — `c5e9b8f26705263e67a795794d096d86b4ec0ea653008a183a9f7f2c1a6c4eb7`

Independent reviewers inspected the Rust projection, browser changes, generated tests, schema mount and fixer session from cleanroom runs `20261125T150654Z-30795` and `20261125T150839Z-31584`. Browser fixtures now read the tracked server schema. The reviewers approved the following whole-file snapshots and session metadata. They also identified a repeated metadata blob read for follow-up.

- `server/tests/browse.rs` — `854b682a4ed4d0974acfdc7c59eff71e55882cae668743ec0458df0edcd75dd2`
- `server/web/scripts/audit-assessment.json` — `8287f2b2f2b87a74dcc15599d5074c97381e728bdadc5e44d7e673c59e6a5dc8`
- `server/web/src/browse.tsx` — `f423c0ef1598a5e58ef781af2e1fb728d74c05463b81927f77cb97a1735cb24c`
- `server/web/src/style.css` — `1ae979edef65184c9f80823ed688768036a5873e40a1b0979d93f25b06627abc`
- `server/web/tests/browser/accounts.spec.ts` — `8aa9c1df0f2e3b89cae275d25e57e7c8f46ba51644b07b63eac2fa38e57c076c`
- `server/web/tests/browser/browse-real.spec.ts` — `eb93107249c9590a1b076c4a8d8962bb95d7bbe8873989e04c69dc92d1f6735f`
- `server/web/tests/browser/browse.spec.ts` — `bdf575d6a2885aad21e79fcdb908c4419ad37a21c1ab2f57ad428c54c705a45e`
- `tools/mg.d/sync-web.sh` — `4d5b78e965e356d65ed0ef67ae60b5b01f7171d2a130f6aeffae13b3cf5ff255`
- `docs/process/provenance/sessions/s-20261125T150342Z-453d824d.json` — `ed382bc21324f5d47c9419f0f8d50839fd5d9922e0dfc058b833a337b86817c5`

The server and web reviewers examined the complete files after cleanroom runs `20261125T151737Z-37986`, `20261125T152034Z-44303`, `20261125T152347Z-52646` and `20261125T152639Z-57550`. The PR feedback session handled field visibility, stale reads, catalog-independent editing and credential-fragment cleanup. The metadata session added bounded incremental SQLite blob reads and corrected the synthetic test assertions. The independent session reviewer approved the PR feedback session metadata. Both browser captures come from the generated fixture. Exact privacy admissions follow.

- `server/tests/browse.rs` — `d83caabe7cb86e4b6ac9befa5e4e5d4c469af11fa0dd4ea0142f2fd6e0cc198a`
- `docs/process/provenance/sessions/s-20261125T151948Z-d98eea19.json` — `e7c6621adec739d2f580915a77662a04838715682f45d47142a5ac875584d482`
- `server/web/scripts/audit-assessment.json` — `1a7395ed0774877d08635d76c231a6eccc18d86c69fafcf3d0310d6cd7ff9dbe`
- `server/web/src/app.tsx` — `bade6fe1e0b7127118f7e52815f7ad26542a46706493aee86e798f3d5d6e8d69`
- `server/web/src/browse.tsx` — `c94f0c3348d20e190c1e3efd81051c2006b7e18d1c69b98e03756053c0e7995a`
- `server/web/tests/browser/browse-real.spec.ts` — `f76ff57b65b65befd66ccf524213c938b76da053f4eae27a0dce7e6d6fb1d797`
- `server/web/tests/browser/browse.spec.ts` — `e2e1947904b415bce17d7a9ecd079d5d41ea3ee14dd786b932e59c9661f8f482`
- `server/web/upstream-records.json` — `68f9aab35e91111f608f4d056f4de7ffff153cf092c216fdbc6c0f48ab384385`
- `docs/process/provenance/sessions/s-20261125T151725Z-6d0637e0.json` — `97c13487dc2912036161db927fa4f01750c4efcd895a1820eeafdb066d307e0e`

History test run `20261125T153145Z-66176` corrects the fixture entry point. It loads the supported browser route and exercises real same-document navigation. The independent web reviewer approved the complete synthetic fixture, matching audit input and operational session record.

- `server/web/scripts/audit-assessment.json` — `89b3cdee886a1af842af4a4adfd665c919e6f143eb973582e607b020f6c8dfa5`
- `server/web/tests/browser/browse.spec.ts` — `02ebe0ed47b4e2c5019834be22368794d36f976271f623e7f1586af046877d5f`
- `docs/process/provenance/sessions/s-20261125T153128Z-eb7fef85.json` — `49a5a9e5257e2d449b303fdd0e49ad578d4f9b7ddc398fc159f57647cbf95904`

The history fixture now asserts the events Chromium delivered for each navigation type. All three Back and Forward cases passed in `sync-web-verify/20261125T153825Z-84099`. The independent web reviewer approved these revised bytes from cleanroom run `20261125T153737Z-83326`.

- `server/web/tests/browser/browse.spec.ts` — `d3c5c4507819b1bd3d778585bab96cb7490cac01741ca0f3f1ff36c2bb09d7f8`
- `server/web/scripts/audit-assessment.json` — `b8077355d45a759b03bf533748a1d427017ff2de9df731ccffee4720dc748b77`
