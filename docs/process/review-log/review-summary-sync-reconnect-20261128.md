# Review summary and sync reconnect

The units started on `b6d68acb6`. Implementation used session
`s-20261128T062417Z-6d6e7872`. The review summary retains Done and its keyboard
behavior. The finished normal deck page and the deck menus offer Custom Study.

The report showed a network failure followed by a refusal before collection
work began. The exact refusal was unavailable. Swift had treated transient
pre-start refusals as terminal failures, disabled Sync Now and shown setup
copy despite a ready binding. The patch keeps interruption and storage
refusals retryable and treats timeouts as network failures. Core still checks
admission before each round.

## Independent review

Session `s-20261128T063702Z-2df9006c` found that the first candidate could
reopen admission during a pending settings change. The revision removed those
Core changes and added a coordinator test through Sync Now and reachability.
The follow-up review `cleanroom/20261128T064749Z-44977` found no actionable
issues. The first review checked both complete session records and found
public operational metadata only. Their SHA-256 digests matched
`7a9b05d2b7519c1c46c07afc1a588e13577430212a3651c7d7172272482e6c21` and
`d27cd2128f582e09f1025e4caff0218440070d58a2b8065e76d62d69af848010`, respectively.

## Focused verification

- Core `20261128T064333Z-40402` passed all 23 provider-normal cases. The
  new case recovered from a dropped request and a temporary storage refusal on
  the original Host and binding. Collection and media both finished, and the
  provider document stayed byte-identical.
- UI lint `20261128T063102Z-31000` and design documents
  `20261128T064417Z-40962` passed.
- Swift `20261128T071133Z-60699` reproduced the disabled Sync Now route
  with the previous refusal policy. Swift `20261128T071537Z-65364` passed
  all 59 selected tests with the fix. The compiler import repair only narrowed
  the generated DTO imports in the new test.
- The Mac filtered-study journey in `ui-test/20261128T063051Z-30365`
  passed. It reported the review summary, confirmed the absent button, and
  used Done. The same deck still offered Custom Study, and its preview
  preserved home schedules.
- The iPhone simulator build passed in `build-app/20261128T071857Z-68916`.

- All 28 iPhone drive steps in `20261128T073654Z-91064` completed. Four
  answers led to a summary with Done and no Custom Study action. Done
  returned to the same deck. The drive reported overlap among card labels
  in the accessibility hierarchy. The captured summary has no visual
  overlap.

## Inherited UI test failures

`LocalReviewJourneyTests/testAllEasyExhaustionAndRestart` fails at the
same assertions on the candidate and the untouched branch point. The Mac
relaunch waits for `home.session.recorded` before it opens the deck
(candidate `20261128T071511Z-63744`, branch point `20261128T073011Z-84672`).
The iPhone journey expects a hidden sync button before the first answer
(candidate `20261128T072539Z-77539`, branch point `20261128T073403Z-88034`).
The focused checks exercise the changed review summary and sync behavior.
