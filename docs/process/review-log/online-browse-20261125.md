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
