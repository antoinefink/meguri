# Note type deletion alerts on iPhone and iPad

The UIKit alert uses the same loss counts, backup notice and sync messages as the Mac alert. Its presentation follows the owning navigation route. The independent review checked presentation ownership, the live action state, dismissal and fresh confirmation when the data changes. Run `20261126T091138Z-8596` raised a coverage gap for renewed counts and accessibility text sizes. The follow-up adds native UI cases for both.

An independent non-author checked each provenance file in full. The committed and working copies matched and contained public operational metadata. The same reviewer read all 16,866 bytes of the UI test file, including comments, paths and literals, and confirmed the staged bytes contained public test logic, synthetic fixture data and user-facing strings.

| Path | Reviewed SHA-256 |
| --- | --- |
| `docs/process/provenance/sessions/s-20261126T090456Z-d2412174.json` | `26dcb48cfc9b6074a2a7a56a975331411f0ef877d50038a5cb4acfa5a9baecf9` |
| `docs/process/provenance/sessions/s-20261126T090610Z-bc798fe1.json` | `f735a6d74d833ac74506fcfc21958e03ddde4a027563355cbaecfd25e273323d` |
| `app/MeguriUITests/TouchNoteTypeDeletionUITests.swift` | `a15a4b80138f4fc769b16afb9298e0d67aa88e520a2712c0e15d5059edbbd4ec` |

The follow-up starts at `4430f2afe`. On the iPhone simulator, all four applicable cases passed (`ui-test/20261126T095103Z-27083`). At the largest text size the test reached the last paragraph and tapped Cancel. A separate case required a second confirmation when the counts changed.

The landscape iPad production-list case passed with the 90-note, 180-card fixture (`ui-test/20261126T091639Z-11709`). This run also exposed XCTest's 128-character limit for identifier queries. The corrected tests match the entire warning by accessibility label. Full-screen captures from both production-list cases are attached to [PR #233](https://github.com/antoinefink/meguri/pull/233).
