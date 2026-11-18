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
