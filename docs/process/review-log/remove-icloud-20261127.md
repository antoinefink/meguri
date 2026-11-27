# Remove iCloud from Meguri

The unit started at `2b80682e8`. The user authorized removing iCloud and merging
the verified pull request. Meguri Sync remains available through the sync
settings on Mac, iPhone and iPad.

## Implementation and review

- Initial app removal used filtered session `s-20261127T065608Z-f736fb6a`.
- The collection recovery work used filtered session `s-20261127T065853Z-a0976797`.
- Review found that rewriting archival bindings breaks replacement history
  required by Meguri media recovery. Session `s-20261127T070953Z-653845b2`
  preserved that history. The focused run `core-test/20261127T072207Z-14982`
  passed all 15 recovery cases.
- The independent privacy reviewer read the complete N30 contract and
  classified it as PUBLIC. The verified SHA-256 is
  `cd47b8392bcb4edbe3c2cbfaf62585f0cc7d64e36f00f4b831cb340aa1041c4e`.
  The independent review also covered all 20 session records added through
  `0f1402451`. Each contains only operational metadata.

## Focused verification

- `core-test/20261127T081126Z-6865` passed 252 cases across provider migration,
  collection open and recovery, first Meguri download, and the FFI.
  `core-lint/20261127T081326Z-9233` passed.
- `ui-test/20261127T083248Z-39185` passed both Mac journeys in the test VM on
  `bb479044e`. It verified the Meguri setup controls, cancellation of a switch
  to Off, and native Settings Back navigation.
- `selftest/20261127T083248Z-39352` passed 11 packaging and preview cases.
  `selftest/20261127T083954Z-52927` passed 14 VM dispatch and performance-policy
  cases. `milestone-gate/20261127T074530Z-53769` passed its manifest check.
- The Swift package compile on `bb479044e` identified the stale factory call
  in the integrated suspend fixture. `131c76066` corrected it.
  [PR #246](https://github.com/antoinefink/meguri/pull/246) records subsequent
  Swift, simulator and hosted verification. The full integration gate is
  tracked in the shared verification register.

## Native journeys and final review

- The iPhone and iPad Account & Sync journeys passed in the test VM on
  `de756175b`, using only the runtime products specified by the Xcode test
  plan in the disk-constrained guest. The receipts are
  `ui-test/20261127T090511Z-99662` and `ui-test/20261127T090726Z-5912`.
- `swift-test/20261127T085846Z-86826` passed 48 focused sync, collection,
  suspend and diagnostic tests. It verifies the original connection owner
  drains before creating the current one, with one credential reload.
  `swift-test/20261127T090837Z-11072` passed 27 diagnostic and owner tests.
- `selftest/20261127T090839Z-11762` passed receipt and product inventory checks.
  `core-test/20261127T091530Z-21319` passed 18 recovery and local integrity
  cases, including refusal of a substituted activated file and completion
  from the original inode after WAL writes.
- Hosted verification found that retired cloud transport and the historical
  Rust test fixture both open without transport. The native FFI now selects
  application policy at every retained host boundary. The historical fixture
  keeps its existing provider choice between offline and connected phases.
  Independent review checked all five app entry paths and the debug-only
  local integrity bridge.
