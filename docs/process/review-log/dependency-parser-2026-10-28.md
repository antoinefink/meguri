# Markdown parser security patch

Starting commit: `218ab2e2543a2c4822b79c1ac3520230ca3c1998`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The interrupted integration gate reported RUSTSEC-2026-0414 in
`core-deny/20261028T103811Z-24555`. Update only pulldown-cmark from
0.13.4 to 0.13.5, preserving disabled defaults and the HTML feature.
Meguri uses CommonMark without extensions; no parser behavior is added.

Filtered source: `cleanroom/20261028T104158Z-26241`.
Independent source and lockfile reviews are clear. Docker resolution
`core-update/20261028T155436Z-84313` changes only the package version and
checksum, with unchanged dependency edges and parity pins.

Verification:

- `core-deny/20261028T160027Z-88441`: all four checks pass.
- `core-test/20261028T160030Z-88705`: parity-pin canary passes.
- `core-test/20261028T160215Z-90754`: 57 text tests pass, zero skipped.
- `private/20261028T160614Z-94244`: manifest and lockfile pass.

No new test is needed for this dependency-only patch; existing description,
sanitizer and text regressions supply behavior coverage. The interrupted
gate stays cancelled and its failures remain recorded. No unit or milestone
acceptance is added.
