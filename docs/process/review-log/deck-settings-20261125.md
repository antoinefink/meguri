# Deck Options display order and preset creation

Work started at `75697e95a`. The filtered implementation run `20261125T152010Z-43454` produced the changes in `c9e7c0bd3`. Independent filtered review `20261125T153101Z-63773` found a collection command could act behind the New Preset sheet.

## Routine privacy review

The independent non-author reviewer `ledger_review` read each complete candidate. The session records contain public operational metadata. The native interface contract contains public design guidance, UI strings and examples. The reviewer found no private collection content, credentials or third-party source. The product change derives from the existing Meguri scheduling specifications and the requested native controls. The coordinator verified the following whole-file SHA-256 digests.

| Path | SHA-256 |
| --- | --- |
| `docs/process/provenance/sessions/s-20261125T151935Z-2772129a.json` | `f4596f336235c527faa1f9475c1f12c5fefa03d4e0e2fa3f4330294964f564c8` |
| `docs/process/provenance/sessions/s-20261125T152446Z-4fd3c2f1.json` | `42f8e7f7d21bd07dd873607114071eb56001fe95a9b2bd595bde1a1d2e526b75` |
| `docs/process/provenance/sessions/s-20261125T153418Z-90a02763.json` | `92bbb9f3f4ceece07f6f00589a0f222e8ac2c9d829fc4c5254e9a26c3ce183ab` |
| `docs/design/native-interface-contract.md` | `3093e8502c312bf6e11a44a4e688e95fbc6537b3ce3e83e2307a302a7c4bb582` |

The shared sheet fix from `20261125T153446Z-75421` landed in `63eb53a27`. The follow-up source review confirmed that text editing keeps native undo and the collection refuses commands behind the sheet. It also found a preset selection could hide an invalid value during validation. The filtered fix `20261125T154332Z-2398` holds preset selection through the pending validation. GitHub review independently identified the selection race and an ambiguity in the preset footer. The wording fix `20261125T154612Z-4684` explains that Save assigns the selected preset. `ledger_review` repeated the independent complete-content review of the clarified contract and classified all 286,521 bytes as public. The coordinator verified the replacement SHA-256 `22cff2ec7c5c71540d5e6326c2c62993a9e7f8ee01021d354522c2abcabc5707` for `docs/design/native-interface-contract.md`.
