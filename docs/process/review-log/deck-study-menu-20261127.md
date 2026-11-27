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
