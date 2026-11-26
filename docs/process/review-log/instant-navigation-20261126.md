# Main navigation responsiveness

The branch started at `1d191a6f9`. The baseline Swift run `swift-test/20261126T195812Z-34239` passed all 14 saved-deck cases. With the selection write held, the previous page stayed visible until the write returned. The filtered implementation run `cleanroom/20261126T195511Z-30305` exported the navigation change, gated read fixtures and native screenshot tests.

## Routine privacy admissions

Independent reviewer provenance read each complete session record and confirmed public operational metadata. The orchestrator verified each SHA-256 digest.

| Session | SHA-256 |
|---|---|
| s-20261126T195333Z-afca17ca | 105a4b7f8da5be108a11ae20880cd0cf1f46a28d9a000fc4904119e1964f578e |
| s-20261126T195846Z-e5754f88 | 8745c7f89a535c1b389117e7a1c4497856e4a5fc116dc42622bcd3def7f4f410 |
| s-20261126T201221Z-a51f7c34 | e73efdc8f7ce9e04f64eeab18b97b4ac6f58715c753d142d77a690aaa8e4628d |

The same reviewer inspected the complete files below, including comments and literals. Both contain public Meguri implementation and synthetic test support. The orchestrator verified the exact repository paths and hashes.

| Source | Bytes | SHA-256 |
|---|---:|---|
| `app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift` | 167892 | 6574faaf99d334524da053e7f85f663805ac3126f5289f9e8eb763a94e0ccfbc |
| `app/Packages/MeguriKit/Sources/DesignSystem/Components/Status/DelayedProgressView.swift` | 1008 | 3c412316533cc9498b80068f589391a1fbca6787b3205722daeac5011615ff14 |

## Corrective review

The independent source review identified pending-selection Back navigation and commands that could still target the prior deck. The fixer added coverage for both and for cancelled Browse reads. The independent privacy reviewer read the corrected `app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift` in full (169016 bytes), and the orchestrator verified SHA-256 `79cb64c7ebf3408df4622bb728d57ad9d3a76df9cc2822075a70a9272d769589`.

The upstream backup integration changed the same collection source. The independent reviewer read all 184150 bytes of the merged file and approved SHA-256 `f6b94d3d852e1b399758fefe3fdc7cf49edf6f3fa70d7dbc418f56aecdb5fc6c`, verified again after integration. Both macOS test registries retain the backup and navigation routes.
