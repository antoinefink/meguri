# Six selftest failures after the dev identity: review log, 2026-11-21

Branch `unit/selftest-six`, fixer session `s-20261121T162958Z-4014b7dd`.
PR #58 (Meguri Dev name, `.dev` ids, blue icon) broke six `tools/mg selftest` tests on master. The hosted "Linux · selftest" job in PR #61 found them.

## Findings

- **Five tests in `tools/tests/physical_device_ui.bats` had a stale fixture. The product is not affected.** PR #58 added a product identity check (`app_identity_check.py product --identity dev`) to `build-app --personal-preview`. The check needs `sips`, `assetutil`, `osascript` and real plists. The bats fakes build placeholder products in the Linux container, so the check failed with `personal-preview-identity` ("Invalid file").
  - A real personal-preview build passes the check: `build-app/20261121T113603Z-79020` in the PR #58 worktree ran on `01750eb3b` and passed.
  - Since then the personal-preview route has changed only by refactoring. The checker has only relaxed (it now accepts a name shown with `.app`).
- **Fix for the five tests:** the check moves into `mg_app_personal_identity`, which the tests stub, as the real copy's `mg_app_real_copy_identity` already is. The checker still runs on real products here and in `xcode-check`.
  - The route still runs the same script, the same identity, the same failure id, in the same order.
  - The pass test now checks that the check is called on the exact product.
  - A new test checks that a failing check reports `personal-preview-identity`.
  - The seam and stub lines are the same as in open PR #72.
- **One test in `tools/tests/private.bats` had a stale pin.** `PublicLibraryTests.test_public_asset_mutations` checks the whole-file digests in `PUBLIC_VENDOR_ARTIFACTS`. PR #58 legitimately added a "Development icon" section to the app icon's README. Its path's SHA-256, the map key, is `757fd5fb3da8ccfca681ebdc23701b14eb3a62c18056c970122d156377c7099e`.
  - The fix refreshes only that path's value.
  - No keys, patterns or matcher changes.

## Public-file admission (`docs/process/provenance.md#routine-privacy-admissions`)

| File | Reviewed SHA-256 | Decision |
|---|---|---|
| the app icon's README (key `757fd5fb…`) | `0ff0200e298c6b2c06eca789fe37c8d967c2f3ce6a1e6fef42e89e19b4341a49` | **CLEAR**, **PUBLIC** |

- **Reviewer:** an independent non-author Claude subagent (Opus).
- **What it reviewed:** all 32 lines. It confirmed that the in-repo paths exist. It used no private inputs and did not look up matched tokens.
- **Contents:** public project documentation only; no identifiers, secrets or collection content.
- **Authorship:** unchanged. The file is Meguri's own text about icon images the owner supplied (see `app-icon-20261118.md`).
- **Bytes:** the same as `origin/master` at `f3e92f66f`. This branch does not change the file.

## Independent code review

The same subagent reviewed the diff and found no blocking issue.
- **Medium: this log was missing.** Fixed: this file.
- **Low: both identity stubs log the same line.** Declined. The failure test already catches a swapped identity, and the stub lines stay byte-identical to PR #72, so the two branches merge cleanly there.
- **Low: the real-copy stub is not used in this file.** Kept for the same reason. PR #72 adds tests that use it.
- **Info, not changed here:** the README entry sits under the Askama header of `PUBLIC_VENDOR_ARTIFACTS`. It was placed there on master before this branch.

## Verification

- **`selftest` on `tools/tests/physical_device_ui.bats`:** 97 of 97 pass. That is 96 existing tests plus the new one.
- **`selftest` on `tools/tests/private.bats`:** 22 of 23 pass. Before the refresh, `test_public_asset_mutations` failed its digest assertion on the README. After it, the README passes. The test then stops with `FileNotFoundError` on the first of the three pinned `oracle/` files, which a fixer worktree's sparse checkout leaves off disk; every other scanner unit test passes. On a full checkout, the hosted "Linux · selftest" job in PR #61 is the conclusive run.
