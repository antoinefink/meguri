# Ordinary review round trip — 2026-11-12

Role: fixer. Source master `dd068dff`; Mac signed Debug preview `build-app/20261112T090422Z-96432`; physical normal phone preparation `run/20261112T091337Z-4646`. This is ordinary app observation, not signed-lane or release acceptance. Device, account and connection values are omitted.

Mac: Easy; New 18 → 17; native saved collection records 3; pending 0, in-flight 0, incoming-waiting 0. Quit/reopen: count and session retained.

Phone observation `drive/20261112T093828Z-50587`, after a real Home/return, showed the fresh disposable deck created on the Mac and nine original incoming collection records delivered to Core. New changed from 20 to 17, matching the Mac. Earlier successful manual completion had not received them. The foreground-dependent delay remains unresolved.

Reverse `drive/20261112T094204Z-52209`: 22 steps completed; Easy on the card already inspected, without a grade, on the Mac. Phone To Review 17 → 16; native saved records +3. Mac: New 17, Learning 0, To Review 16; Core received 8 collection records in that process; pending 0, in-flight 0, incoming-waiting 0; completion `1794476746273`.

The Mac was quit normally, its process exit verified, and the exact signed product reopened through Finder. It retained 17 New and 16 To Review. Phone cold-launch `drive/20261112T095550Z-56178` completed all eight steps and retained those counts and Previous Session. No review was added during either persistence check.

UI runs: fail. Findings: cell identifier, disabled button identifier, audio link identifier and ruby overlap. All script steps completed. No accepted gate, complete row comparison or automatic delivery proof.

The existing 2,389 files absent from both local media and iCloud remain a separate content issue. Current fresh-file media verification and the delayed receive behavior remain open. Pause/Resume's later media-read warning, two Host failures, wider recovery review, P24 and the full batch gate also remain unresolved. No Unit trailer, milestone gate or release acceptance is claimed.

## Fresh media on current builds

- Mac normal signed Release: `build-app/20261112T095822Z-61062`, pass. Phone normal signed Debug: `run/20261112T100422Z-69088`, pass, normal launch verified.
- `cloud-media-probe/20261112T100417Z-68799`: prepared-only, 10,240 fresh bytes. Finder copied the exact file into the existing media folder; the installed selection showed 10 KB and the folder had 567 items. No replacement or access prompt occurred. Normal File-menu Sync Now advanced the observed time to 11:09.
- `drive/20261112T101030Z-75702`: all six steps completed; overall fail on two missing identifiers. Core received five collection records. The phone showed 2,390 missing files at that point. First byte read `cloud-media-probe/20261112T101221Z-79885` failed with native-command-failed and returned no verified file.
- The Mac then showed 2,389 waiting files and a sync time of 11:10. `drive/20261112T101336Z-82919` completed all five steps, including one Home/return and Sync Now, in the same phone process. It remained fail on a cell identifier. Phone missing files returned to 2,389.
- After that receive phase, `cloud-media-probe/20261112T101519Z-86174` passed: peer-byte-match, size 10,240, transfer verified true. SHA-256: `bcff84d3690f96e9e86789fa094338460faa306d39ca06c4c7ed98ae3754d3be`. The helper only copied from the phone and compared the original source bytes. No copy to the phone or direct container fallback was used.
- Signed-lane acceptance remains false. This proves the fresh file reached the physical phone; it does not prove all original missing files, automatic timing, complete recovery or release acceptance. Earlier failed observations and UI findings remain recorded.
