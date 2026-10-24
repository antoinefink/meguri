# Mac embedded widget refresh

Start: `dacb1ad37c53828d1a38d82372c6503f49be0dc5`.
Fixer session: `s-20261024T174115Z-511e3ad6` (infrastructure integration).

Gate build `build-app/20261024T191128Z-51399` rejected an embedded widget with
a signing certificate different from its parent. Read-only diagnostic
`mac-signing-facts/20261024T193233Z-66947` confirmed the app and source widget
had ad-hoc signatures, while the embedded widget retained a development
signature. All three lacked embedded profiles; no account change was needed.

Before a Mac build, the helper deletes the previous widget directory from the
generated app bundle. The original extension product remains. The helper rejects
symbolic links and accepts only the documented build directories. Code signing
settings do not change.

Review: clear.

Checks:

- `selftest/20261024T193921Z-70120`: 14 passed.
- `build-app/20261024T194135Z-71175`: Mac Debug passed with signature validation.
- `static-checks/20261024T194143Z-72471`: passed.
- `similarity/20261024T194139Z-72195`: passed.

This closes the final identified repair before the shared full integration
gate against `81c207573c20c337c8834e4ae8b4ee8f29603dbf`; acceptance still requires
that gate and the remaining native checks.
