# Independent review of the spec housekeeping, 2026-11-20

Scope: bringing specs and the plan in line with the built 2026-11-19/20
decisions. `sync/status-strings` T12, T19.3 to T19.6; `sync/meguri-provider`
MP6.3, MP6.9; `sync/cloudkit-recovery` V13.1, V27 (status note);
`sched/app-coordination` F5; `sched/fsrs-policy` A1; `package/file-intake`;
superseded notes in the original requirements plan.

Author: spec-writer session `s-20261120T075032Z-23320005` (branch
`unit/spec-housekeeping`, base `origin/master` c2b24f49c). Reviewer: a separate
read-only agent session that wrote none of the text. It read the specs, the
contract, the plan, and the app and core source on master and on the branches
`unit/round2-meguri-setup-fix`, `unit/round2-tests-fix` and
`unit/icloud-core-merge` to check claims about what was built. It read nothing
under `local-data/`, `refs/` or `oracle/`.

## Passes

1. First pass on 6f0bb93c9: changes needed, 19 findings. Held up: ids stable
   (T19.4 rows 23a and 23b inserted without renumbering), plan lines changed
   only by appended notes, and most built claims (row 1's media test, the
   `localUnusable` refusal, the first-binding history test, `media_usn` 0,
   overtaken and indistinct, the FSRS facts mapping, the Study alert, the
   refused service change and sign-in, no file intake, rows 3 and 6 of the
   dormant iCloud decision).
2. Second pass on 433c22a55: two should-fix findings and two nits. The
   file-intake bullet now names every entry that opens the collection-file
   picker (Welcome, Settings › Import Collection, the Mac's Import
   Collection…), which corrects pass 1's finding 18; the three MP6 lines
   whose text differs from the built app say "text not built"; "started" is
   now "started or joined" in F5.3 and A1.4. Nit 4 (row 23b is a fallback
   the built code never reaches) was accepted as written.
3. Third pass: see the end of this file.

## Findings of pass 1 and what was done

- 1, 2: the upload-only sheet without the "is replaced" line, and the new key
  `sync.needsYou.meguri.localUnusable`, are changes from the built app. Both
  are now marked "not built".
- 3: a job stopped for other work, started by a study entry point, refuses
  that entry point once. Recorded as built (F5.3, A1.4, F5 tests).
- 4: a round stopped for the network does not end the wait. Recorded as
  built (F5.1, A1.2, tests).
- 5: V13.1's note and the T1 intro now say that until T19 ships, Set Up Sync
  opens Account & Sync, whose connect control runs V13.
- 6: row 23b's invented retry-after condition is removed; it is the fallback
  for an op whose Resume is not offered.
- 7, 8, 9: row reference, "above" and "below", and the history rule limited
  to a device that has a collection.
- 10: `finishSetup` now says "any changes", and MP6.9 says that after a
  relaunch no activation is attempted by itself.
- 11: the edits in files with no Status line. `specs/README-format.md` allows
  a Status line only where a requirement asks for review before
  implementation (R2066), and allows no draft note inside a section. So those
  files take the changes when the pull request merges, and the merge is the
  owner's approval. The new T19.3 forms and T19.5 rows say that they follow
  MP6.3's and MP6.9's lines under review; every text that differs from the
  built catalog is marked "not built".
- 12: this file is the cited log. meguri-provider has no requirement id; its
  Status line names Antoine's 2026-11-16 decision instead.
- 13: V27's note says that row 3's confirmation needs T12's
  `sync.connect.continue.*` texts if it is used before T19.8 removes them.
- 14: R1625's note covers "Include media" only; the legacy option stays open.
- 15, 16: notes added beside R103 and R123.
- 17: the R161 note was removed: N18.2 does not clearly cover the empty
  collection's Home.
- 18, 19: file-intake wording; the download-only form's service side.

## Not changed

- `docs/design/screen-inventory.yaml`: the spec-writer role does not write it.
  The needed edits are listed in the pull request.
- Review status: nothing is marked approved. The 2026-11-20 lines of
  `sync/meguri-provider` and `sync/cloudkit-recovery` stay under review until
  Antoine approves them.

## Third pass

On 557bb2324: no findings. The reviewer checked the K1 bullet against
Welcome, Settings › Import Collection and the Mac's Import Collection…
command on master, the three "text not built" marks against the setup-fix
branch, and "started or joined" against `StudyPreparation.prepare(for:)`.
Verdict: clean.

## AI reviewer on pull request 5

CodeRabbit posted four comments. All four were taken:

- F5.2 during the open-time wait: added. A read during F5.1's wait leaves
  the job waiting.
- The MP6.9 table row marked "changed 2026-11-20": now named in the Status
  line.
- `media_usn` 0: now cites `sync/server-media#M3`. Every effective media
  change takes the next USN, the USN never wraps, and tombstones are kept.
- The exclusion released on "neither" (and the premise of "not accepted"):
  recorded as an open point for Antoine in MP6.9. The server contract does
  not bound a late commit after a disconnect. The rule stays as built and as
  approved on 2026-11-19, because changing it needs Antoine and possibly a
  server capability.
