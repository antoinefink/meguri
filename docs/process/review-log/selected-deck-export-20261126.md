# Export a selected deck as an Anki package

The task branch began at `4430f2afe`. The package writer and native export action were built in separate filtered implementation runs against `8d1a579c1`. The package is a fresh database. The selected deck and its children determine which notes to include, and every card belonging to those notes travels with them.

## Provenance and privacy

The independent admissions reviewer read each complete record, confirmed that it contains public operational metadata and matched the committed bytes to the working copy.

| Session | SHA-256 |
| --- | --- |
| s-20261126T111145Z-29548709 | 939aa482a3a7af30b86c8e749701294dad077c2596b5d6ece9222953d68e7117 |
| s-20261126T111446Z-51e4bb75 | 1131fc5105d62e9d4944c607e53e202bb8dc87c2331742f5ae792e0f78350099 |
| s-20261126T111800Z-82c24254 | 9e17f4292e2aad848dfa23a9f02497fcb596e387527db983f902339a167a5b6d |
| s-20261126T113903Z-2752cdf1 | 5d9721e13fdc13f715869a4a9a866694fc2768637c156681c605144277a79cd5 |
| s-20261126T114439Z-bdd3910d | 7d92df79d1994440f76467abb2a644a808a5cd7c7b98b3112c1597471be039bf |

The same reviewer read the complete `core/crates/meguri-host/src/collection_export.rs` and confirmed public Rust source, generic messages and synthetic test values. The filtered native run, staged file and working file matched SHA-256 `bc322830588cef34dd00c8d9a5603e94e9befea0a72fa5c297e3d8e37ceb4fdf` at initial admission.

## Independent review

The package implementation ran in `20261126T111834Z-38243` and the native implementation in `20261126T111835Z-38679`. Read-only review run `20261126T113721Z-7073` traced the selection, media capture, host ownership and native destination through both commits. Findings covered Unicode descendant names, recovery of an interrupted package, reading unrelated deck records during naming, misleading database errors, feedback for empty decks and empty LaTeX blocks.

The fixes use the collection collation on complete deck components and read the selected deck name directly. An interrupted export can recover its own reserved temporary file. The native sheet reports the writer's counts and explains what an empty package contains before saving. A generation change invalidates an export even when both collections use the same deck ID.

Run `20261126T121337Z-31651` independently re-read the writer, storage rules, media boundary, native UI and ownership path after the snapshot and shared card-shaping changes. It reported no findings. The PR review also checked the touch menu's delete confirmation, the launch diagnostics and host ownership of a synthetic damaged-deck fixture. The two suggestions about template-media scanning were resolved against A9.1 and A9.5. Template references use raw case-sensitive matching and NFC normalization.

## Changed source admissions

The independent reviewer read each complete file at each admitted revision and found public source and synthetic data. The digests below follow commit order for each path.

| Path | SHA-256 |
| --- | --- |
| `core/crates/meguri-host/src/collection_export.rs` | `bc322830588cef34dd00c8d9a5603e94e9befea0a72fa5c297e3d8e37ceb4fdf` |
| `core/crates/meguri-host/src/collection_export.rs` | `ec3d9176ec7a6a776b40d311a8e0f6c590921488a49c962ebd95ba1acb9f9e65` |
| `core/crates/meguri-host/src/collection_export.rs` | `d8b35cd6ade111ff36283dc0e461c8ff1a88ed00c96f5f422ff0b8616bf3dd3e` |
| `core/crates/meguri-host/src/collection_export.rs` | `31ce7cc1cdd28d5f2fb69a64e7a1ce4bb0cfee8ae2861d505ab12f0202ec316a` |
| `core/crates/meguri-host/src/collection_export.rs` | `11423f8a11be1a736823c96c281bb4c2b6bd14cfe10f3e7fb15b02af4dbfc10e` |
| `core/crates/meguri-ffi/src/collection_export.rs` | `07f043190bbdfe85a01c528583edb02177be0a3bae9737ee3e7db72e437dc599` |
| `core/crates/meguri-ffi/src/collection_export.rs` | `b7eeae7c8e3a500640b0a22e6b736cb9876b4d4b05986c6019044a847559c3cb` |
| `core/crates/meguri-package/tests/deck_export.rs` | `d3556478eb2f8b362fb09188bc1a784c7805db262d94a7cf196bd4090a94ea61` |
| `core/crates/meguri-package/tests/deck_export.rs` | `8034f306052593560f5fd328062f402e7aba6662d817138935c55e989d31da89` |

## Verification

The Docker integration run `core-test/20261126T121257Z-27087` passed 25 tests. It covered all eight export option combinations, importing the archive into a fresh collection, excluded row and media data, changed deck membership between preflight and writing, Unicode deck components, cancellation, and retained output recovery. The focused unit run `core-test/20261126T121346Z-32473` passed five cases covering media parsing, filenames and the native save lease. The six branch static checks passed in `static-checks/20261126T121331Z-30552`.

The extra review of retained recovery files led to using the existing bounded identity inventory when exporting referenced media. The ordinary-media invariant verifies a matching baseline and the no-media case. All 21 capture and package cases passed in `core-test/20261126T121753Z-50849`. The malformed-deck fixture now writes only while the collection is closed and passed by itself in `core-test/20261126T121655Z-46357`. A9.3 specifies exactly which HTML line breaks affect a LaTeX image name, including the treatment of `<br />`.

The native UI journey uses the public Local Review fixture and attaches the saved file on macOS. The build and UI receipts, screenshots, and archive inspection belong with [PR #235](https://github.com/antoinefink/meguri/pull/235).
