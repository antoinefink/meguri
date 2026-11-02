# Approved compiler import

Status: component verification; full batch acceptance remains open.
Start: `a2e5ee89`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Approval: `E-20261102T070407Z-6a04`, resolved by Antoine.

The approval covers one exact public Swift compiler import. Its digest is
`0cd4593e5b7c20a193357a8d46e91ae8d04f5d92d0ecf4aa3167ac4dc0379c5a`.
The guard exempts only matches fully inside that exact fact in accepted syntax.
Bare names, custom names, surrounding text, qualified names, changed case and
crossing matches remain checked. Committed facts do not make their component
words globally public. Raw, decoded, binary and staged forms have synthetic
boundary tests. No private input was read for this change.

Independent reviewer: `native_bounds_review`, clear on production logic and
the final test-only delta. Filtered exports: `cleanroom/20261102T071526Z-92014`,
`cleanroom/20261102T073551Z-19577`, `cleanroom/20261102T074846Z-30021`.
The first two focused runs retained a failing Unicode test: its URL helper
encoded code points rather than UTF-8 bytes. The first correction changed a
different helper; the final delta fixes the helper used by that test. All
boundary assertions remain enabled. Production guard logic was not relaxed.
Failed receipts: `selftest/20261102T072735Z-10938`,
`selftest/20261102T073038Z-15036`, `selftest/20261102T073841Z-21521`.

The direct import restores required declarations on iOS. The earlier Mac
compile passed without it, but the iOS compile `build-app/20261102T064114Z-65255`
failed with nine missing declarations. The same restored import bytes pass
`build-app/20261102T073842Z-21779` for iOS simulator. Mac build
`build-app/20261102T063930Z-61414` also passed. These are compile evidence only;
neither launches a signed agent or proves an SDK exchange.

`selftest/20261102T075035Z-32120` passed the scanner wrapper, including all 76
unit tests. `private/20261102T075036Z-32362` passed for all three source files.
`similarity/20261102T075105Z-33106` passed for those three files.
`static-checks/20261102T075101Z-32807` passed all three checks.

No Unit trailer or signed runtime acceptance is claimed. The U114 escalation
is unchanged and this approval supplies no additional repair rounds.
