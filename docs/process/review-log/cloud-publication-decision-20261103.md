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
