# CloudKit publication decision workflow

Status: continuing prepared U113/U151/U161; no unit acceptance.
Component start: 7fb0095a0bb7e57a70aff4b05cb4e9e77eeb1291.
U113 original start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Original integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer session: s-20261103T064254Z-24574bf2.

Master has no completion trailer for U151, U160, U161 or U166. Reuse prepared
ports0677c9af, coordinationc716d9dd and Resume6060d95c. The related prepared
branch is already an ancestor; no separate implementation patch remains.
Preserve the existing lifecycle, preview and native operation ownership.

Direct Mac inspection of the signed source6759694a opens the collection and
retains a completed review after normal quit/relaunch. The publication-choice
sheet shows disabled placeholder actions. Source confirms the gap: no public
Keep Local workflow exists, and the publication reason comment is stale.

The existing contracts define this work: recovery V12.2/V15/V26.2, native
interface N11.8 and the sync.publicationChoice/sync.replaceCloud screens.
No new design choice or policy is needed. Local choices remain available
during preview loading/failure, without an account and while paused. Replace
pushes the native destructive preview; Rust owns its reason and authority.

Complete the U113 core and bridge decision surface, U151 coordination and
U161 screen actions as one coherent workflow. Reuse U160's existing Settings
entry and keep its acceptance dependencies open. U166 first-launch work stays
separate. The capped U114 native-adapter repair is outside this component.
All existing full-gate failures, dependency evidence and final checks remain.

## Publication entry and focused checks

Filtered authoring95771 passed. Independent review found no P0–P2 findings
in its three-file entry/readiness delta. WIP0998b009 records the U151
presentation predicates and tests;811deb4d records the U161 same-sheet
replacement preview. Neither commit accepts a unit.

Focused Swift1882 passed10 selected tests in2 suites. Team-signed standalone
Mac build8332 passed on clean811deb4d. Privacy98339 passed for3 files.
These focused receipts do not replace the pending integration gate.

## Direct Mac publication investigation

The owner approved the specific development-container replacement and later
gave standing permission to stop and reopen Meguri during testing. The
replacement preview did not submit on Return; its explicit action led to
preparation and a later recoverable upload screen. No successful publication,
remote adoption or round trip was established.

Computer use later retained an app surface after the corresponding process
had exited. Resetting its session produced a real fresh launch. Treat earlier
visual and keyboard observations as provisional until repeated against an
independently verified live product. The fresh executable matched the exact
standalone path and SHA256 a7cf9a6ed46eb4afc94b5becd082e5fb0aaa69b43f04764aa616462b0982246f.

Startup evidence:

| Field | Result |
| --- | --- |
| Display | Unknown |
| Call | CloudWorkflow.prepareRuntime |
| Bridge enum | CoreError.Unavailable |
| Fixed-message classification | RuntimeError.IdentityUnverified |
| Debugger | Detached; exited |
| Root cause | Not established |

The first breakpoint attempt produced no diagnosis. The later observation
used a fixed-message whitelist and exported no user values. Local lifecycle
and account-proof ordering remain open. U114 remains unchanged.

Independent source review also identified a presentation coupling: the view
holds its confirmation spinner until the coordinator's whole publication job
returns, including network work after durable acceptance. Sticky historical
operationFailed can appear as a failure of the new attempt. A publication-only
U151/U161 repair is in filtered authoring; Resume/adoption and capped native
ownership stay outside its scope. This is not a claim of an indefinite hang.

## Local information component

Start811deb4d; implementer s-20261103T074233Z-81791508. Filtered export16765
adds read-only local publication facts through Rust, FFI and CoreBridge, without
an account read or confirmation capability. No compilation or test execution
was claimed by its author.

R1 found two P2s in optional tracking information: isolated pending counts omit
local revisions whose dirty mark was lost; the conflict projection decodes
unrelated scopes before filtering. Fixer s-20261103T080958Z-70efb6d7 is repairing
both using the reviewed seed. No local-information source is integrated yet.
Rich preview facts and Keep Local remain incomplete; its proposed mutation
contract remains parked, with no privacy-guard exception or normative change.
