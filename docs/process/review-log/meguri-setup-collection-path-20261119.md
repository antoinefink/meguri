# Meguri Sync collection path admission

Base `920ac312601588579781219a11ba6492c4eeea99`. Specs `sync/meguri-provider#MP4` and `sync/one-way#O4`.

The engine opened the canonical collection path. A manual transfer compared it with the original configured path before creating an operation. A normal ancestor alias caused that comparison to refuse the same collection. Transfers now use the canonical collection and backup directories already resolved by CoreHost. Setup still validates the original configured paths for disallowed components, aliases that changed targets, and overlap with the operation journal.

Cleanroom `20261119T152930Z-49530` produced the original reproducer. Independent review identified the need to preserve raw path validation. The follow-up `20261119T153246Z-60154` retains those checks. Integration qualified the `CollectionPaths` import from its owning module. Independent complete-file source review by `verification_plan_review` was CLEAR and classified all three exact candidate files PUBLIC.

| File | SHA-256 |
|---|---|
| `core/crates/meguri-host/src/open.rs` | `80a6d4f0358fb393f7ac6d0de4c4c8cb0314475639a04df87ac5171fa9cbedce` |
| `core/crates/meguri-host/tests/provider_bootstrap.rs` | `a49307900a8e7d35ff715d2085142d3c2a83f45eaa8cd57c5896437f99a0b763` |
| `core/crates/meguri-sync/src/oneway/mod.rs` | `c8d0df87b3996fe75262dcd4671f6ba65bbf60c6e9381ca139078e87c38fd18f` |

The alias regression failed with `Unavailable` on the original code in `core-test/20261119T153352Z-63070`. All 26 bootstrap cases passed in `core-test/20261119T153622Z-74529`. The new positive case verified the preview and left the local file untouched. The new negative case verified no request or journal on a forbidden alias. The privacy scan `private/20261119T153658Z-75524` passed on all three files. Temporary device diagnostics are outside this patch.

The scoped similarity check `similarity/20261119T154923Z-7918` passed. Signed iPhone installation and normal launch passed in `run/20261119T153737Z-76649` with the same repair plus temporary local-only worker markers. The install receipt does not establish a completed sync.
