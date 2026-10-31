# First-adoption payload assets

This slice starts at `4e8b18c675b6b75807a1bcf10454d484714b126c`.
The original batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Tests precede production changes:

- `core-test/20261031T173310Z-10642` reaches the real adoption refusal after
  the sender saves a payload asset. The live collection stays unchanged before
  activation. The test also requires exact rows, ACK, reopen and review entry.
- `swift-test/20261031T174512Z-15524` runs the native record callback with a
  real asset. It fails with `unsupportedAsset`, where acceptance must instead
  reach the existing missing-zone-reply check.
- Earlier Swift runs failed to compile an opaque SDK token or selected no test.
  Neither provides acceptance evidence. The corrected test uses no fabricated token.

Independent review found no required test fixes. Native file ownership after
SDK-source deletion still needs coverage when the new path is implemented.
All existing assertions remain. This is tests-only WIP; U113 and U114 remain
unaccepted, without a Unit trailer.
