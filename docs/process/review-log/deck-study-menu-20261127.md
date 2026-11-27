# Custom Study and deck menu titles

The unit started at `be8c979e9`. Clean-room implementation `140449Z-75901` used F5 and F6 and the owner’s deck menu decision. The iPhone amount row keeps its full label and aligns the editor with the count. The context menu and its menu-bar counterparts use plain deck command titles.

Independent clean-room review `140851Z-80898` identified the model’s normalization of a leading plus sign and requested largest-text evidence. Follow-up review `141450Z-90769` identified the largest-text width assumption. Final review `141803Z-98078` confirmed the revised assertions and found no remaining source findings.

## Public-file review

The reviewers read each complete file and classified these exact contents as public. The orchestrator verified the digests in the task worktree.

| Path | SHA-256 |
| --- | --- |
| `app/MeguriUITests/FilteredStudyTouchUITests.swift` | `3b756e37eed70f82eedfbc038198122b4143a5f4901ba1c40b0077d7c54a5daa` |
| `app/MeguriUITests/NavigationFeedbackUITests.swift` | `1b7aa440e531065224b2cc1545b40c959d864c8785f43c8841ab77651250a3b5` |
| `app/Packages/MeguriKit/Sources/Features/Home/DeckManagement/FilteredStudyForm.swift` | `5d8e5268ff9cc4cf3a1d69a1443abfda78a248a34ad55b74af0e0daee6bc2141` |
| `app/Packages/MeguriKit/Sources/Features/Home/Resources/Localizable.xcstrings` | `53d51f1b189acf3e833808c9ddd5157ceef6623a66c8ff6c7753d1709b67e8e8` |
| `docs/process/provenance/sessions/s-20261127T140229Z-44294cea.json` | `da64ec897034ff13bf91b8226e521e366194aed75a23590f338694a3f3ba644a` |
| `docs/process/provenance/sessions/s-20261127T140715Z-fe532913.json` | `7ed194453ce0fc0fdbd939387731c58696b456da46cd6d1729185fe4d1fbdbff` |

## Verification

UI lint passed in `lint-ui/20261127T141827Z-98755`. Design documentation validation passed. Native render and hosted check evidence is recorded on the pull request. The repository’s [verification register](../verification-status.md) tracks the inherited open full gate.

The iPhone VM run `140510Z-76742` exposed a test error. The available count’s accessibility frame covered the digit, so the test now uses the window for its width check and compares the trailing edges of the two controls. Independent review `143553Z-34441` read the complete corrected touch journey and classified its verified SHA-256 `fdf0086ac9e2b255e61c2e09e10d5be4c864c22efd52defdf8bd49cdd5f69e2d` as public.

In iPhone run `143843Z-39137`, the compact journey passed and showed the full label with the two zeros aligned. At the largest text size, the label split words and the native read-only count moved to the leading edge. The amount label now takes the full width above the trailing editor at accessibility sizes. Independent review `145058Z-59556` read both complete revised files and confirmed the layout and test. The verified public digests are `d0db213c3114ef389a48b32a68ea41581a6fb57d130c203188a3bdb44ba91ca6` for `FilteredStudyForm.swift` and `4ab0a09a73e381571f1a37bddbdbfcf9201c1bb682f80d6908d776927d5d3745` for `FilteredStudyTouchUITests.swift`.
