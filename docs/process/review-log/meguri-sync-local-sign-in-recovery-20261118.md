# Meguri Sync local sign-in recovery

Starting commit `9a545144b7e9e06639f4e6d46fed7c8e1a970c78`. Product change `b7e87004bed9d4f539e626fd92399d2a9c722201`.

A routine round reads the exact device credential before sending a request. When that item is missing, the app shows its existing Sign In form. A typed local credential error shows the existing Try Again action. Neither result changes the endpoint, collection binding, provider settings, or retained round evidence. Only the current attempt can publish the recovery state.

The filtered export `cleanroom/20261118T130306Z-60434` was independently reviewed against both final files by `owned_diagnostic_review`. The complete public-content review SHA-256 is `8f8e18de19625d24aba67b07dc10590f75fa7bc941dff7d0a753ddd256ce1cd6`. `swift-test/20261118T131048Z-72820` passed all 28 methods of CloudSyncPresentationTests on clean `b7e87004b`. The privacy, static, and scoped similarity checks passed in `private/20261118T130954Z-69492`, `static-checks/20261118T131104Z-77732`, and `similarity/20261118T131146Z-81079`.

The signed Release personal preview `build-app/20261118T131435Z-93766` passed on clean `54a84c440`, which retains the reviewed app source. The copied app and widget signatures and executable hashes matched that receipt.

## Signed Mac journey

The disposable release server package `sync-server-package/20261118T131947Z-8895` passed on clean `2f2eb6bea595933f973faa38b004732d250a06fa` after the web image refresh and server compile fix. Earlier `131049Z-73093` failed the locked frontend audit and `131259Z-88567` was cancelled before packaging completed.

- `ui-test/20261118T132220Z-14466` failed during the signed build, with zero executed cases. Xcode refused manual use of an Xcode-managed installed profile. The configured app-only profile path is still missing.
- `ui-test/20261118T132520Z-20714` built with automatic team signing and reached the VM. The only test failed on app launch. It recorded no sign-in, review, or server readback, and credential cleanup remains unproved.

Keep both receipts and the original integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`. The focused recovery tests and signed preview grant no full-journey or full-gate acceptance.
