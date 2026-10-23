# RF3: Collection-only file export

## Scope and starting point

- Starting master: `64701ca6eaf6d660eb4246b6faa31a955fddb659`.
- Deliver the latest-format whole-collection `.colpkg` file without media through native Mac Save and iPhone/iPad Files. Reuse the existing live package writer, review flush barrier and bounded job ownership. File export remains available offline and without an Apple or server account.
- This covers parts of U124 and U162. Media, legacy formats, deck/selection options and the rest of those units remain open. No `Unit:` acceptance is claimed for a partial delivery.
- Explicit self-hosted Anki-compatible import/export remains next after file export. Routine CloudKit sync remains separate.
- Carry the original RF2 integration base `81c207573c20c337c8834e4ae8b4ee8f29603dbf` into the full native gate. The original RF2 full gate remains failed; its known failures have passing focused follow-ups and its required T1 checks pass. The next full gate must cover all pending RF2 and RF3 changes.

## Spec and implementation

- Spec-writer session: `s-20261023T001251Z-3706ebdb`, recorded on master as `c82e0aaa`.
- The new contract describes the current writer and job interfaces. Code work and its checks follow the spec commit.
- Spec committed as `a7267243`: `package/file-export#E1` through `#E6`. Privacy matches in new prose were removed by rewriting the complete sentences; the contract and scan rules are unchanged.
- Ready export artifacts must stay outside temporary namespaces swept on restart. Flush on the worker before snapshot acquisition; keep the FFI starter short. Retain existing reader leases through writer completion and drain them before replacement can close the collection.
- Keep export files on disk across FFI and native handoff. Do not materialize the package in Swift `Data`, `FileWrapper` or a `FileDocument` snapshot. Preserve existing destinations until verified atomic publication.

## Retained UI work

- RF2 critic findings remain open in U152/U166. Apply the small native iOS inline import-title correction with this UI work and obtain fresh evidence for the affected large-text cells. Keep the full title.
- Home shell evidence and the other full-screen findings remain recorded. No complete Home or import visual acceptance is implied by the local review journeys or this export delivery.

## Source delivery and independent review

- Implementer session `s-20261023T001829Z-cd534ca1`, source `ca13993d`, exported own-code patch in cleanroom run `20261023T001929Z-24460`. The runner passed its boundary checks; it ran no compilation or tests.
- Independent round 1 found five issues: reverse path overlap could put backups in owned output; capacity preflight followed directory creation; strict route expectations were missing; unused Ready output could block exports after restart; native Files selection needed replacement serialization.
- Fixer session `s-20261023T004303Z-09daf314`, clean source `da1b62ce`, used the first patch as seed in cleanroom run `20261023T004631Z-33999`. Added canonical overlap checks, reused writer preflight, strict route updates, durable ownership states, native handoff admission and focused regressions. A separate real-file memory test uses the existing sampler.
- Independent round 2 confirmed the original five source fixes. Two P2 issues remain: queued cancellation can leave native completion non-retryable; cleanup can lose its ownership evidence before a failed final sync. Preserve those findings until fixed and checked.
- Native rendering and actual Files/Save behavior remain pending. No parent unit is accepted by this source checkpoint.

## Focused checks on the first integrated patch

- Docker formatting `20261023T005912Z-37858`: pass.
- UI lint `20261023T005945Z-38518`: pass, 115 files.
- Docker focused export tests `20261023T005942Z-38271`: failed, 13 passed and 1 failed. Reverse-nesting coverage attempted to open a configuration already rejected by the host's collection/backup separation check. Preserve the earlier refusal and correct the test to assert its safety at that boundary; the other alias cases still require export-level refusal.
- This checkpoint is WIP with known findings. The full gate, native journeys and measured memory result are not yet claimed.

## Targeted round 2 source fixes (checks pending)

- Fixer session `s-20261023T010207Z-73726269` resumed the supplied formatted implementation and round 1 fixes. The starting master and required full-gate base above remain unchanged; no build, test, scan or commit ran in this filtered session.
- Native completion now captures a terminal guard that restores retryability when its worker is cancelled before entry, dropped, rejected, fails or unwinds. Durable safe ownership still precedes releasing native admission. Swift retains failed completion leases and exposes Retry Export Cleanup without changing a completed save into a failed or cancelled publication.
- Artifact cleanup retains its validated directory identities and narrow unlink/sync phase through errors. Retries recheck protected paths, reject substituted directories, symlinks and unknown files, and can finish the parent sync after their owned directory has disappeared. Added focused regression source for queued cancellation/suspend, completion error/unwind, unlink/sync retry, idempotent discard and a subsequent export.
- Reverse-nesting coverage now expects host admission to refuse backups containing the collection; all other alias configurations still reach export admission. Both boundaries preserve source/user content and assert no owned output creation. The two reported own-source comment regions were rewritten without consulting scanner inputs or changing scan policy.
- Integrator verification remains required. Preserve the earlier focused failure and privacy failure until fresh checks resolve them. Full/native/RSS evidence and unit acceptance remain pending; the full gate must start at `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## Integrated source checkpoint

- Independent review round 3 confirmed both round 2 findings fixed and found no new P0–P1 issues. Actual native behavior and rendered visual review remain pending.
- Focused check-correction session `s-20261023T011343Z-a99f7798`, cleanroom `20261023T011403Z-45572`, corrected three event borrows in regression tests and removed an optional Swift comment. Executable Swift statements are unchanged.
- Docker format `20261023T011133Z-43852`: pass. The first fresh test build `20261023T011202Z-44223` failed on the three test borrows; that failure is preserved.
- Docker focused export suite `20261023T011520Z-46607`: pass, 18 tests. Includes real backup preservation, capacity refusal, canonical aliases, captured reviews, native admission, cancellation/unwind, restart ownership and cleanup retries.
- Standalone memory check `20261023T011632Z-47651`: pass, one test with no skips. A real sparse 1 GiB file traversed the production destination preflight/copy/hash/publication path. The sampler enforced less than 128 MiB peak growth; length and digest matched. This is Rust file-save evidence, not native Files-provider memory evidence.
- Privacy `20261023T011506Z-46313`: pass for staged source. Earlier failures `20261023T010121Z-39447` and `20261023T011205Z-44480` remain recorded; optional own-source comments were rewritten or removed. No scanner rule changed.
- Similarity `20261023T011523Z-46857`: failed, one structural flag in `core/crates/meguri-engine/src/lifecycle/state.rs`, lines 84–108. Only the sanitized failure was read. Rewrite this region from Meguri's lifecycle contract; no exemption or acceptance is claimed.
- Native bridge checks are in progress. This is a WIP source checkpoint; no full gate or parent unit acceptance is claimed.

## Native compilation and lifecycle correction

- Native checks `20261023T011717Z-48107` and `20261023T011841Z-49969` failed compilation. The first used an array where generated digest metadata requires 32-byte Data. The second caught explicit task-capture and returned-switch syntax in the export coordinator. No native behavioral pass is claimed for those runs.
- Fixer `s-20261023T011846Z-a530e25d`, cleanroom `20261023T011917Z-51403`, corrected those three source issues and rewrote the flagged lifecycle region from Meguri's contract. Operation declarations now keep names and admission flags together through an exhaustive match; all names and flags are preserved.
- Independent round 4 reviewed the small correction and found no P0–P1 issue or deterministic check blocker. It confirmed admission semantics and Swift callback ownership are unchanged.
- Docker format `20261023T012214Z-52791`: pass. Lifecycle tests `20261023T012227Z-53096`: pass, seven tests with no skips.
- Similarity `20261023T012227Z-53095`: pass against the original RF2 base. The earlier similarity failure is resolved without an exemption; no reference source was read.
- Privacy check `20261023T012325Z-54661` passed. Native bridge run `20261023T012322Z-53917` passed its focused tests. Mac and simulator code compiled with no warnings; the tests covered metadata, cancellation, destination lifetime and save outcomes.
- Settings/AppModel tests, actual native destination journeys, rendered accessibility/visual checks and the full gate remain pending. Full gate base remains `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## AppModel and native adapter follow-up

- `20261023T012419Z-55531`: failed test; entry kept its prior stage. `20261023T012521Z-57306`: failed build; missing CoreBridge import. No UI test ran in that job.
- Session `s-20261023T012731Z-5caf16a7`; cleanroom `20261023T012744Z-60066`: added CoreBridge import. Test now checks no new entry, same stage, generation and lease, refused close and later saved output. Entry code is unchanged.
- MeguriKit `20261023T012918Z-60871`: pass, four tests. Coverage includes actual export/reimport/review, blocked replacement, no collection, and all three cleanup retry cases.
- An earlier session append `20261023T012611Z-58616` failed its privacy hook and was never used by a runner. Only that uncommitted append was removed. The fresh minimal session record above was committed through the normal tool; existing ledger rows and privacy rules were preserved.
- Next: Mac Save, iPhone/iPad Files, accessibility, image review and full gate from `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## Native Settings checks

- Mac `20261023T012954Z-61946`: failed before export; Settings row click did not open the section. Fixer `s-20261023T013335Z-6b330f3f`, cleanroom `20261023T013356Z-66886`, added real section Buttons and kept shared navigation, tags and selected state. Independent round 5 source review found no P0–P1 issue; images remain pending.
- iPhone `20261023T013332Z-65174`: failed before Settings. Real import, review and restart passed. Native tabs exposed labels with no app IDs after restart. R1964 remains open for the shell. Strict fixture ID tests are unchanged.
- Commit `2d04309c`: native navigation test correction. R1964 open.
- Mac `20261023T014104Z-70595`: Settings opened, but Form put the root ID on its child buttons. No export had started. Fixer `s-20261023T014415Z-259c723e`, cleanroom `20261023T014438Z-73254`, added a container around the Form children; labels, actions and layout stay the same.
- Source includes real route readiness, existing folder selection in Mac Save, and expected saved-file checks. Save/Files, image review and full gate are still pending. Do not accept U150, U152, U162 or U166 from this source checkpoint.

- Mac `20261023T014555Z-74003`: real Save panel cancellation passed. Save failed and retained the prepared export. VM sandbox logs identify denied export temporary sibling creation; destination staging needs an Apple-platform correction. No native save pass is claimed.

## Sandbox publication correction

- Fixer `s-20261023T015555Z-2b662a2e`, cleanroom `20261023T015640Z-79691`, source master `4e67eb02ba54778083c557aebbd83bc77d55375c`. Boundary checks passed. Native checks remain pending.
- Spec `52e7316e` permits authorized staging on the destination volume. Rust checks staging and final paths. Native publication creates a new file exclusively or replaces an existing file through Foundation. Uncertain results retain recovery files.
- Fixer session is recorded in `4e67eb02`. Cleanroom boundary checks passed; native checks are pending.
- Independent round 6 source review: no new P0–P1 findings or deterministic blockers. Native and visual evidence remain pending.
- Docker format `20261023T020920Z-84629`: pass. Focused export tests `20261023T020954Z-85161`: pass, 19 tests. Privacy `20261023T021045Z-85798`: pass after whole-sentence log rewrites; earlier failure `20261023T020954Z-85121` is preserved.
- Native `20261023T021111Z-86248`: production compiled; test compilation failed on an async thread assertion. Fixer `s-20261023T021214Z-66c2fd6c`, cleanroom `20261023T021235Z-88520`, removed that single assertion. Synchronous publication checks remain. Native results are pending.
- Similarity `20261023T021132Z-87399`: pass against the original RF2 base.
- Native CoreBridge `20261023T021528Z-91146`: pass, 12 focused tests plus seven interval tests. Includes real Foundation staging and replacement, exclusive creation, cancellation, uncertain results and coordination lifetime.
- MeguriKit `20261023T021717Z-92930`: pass, four tests including real saved-file reimport and subsequent review.
- Mac `20261023T021236Z-88708`: Save, Replace and Cancel passed. Import did not open. Native follow-up pending.
- Mac `20261023T021906Z-94909`: pass, one test and no skips. Actual import/review/restart, native export cancellation, Save, Replace, reimport, another review and restart all passed. iPhone/iPad, rendered accessibility, critic and full gate are pending.
- iPhone `20261023T022347Z-97809`: failed before Cancel; native picker exposed that control as Other. Import, review, restart and export preparation passed. Fixer session is recorded on master; cleanroom `20261023T022801Z-1529` corrected Cancel and folder-title selectors. Native Files behavior remains pending.
- Mac accessibility `20261023T022820Z-1981`: failed on the indeterminate spinner role expectation. Cleanroom `20261023T023039Z-5686` changed that single assertion to activityIndicator. Labels, target sizes and system audits remain required; the fresh result is pending.
- iPhone `20261023T022952Z-3748`: Cancel existed but readiness failed. Cleanroom `20261023T023520Z-9657` adds post-wait picker capture and native control attributes. This is diagnosis; no Files pass is claimed.
- Mac accessibility `20261023T023306Z-7416`: spinner checks passed; system audit reports contrast for sidebar No decks in all six export cells. This finding remains open.
