# Native sync status identifiers

Repair start: `8b0681d58f78d577c0e490fef03022bfebe397a6`. Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The task is reading actual iCloud status and reaching Sync Now or Account & Sync. The system Form, Toggles, buttons, navigation and visual styles remain unchanged. Direct identifiers cover existing status, account, heading, footer, attention and recovery elements. Row positions use existing fixed display order; enum identity remains intact. Data, copy, action behavior and disabled states are unchanged.

The new classification applies only to missing IDs. A native iOS Toggle must have an owner with an ID, equal binary values and equal labels, exactly two leaf children, and finite child frames inside the owner. The source nodes keep their identities. Checks still report bad IDs, duplicate IDs, unrelated controls and every other diagnosis. The change adds no custom control, group, ID assignment or allowlist.

The saved patch is intact. Author: `cleanroom/20261112T161737Z-33283`. Independent source review: `cleanroom/20261112T162125Z-36734`; no P0–P2 findings. Root inspected the retained phone report and found three attention elements without IDs. Fixer: `cleanroom/20261112T162409Z-38633`. Second independent review: `cleanroom/20261112T162702Z-40777`; no P0–P2 findings. Test results, review results and receipts below are unchanged. Native checks and renders remain pending.

Evidence:

- Pre-fix: `image-tools-selftest/20261112T154843Z-94713`, twelve methods with five failed positive-case assertions. All original assertions remain intact.
- Focused: `image-tools-selftest/20261112T162721Z-41535`, pass, twelve tests.
- Full image-tool regression: `image-tools-selftest/20261112T162804Z-43433`, pass, 201 tests.
- Existing Swift status-copy tests: `swift-test/20261112T162728Z-41869`, pass, 2 tests. Its nested `lint-ui/20261112T162728Z-42082` passes, 146 files.
- `static-checks/20261112T162839Z-45021`: pass, 3 checks.
- Similarity: `similarity/20261112T162954Z-45999`, pass, 4 files scanned, no findings or exceptions. Only the permitted implementation view was read.
- Wording repair: `cleanroom/20261112T163355Z-51581` and `cleanroom/20261112T163735Z-56081`. Swift source, recognition logic and test assertions are unchanged. One test name and invented label changed.
- Final tool tests: `image-tools-selftest/20261112T163908Z-57944`, pass, 201 tests. Final Python similarity: `similarity/20261112T163913Z-58244`, pass, 2 files. Privacy: `private/20261112T163854Z-57659`, pass, 5 files scanned.
- Exact normal-device native checks and renders: pending. These source commits remain WIP until that evidence and the required batch gate pass.
- Retained normal-phone drives remain failed on their original identifier findings; none is relabelled as passing.

Contracts: native-interface-contract N4, N11.4, N11.8, N12.4; screen-inventory `sync.status` and `settings.accountSync`; R1964. No unit, integration gate, milestone or release acceptance is claimed.
