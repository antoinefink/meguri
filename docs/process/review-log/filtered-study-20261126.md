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

The final search, engine and scheduling selection passed all 239 tests in `core-test/20261126T124047Z-42132`. The queue checks only cards eligible for presentation and detects a changed filtered-deck revision. Rust lint and layering passed in `core-lint/20261126T124135Z-46498`. The Apple bridge passed eight tests in `swift-test/20261126T130314Z-43690`, and the native study models, navigation and keyboard handling passed 144 tests in `swift-test/20261126T130546Z-57696`.

Both Mac journeys passed without skips in `ui-test/20261126T140102Z-38683`. The Browse journey used overlapping queries, Empty, Undo, Rebuild and timed preview across relaunch. The finished-review journey selected actual cards through included and excluded tags. Both compared stored scheduling fields and card identities before and after preview. The iOS simulator build passed in `build-app/20261126T132404Z-48445`. The 14-step native capture completed every action; its separate hierarchy audit reported the existing sync toolbar button's missing identifier. The filtered study journeys themselves passed. Captures of the isolated fixture are attached to [PR #237](https://github.com/antoinefink/meguri/pull/237).

Plancheck independently inspected every byte of the updated 71,387-byte verification register. It contains public checks, synthetic scenarios and repository references. The verified SHA-256 for `docs/process/verification-status.md` is `6fbebade36e4f5cadb45762fe6bde91c769d9ab54b1197c0e380502c629e3f61`.

### Integration with selected-deck export

The reviewers examined the complete merged files and approved these exact public contents. The records contain UI source and localization, public design requirements and synthetic verification results.

| Reviewer | File | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Plancheck | `docs/process/verification-status.md` | 72,225 | `8d491a279fc12638458d91f8ba60706257402813d3fefdb29802cacc04a27f63` |
| Plancheck | `docs/design/native-interface-contract.md` | 293,400 | `cbd5905a9d183331c00deba0ded170f74bc13f7a9467d2d92461804f6e0991cb` |
| Planinterop | `app/Packages/MeguriKit/Sources/Features/Home/Resources/Localizable.xcstrings` | 51,334 | `61695e0b755bcd741efff9aa6d31f15daeba8907508ef9893ea2508cd6d50225` |
| Planinterop | `app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift` | 166,524 | `5b329638e20ef17b258809d3e68089eaa04f059652cd6cfc4c113462be76f6ee` |

Planinterop independently reviewed the complete 23,767-byte touch journey. The flagged comments describe the geometry of a native scrolling gesture. All inputs are synthetic and the file contains only public XCTest source. Its verified SHA-256 is `47644f411be5dda7625f6290ad20c6f5f60d79e70e5fe0980c545fe558430bc6`.

After the upstream MCP media status changed, Plancheck reviewed the complete merged 72,424-byte verification register. It contains public test and integration metadata. Its verified SHA-256 is `ac341a60bbd004b30fb17fd4798d7d90d19b012b027d6b86ac4263ff63afd857`.

Planinterop re-reviewed the complete 23,768-byte touch test after it adopted the current SDK’s rectangle formatter. The file remains public. Its verified SHA-256 is `bb1165bc654036b458500f14144b4e8b592d21eb14a088b64b4e98affe5e5484`.

Planinterop reviewed the full 22,564-byte touch test after it began editing visible fields with the keyboard open. The code and inputs are public and synthetic. Its verified SHA-256 is `20a75301658060e8904c4f43a2b685f748e1d2cf44ce6ba69e7cb929e91a88b5`.

Planinterop reviewed the full 22,619-byte touch journey after native Select All replaced cursor-relative deletion in its text helper. Its public synthetic content has verified SHA-256 `c26eb356b8475eb65c523bce25dd5f24869e38eda160c97f12442361d1b57701`.

Plancheck reviewed the complete 14,332-byte form with persistent touch labels. It contains public native controls, localization keys, bindings and accessibility identifiers. Its verified SHA-256 is `bcc838fe6ba4a79d42d24870ed7a35aa3f6d03132dc129b86ab5024047e9152d`.

Planinterop reviewed the complete 24,257-byte touch test after it used the native Return or Hide button before switch interactions. The iPhone hierarchy exposed Return as a button. The source uses synthetic study data and its verified public SHA-256 is `fb0b2c9568055b9bd202aef4888ad6c1cb1d9ce53beb51d5fb5f87f924554caa`.

Plancheck reviewed the complete public 14,618-byte form after the empty touch fields stopped repeating their persistent labels. The data bindings, accessibility labels and Mac fields are retained. Its verified SHA-256 is `7365d1732a69f2cc3c08a7394d42727df98171af5278402d058c447c2191dc49`.

Planinterop reviewed the complete 25,175-byte public touch test. Its input helper clears synthetic fixture text around the insertion point and checks that the field is empty. Its verified SHA-256 is `1e27e026a80bd495134fa80bad0a07613202cd3fa00bd46fbf99117e305c4762`.

Planinterop reviewed all 25,615 bytes of the public touch test with actual caret-movement events and synthetic field diagnostics. Its verified SHA-256 is `22267f674a8fb719ccfc675caab34256567c1cc8b18264aafc7c39fe1a9d3e8f`.

Planinterop reviewed the complete 27,617-byte touch test, including the native switch interaction and synthetic simulator diagnostics. Its verified SHA-256 is `b8d679d1313c00539c57989e0789dbca1a2540aa8fdd1a255da133e42b4cda2e`.

Planinterop reviewed the complete 27,956-byte touch test. The simulator test waits for the exact field value after each edit. Its verified public SHA-256 is `aa2ec7c6b55cca107c77a59fbd3b096a445d0d40b217411d2312aed56ee3e64c`.

## Integration privacy review

The independent reviewers read the complete merged files and classified them as PUBLIC.

| File | Reviewer | SHA-256 |
|---|---|---|
| `docs/design/native-interface-contract.md` | Codex privacy_contract | `ce52b837fd80fa41162ebc03cf51f1b1f004ef2c3defd5d6952d7fe003e519a9` |
| `docs/process/verification-status.md` | Codex privacy_register | `015f7fdb8a67bacf74442f6ea44dd43951035426064608b4a2defbe8dea41326` |
