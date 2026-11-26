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

Plancheck reviewed all 1,586 lines of `docs/design/native-interface-contract.md`, including the revised menu and key bindings. It contains public product decisions, examples and repo references. Its verified SHA-256 is `5f81c9b56335f87958c5d53bba3dd4dbe770cccda8bc1764b89f9aa182286dbe`.

The keyboard shortcut and saved-state feedback changed the form. Independent reviewer planinterop examined its complete 13,239 bytes. The verified public digest for `FilteredStudyForm.swift` is `3c7cd4ebb07a6408449ccebce59ae624fb41be4ae044bf03a39e784821442310`.

Planinterop reviewed all 179 entries, including translator comments and plural variants, in `app/Packages/MeguriKit/Sources/Features/Home/Resources/Localizable.xcstrings`. The orchestrator verified the public catalog's 51,185 bytes and SHA-256 `6b96ea5123988e28e5a4a8a79187594ffeb5ef0ff85ee2f7871821dff1e16275`.

Plancheck inspected all 166,484 bytes of `ReviewCollection.swift` at `176e96726`. It handles collection state and native navigation, and takes card, media and account values from runtime arguments. Its verified public SHA-256 is `fb7df5a364e8fc7d99f79196d53705eca5bfa7826a00518486236a63cd6858cd`.

Planinterop reviewed all 292,739 bytes of the interface contract at `472bd637b`, including the added menu-bar commands and their enablement. The file contains public product decisions, feedback, examples and test measurements. Its verified SHA-256 is `bb9a40b994d55e420f4408d58927b6deddf0072e78b404677503591044c4bf24`.

## Verification

The collection writer and pending-answer projection now preserve the original deck and due date through a filtered answer. The Docker suite covered preview completion, rescheduling, overlapping searches and stale answers after a same-second rebuild. All 236 selected engine and search tests passed in `core-test/20261126T121534Z-38442`. The real collection-replacement test passed with test hooks in `core-test/20261126T115955Z-77161`. Rust lint, crate layering and the endpoint scan passed in `core-lint/20261126T122102Z-61502`. All six static checks passed in `static-checks/20261126T122201Z-64664`.

The native operation alert now offers Refresh Decks after a committed change. Plancheck independently reviewed the entire 13,590-byte form. Its verified public SHA-256 is `4ded70a1d10bb62adf87d3342b98857a266c92a7a1c7508a8f4ba43f32a29419`.

Planinterop inspected the complete 13,742-byte `core/crates/meguri-engine/tests/session_filtered_lifecycle.rs`. It uses synthetic harness data and runtime state tokens to exercise writer and observer timing. The verified public SHA-256 is `d18d814689444a9d2823c27512a4697f320746fb10cf68c58d4b609945943d35`.

Planinterop independently re-reviewed the 14,500-byte lifecycle test after its deletion and move cases changed. All records are fixture-derived. Its verified public SHA-256 is `847e05fc1456d929196a53a7deb4b401bf06e2441ebb67ba38ff3d4ce7be1fcc`.
