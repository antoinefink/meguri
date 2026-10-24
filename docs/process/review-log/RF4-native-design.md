# RF4 native design preview

Starting commit: `1bb4916106cbfa81a56f3c9a0440fd9f0b3902f9`.

Antoine authorized this bounded implementation and opening the Mac app on
2026-10-24 after the HTML preview. The full application goal stays paused.
The agreed design is `docs/design/native-interface-contract.md` (N1–N10).
Spec session: `s-20261024T111732Z-b3754a3a`.

Scope: native Settings with Connection/Collection sidebar; one Connection
details disclosure containing real sanitized logs; inline connection test;
quiet-corners review, bottom answer controls and native menu shortcuts; simple
deck workspace and empty state. Preserve actual collection and transfer guards.
Do not apply the held file-media export patch or claim media-file export works.

Antoine explicitly deferred detailed verification until the native design is
reviewed together. This is a WIP preview, not RF4 or milestone acceptance.
Run lint, the focused affected Mac build and a bounded smoke/render check now;
retain broader suites, audits, privacy/provenance/similarity and full integration
gate as required acceptance work after visual agreement.

Implementation, independent review, fixes and actual receipts will be recorded
below. No completed-unit trailer is authorized by this checkpoint.

Implementation session: `s-20261024T112011Z-6ad2db2e`.
Filtered run `cleanroom/20261024T112121Z-31871` passed its boundaries and
exported 23 app files. Parent applied the reviewed patch without conflicts.
`build-app/20261024T113555Z-40409` passed UI lint and the team-signed standalone
Mac Debug build. `swift-test/20261024T113705Z-42383` passed 20 focused tests.

Round 1 independent review found no P0/P1. Three P2 items remain: plain export
labels with honest detail copy; transfer decisions in Collection rather than
the Connection setup page; proper pane-height centering. The compact Mac Study
and sidebar composition is now explicit in N2. The existing Sync Logs heading
was confirmed; it is not missing. These changes are WIP pending the fixer and
the running native preview. Full acceptance checks remain deferred as above.

R2 filtered fixer `cleanroom/20261024T114346Z-48138` closed the source review
items. `build-app/20261024T114842Z-53154` passed lint and the team-signed normal
Mac build with zero warnings. Independent R2 source review was clear of P0–P2.
Privacy retained one explanatory-comment match; filtered comment-only run
`cleanroom/20261024T115258Z-58286` removed those two comment lines without
behavior changes. No guard exception was added.

Antoine approved normal quit of the two older previews. Both quit through their
normal application handlers. The new checked product was copied to a stable
preview path; app and widget hashes matched its receipt. PID 59001 opened that
exact product. Direct inspection of its Settings found a duplicated URL label,
an unchanged-address Save action and too much initial transfer setup content.
Antoine requested ongoing direct visual inspection and Apple-quality results.
URL row, a Save action with no draft change, and a long transfer form.
was made during inspection. Full acceptance is still pending.
