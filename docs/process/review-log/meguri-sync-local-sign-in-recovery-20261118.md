# Meguri Sync local sign-in recovery

Starting commit `9a545144b7e9e06639f4e6d46fed7c8e1a970c78`. Product change `b7e87004bed9d4f539e626fd92399d2a9c722201`.

A routine round reads the exact device credential before sending a request. When that item is missing, the app shows its existing Sign In form. A typed local credential error shows the existing Try Again action. Neither result changes the endpoint, collection binding, provider settings, or retained round evidence. Only the current attempt can publish the recovery state.

The filtered export `cleanroom/20261118T130306Z-60434` was independently reviewed against both final files by `owned_diagnostic_review`. The complete public-content review SHA-256 is `8f8e18de19625d24aba67b07dc10590f75fa7bc941dff7d0a753ddd256ce1cd6`. `swift-test/20261118T131048Z-72820` passed all 28 methods of CloudSyncPresentationTests on clean `b7e87004b`. The privacy, static, and scoped similarity checks passed in `private/20261118T130954Z-69492`, `static-checks/20261118T131104Z-77732`, and `similarity/20261118T131146Z-81079`.

The signed Release personal preview `build-app/20261118T131435Z-93766` passed on clean `54a84c440`, which retains the reviewed app source. The copied app and widget signatures and executable hashes matched that receipt.

## Signed Mac journey

The disposable release server package `sync-server-package/20261118T131947Z-8895` passed on clean `2f2eb6bea595933f973faa38b004732d250a06fa` after the web image refresh and server compile fix. Earlier `131049Z-73093` failed the locked frontend audit and `131259Z-88567` was cancelled before packaging completed.

- `ui-test/20261118T132220Z-14466` failed during the signed build, with zero executed cases. Xcode refused manual use of an Xcode-managed installed profile. The configured app-only profile path is still missing.
- `ui-test/20261118T132520Z-20714` built with automatic team signing and reached the VM. The only test failed on app launch. It recorded no sign-in, review, or server readback, and credential cleanup remains unproved.

Keep both receipts and the original integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`. The focused recovery tests and signed preview grant no full-journey or full-gate acceptance.

## Recovering a busy round

This update started on `27c8c472fb58f264c0e7e1b66222f7a26e827a6e` and landed in `cdb07d5b94da3a1c94d397b32c384c47f77efa3b`. If Core refuses to start the collection phase or the media phase after a completed collection phase because it is busy, the app now shows Waiting to Sync and keeps Sync Now available under the existing admission checks. The title reflects a refused start. Only Core can establish pending changes, completion, or uncertain acceptance. Waiting clears the queued wake. A new permitted event or Sync Now can retry. An availability event can be dropped if it arrives before the original wake releases its slot.

The isolated export `cleanroom/20261118T133406Z-39041` was independently reviewed in full by `owned_diagnostic_review`. The report SHA-256 is `d48efe7c68ec2a79872e2be38d1824ea435ef12f9a5d873ab484499d4f8c9b85`. The first privacy scan `133818Z-40876` flagged the presentation test file. After exact public-content review, only that file's existing digest was refreshed. `private/20261118T133907Z-41251`, `static-checks/20261118T133957Z-45005`, and `similarity/20261118T133950Z-42320` passed.

`swift-test/20261118T133950Z-42576` passed all 29 presentation methods on clean `cdb07d5b`. The signed Release personal preview `build-app/20261118T134036Z-42552` passed on the same clean commit. The copied app and widget signatures and executable hashes matched the build receipt. The focused checks cover the real outcome classifier and disabled routes. The full signed Mac journey remains open.

## Showing the last completed exchange

The work started on `1a5e6ecbb32e1e2df06c8906200216d81ddd5907` and landed in `d7544ecf8c46fa8a2a89a9cec024551ea9cd5ff3` after the intervening favicon commit. When a current routine round confirms collection and media completion, the coordinator observes the existing device clock. Settings, the status popover, and the Mac toolbar help share the approved text. Pending work can display an earlier complete exchange. The instant belongs to the current attachment and clears when its binding or readiness changes. It is empty on a fresh process until a round completes.

The initial export `cleanroom/20261118T134822Z-50777` and the toolbar follow-up `cleanroom/20261118T135536Z-56178` were independently reviewed. The report SHA-256 digests are `3c752bccc3ce19896597a8aeed1fc4a9e94ec4acf43d44103543040d681d726f` and `7867e9413deac4e9d28daf844332f21062556500cae6007a8837516fddc11399`. The initial privacy scan `135935Z-63382` found the already reviewed test-file match. Its exact public-content digest was refreshed. `private/20261118T140011Z-64731`, `static-checks/20261118T140108Z-68843`, and `similarity/20261118T140100Z-66000` passed.

`swift-test/20261118T140102Z-66917` passed all 30 presentation methods on clean `d7544ecf8`. The signed Release personal preview `build-app/20261118T140149Z-66913` passed on the same clean commit. The installed app and widget signatures and executable hashes matched the receipt. The focused scope covers presentation precedence and exact formatting. The earlier full Mac journey remains open.

## Retrying on an overlapping availability event

The work started on `0457f16eee8fcf8a3817402e35f5c4cda6ec019f` and landed in `69e54cd92d71f5cf6159dcff06c195fe52cc1169` after the intervening web update. The original wake records a newer same-generation Core Available event while the collection round is returning. A typed pre-start Busy refusal can consume that event and try once more through the same automatic settings and current binding checks. The retry budget belongs to that wake. Media Busy and terminal outcomes cannot use it.

The isolated export `cleanroom/20261118T140750Z-78046` received complete independent review of both final Swift files. The report SHA-256 is `c7b7b8acd01efeeff8da49c9f372f3a3a9f1d874bf38a1424c0fe9903f854f83`. The initial privacy scan `141506Z-88331` flagged the prior test-file match. Its exact reviewed digest was refreshed. `private/20261118T141521Z-88650`, `static-checks/20261118T141558Z-91839`, and `similarity/20261118T141603Z-92364` passed.

`swift-test/20261118T141549Z-89537` passed all 34 presentation methods on clean `69e54cd9`. The four added methods check the actual wake record's single use, generation and sequence, outcome and admission, and cancellation. The focused check does not exercise the signed journey or a review ending without a corresponding Core Available event.

The signed Release personal preview `build-app/20261118T141643Z-89527` passed on the same clean commit. The installed app and widget signatures and executable hashes matched the receipt.

## Retrying when a review finishes during a Busy refusal

The work started on `8f5be15947c3eec991edb3726a4517163fb4c2db` and landed in `be606802fdca002bfa02a5968a8bfa2093b307f7`. The core ends a review only after it drains pending answers and releases review admission. Its SessionEnded event does not require a separate Available event. Both events now share the original wake's single Busy retry. Other events do not gain this retry, and existing automatic settings, provider admission and outcome guards apply.

The isolated export `cleanroom/20261118T142045Z-2488` received complete independent review of both final Swift files. The report SHA-256 is `cbba2611843401e96138d3359019ddac6fe4aaf2009aebac12697d600d0de23c`. Initial privacy scan `142539Z-6362` flagged the existing test-file match. After its exact reviewed digest was refreshed, `private/20261118T142555Z-6852` passed. The static check `static-checks/20261118T142635Z-11334` passed on the product commit range. The similarity check `similarity/20261118T142640Z-11921` passed on clean `77afcdfe9` with the intervening web update.

`swift-test/20261118T142627Z-7913` passed all 37 presentation methods on clean `be606802f`. The three added methods check SessionEnded by itself, one shared retry across both event orders, and unrelated event rejection. They exercise the wake record and event classification without a live provider. The first wrapper stopped at its pre-build commit check when the web update landed. The app and Core trees on `77afcdfe9` are byte-identical to `be606802f`.

The signed Release personal preview `build-app/20261118T142741Z-17717` passed on clean `77afcdfe9`. The installed app and widget signatures and executable hashes matched that receipt. The full signed Mac journey remains open.

## Choosing Sync Automatically for Meguri Sync

This work started on `2e7bdea172667f5b8a1d74fc03362ca458390317` and landed in `7e2df75dfb509d6c8621030910ca04db4d6eff24`. A ready Meguri binding shows the native Sync Automatically toggle in Account & Sync. It uses the existing label and footer defined in `sync/status-strings#T14`, with behavior from `sync/meguri-provider#MP5`. The setting uses the existing device-local Core save. Once off, automatic scheduling stops and Sync Now remains available under the usual admission checks. A successful change to on requests an ordinary automatic wake.

The isolated exports were `cleanroom/20261118T143303Z-22932` and `cleanroom/20261118T144037Z-26079`. Review identified that a Core settings save closes ordinary admission, including for an automatic-only change. The toggle is unavailable while the original wake owns work, and new rounds cannot start while the settings save settles. Pause keeps its existing drain. Complete independent review of the four final Swift files is recorded with SHA-256 `43f53d0805e541bb9478df1522b86c1d12fc7fa94f8b0bbd3e9f5782c2efaa64`.

Privacy scan `144535Z-31095` flagged the existing test-file match. Its exact complete reviewed digest was refreshed. `private/20261118T144553Z-31509`, `static-checks/20261118T144635Z-34755`, and `similarity/20261118T144638Z-35664` passed.

`swift-test/20261118T144624Z-32580` passed all 41 presentation methods on clean `7e2df75df`. They cover the scheduling predicate, ordinary route availability and refusal without a ready binding. The signed Release personal preview `build-app/20261118T144708Z-32570` passed on the same clean commit. The installed app and widget signatures and executable hashes matched that receipt. The signed Mac journey and exercising this toggle on an actual ready test-server binding remain open.

## Daily review and packaged server checks

The current Mac native journey `ui-test/20261118T145651Z-59551` passed its one selected method, `LocalReviewJourneyTests/testStudyRevealFourRatingsUndoWaitAndRestart`, on clean `274b4f88fa758d90d9b5028648413dffeaa650e9`. The app ran in the dedicated Mac test VM with generated data. The journey exercises reveal, all four ratings, undo, the waiting state and persistence through app restart.

The production server package `sync-server-package/20261118T145512Z-50357` passed on the same clean commit. With that exact receipt, `meguri-provider-real-server/20261118T145801Z-65061` passed six phases. It bootstraps two independent CoreHost clients, saves a real Good review in the first, transfers the review and updated media, and checks the same daily rows and media on the second after cold reopen. It uses a disposable service and generated accounts. The first launch wrapper stopped before running Meguri because the intervening web commit advanced master. No failed runtime check was rerun.

The two jobs establish the native local review path and the packaged server's separate CoreHost path. The combined signed app-to-server journey and live use of the Automatic toggle remain open. The configured profile file for the dedicated Mac test VM is unavailable.

The same native method passed once on each simulated device in the dedicated VM. iPhone receipt `ui-test/20261118T145940Z-71735` and iPad receipt `ui-test/20261118T150207Z-71727` are clean at `16ccabb4654f44502f9073932aca740cd9b4ceea`. The intervening verification record leaves the app, Core and server trees byte-identical to `274b4f88`. These three local journeys and six CoreHost phases do not close the original integration gate.

## Collection entry and signed Mac follow-up

The existing Mac native journey `ui-test/20261118T151016Z-89490` passed its one selected `CollectionEntryJourneyTests/testNativeExportCancelSaveReplaceReimportAndDurableReview` method on clean `972e27aa6b92a2fca87e9165de7f135e0fe41211`. It uses the production picker, generated collection data and the dedicated Mac test VM. It checks import, export cancel/save/replace, reimport and a review that survives app restart.

The configured development profile was restored and matched Xcode's installed copy. Signed `ui-test/20261118T151451Z-97617`, on clean `972e27aa6` with exact AMD64 package `sync-server-package/20261118T150644Z-82900`, reached native XCTest. Its one case failed at the first replacement disclosure assertion after sign-in and opening the actual preview. The retained hierarchy shows those native StaticText rows expose their text as accessibility values. The test had searched labels alone. The receipt also reports cleanup pending, so it supplies no credential-cleanup proof.

Filtered correction `cleanroom/20261118T152454Z-8221` changed only the locator in the signed test. The test still requires all four exact disclosures to exist once in the real preview. The independently reviewed complete file SHA-256 is `921f49f62f649ec2ed1cd3085d513440abb461a1cd48ae03a0a2f815672f1e1d`. The correction landed in `6de3f21e4286b9a5be3b8c51c2fd6e2e108c9288`.

The correction passed `private/20261118T152639Z-10543`, `static-checks/20261118T152729Z-13080` and `similarity/20261118T152743Z-13765`. AMD64 attempt `sync-server-package/20261118T152712Z-11961` stopped before XCTest when the cached frontend dependency lacked the x64 Rolldown binding. The native package `sync-server-package/20261118T152810Z-14347` passed on the correction commit.

Signed `ui-test/20261118T152946Z-14343` on clean `6de3f21e` passed the setup disclosures and proceeded through confirmed bootstrap, Study, Reveal, Good and Close. Its single case then failed when the test compared the label of the unique recorded-answer element with the exact one-answer caption. The passing native local review test reads the same control's label or value. The signed receipt again reports cleanup pending.

Filtered correction `cleanroom/20261118T153457Z-29116` kept the exact caption on the same unique element and accepted either field. Final file SHA-256 `e7801959c3ce0d82e99fd2aa336ecf2215008df475cd86cfde8162dff958fb14` passed complete independent source review. Commit `7b155c317c10f037d6ee38f2dc21a2f9cf9c642a` changes only that assertion.

The answer-caption correction passed privacy `153627Z-30102`, static checks `153715Z-33176` and similarity `153717Z-33601`. The new exact native image package `sync-server-package/20261118T153700Z-31474` passed on clean `7b155c317`.

The signed Mac UI receipt `ui-test/20261118T153838Z-31470` passed its one
selected case on clean `7b155c317` with that exact packaged server. It exercised
sign-in, the explicit upload choice, Study, Reveal, Good and Close, then used
Sync Now and observed a fresh successful round. The public
`owned-synthetic-card-good-committed` marker required actual server readback of
the same one synthetic answer and checked cleanup. The manual action can
coalesce with automatic work. The receipt does not ascribe delivery to either
trigger. It does not cover opening the app without a local collection.

Opt-in diagnostics landed on `47cc21b45` after a complete seven-file public
review. The existing native normal, full, authorization and session checks
passed 48 methods with zero skips in `sync-server-test/20261118T160635Z-95217`.
The new case drives the actual server process and checks all three log levels,
milestones, exact integer stamps and absence of synthetic private inputs.
Privacy `160515Z-92007`, static `160633Z-94488` and similarity
`160634Z-95017` passed. The AMD64 preparation `161044Z-97530` stopped on
the old Compose fingerprint without starting npm or the package test.
The complete policy review on `b02daad32` verified that only this runtime
Compose fingerprint changed. Privacy `161421Z-1689` and static
`161459Z-2899` passed on the exact change.

The signed journey test change `a4da1fff3` moves the manual round check
before Study and checks the native Automatic setting. After Good and Close it
observes a fresh round without invoking Sync Now. The unchanged MG producer
still requires the exact persisted answer and cleanup. Complete independent
review cleared file SHA-256
`dbc79cecca8d6f66cb7192930d16f3e03112ec0411ef7871ddf83949699bf3d2`.
Privacy `161705Z-4299`, static `161741Z-5489` and similarity
`161741Z-5536` passed on the changed sources.

## AMD64 release image with diagnostics

On clean `a4da1fff3`, the AMD64 package
`sync-server-package/20261118T161946Z-9612` passed. Its fixed source manifest,
runtime image and exported inventory match, including the x86-64 executable.
The encompassing `sync-server-package-test/20261118T161945Z-9082` stopped at
browser seed before the protocol or restart checks. The deployed service was
not changed.

The first different-platform check `sync-web-verify/20261118T162951Z-18523`
passed nine frontend cases, then stopped before Chromium because the global
ARM64 override also constrained the retained AMD64 runtime. With that override
unset, `sync-web-verify/20261118T163305Z-26020` passed all ten cases. It used
the host-native ARM64 browser tool, the same exact AMD64 runtime and linked
input and inventory digests, and confirmed owned cleanup. This establishes
the exact image's browser journey. The earlier combined package smoke and
its unrun protocol/restart checks keep their original outcomes.

## Automatic delivery after a signed Mac review

The first current attempt `ui-test/20261118T162502Z-12721` stopped before
XCTest at approved profile validation. The configured Downloads file was
absent. The single byte-identical cached Xcode profile matched the prior
signed CloudAgent Mac app. The successful invocation scoped that existing
cache path to the one command and used the same validation. Both receipts
remain recorded. The configured Downloads path still needs attention for
future invocations without that override.

`ui-test/20261118T162825Z-15174` passed its one selected signed Mac VM case
on clean `a4da1fff3` with exact AMD64 package `161946Z-9612`. The app
completed bootstrap, performed a manual round before study and exposed
Automatic as on. After one Good answer and Close, it observed another
successful round. The test invoked no manual sync after Good, then signed
out and quit. The unchanged wrapper verified the same answer on the
disposable server, completed its cleanup, and returned
`owned-synthetic-card-good-committed`.

This verifies delivery through an automatic opportunity with the setting on.
The specific trigger, changing the toggle on a live binding, fresh-device
download through the signed UI, and the full integration gate remain open.
