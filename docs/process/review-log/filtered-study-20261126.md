# Filtered decks and Custom Study

The feature branch began at `400306904`. The spec writer checked behavior for F1–F6. Three filtered implementer runs used the committed specification at `55862a28e`. The exports were `111938Z-47084` for deck operations, `111939Z-47170` for native UI and `111940Z-47724` for review scheduling.

Independent source review `113142Z-83466` found that the writer and session overlay omitted original-deck fields, and that finishing a learning card in preview could requeue the card. The original home-return/Undo regression passed in Docker at the feature baseline.

## Routine privacy admissions

Independent non-author reviewer plancheck examined the complete session records and classified their contents as public operational metadata. The orchestrator verified the four complete-file digests.

| Session | SHA-256 |
|---|---|
| s-20261126T111401Z-41e5f490 | 01c380cace200f9d22df1da7e33805569d691160553ce8b41f54a096d3b2260e |
| s-20261126T111454Z-c1c46efe | 6ce6738c6c12eace4390f5b362aaa40b78e1bcb5cf3c42aa837d625d789faadd |
| s-20261126T112409Z-e3850e09 | a9d73a42e4149c456c144ba94f4cfbe8d6d29f6cb9bf93a7fdf669ac36cc1793 |
| s-20261126T112411Z-8c5a0336 | 5fbce8f52b1e8ed65adf345305bc9dd805057e2cbc9a6255d3ff3f8c4f947452 |

Plancheck independently examined every byte of `app/Packages/MeguriKit/Sources/Features/Home/DeckManagement/FilteredStudyForm.swift`. The file implements the public F1–F6 contract using native controls and runtime bindings. The orchestrator verified the reviewed SHA-256 `a5345e27c501bc569899b8982826cff4da8d595670b5368844e9eca419795cf5` for the exact path.

The explicit empty-save confirmation changed the native form. Plancheck reviewed its complete 12,876 bytes again. The orchestrator verified the new public SHA-256 `bb91db2a2fbf78fd4877527ace43e5b2cf9bddf47337c5ea7e8f520ead4592ba` at the same path.

Plancheck reviewed the complete `core/crates/meguri-engine/src/decks/custom_study.rs` after the range and scope changes. It contains public F5 logic and takes deck and tag data from the collection at runtime. Verified SHA-256 `73e1e2a2e65526dc81129bf299b7731bcad202fb37f5404b55b5a31baeb1b166`.
