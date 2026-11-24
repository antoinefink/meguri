# Statistics, Browse and Today navigation performance

The batch starts at `7d199588c`. The investigation used the generated 150k collection. Its 149,992 cards and 1,206,529 review-log records exercise collection-sized reads without requiring a copy of the owner’s data.

| Unit | Starting commit | Source commit | Independent cleanroom review |
| --- | --- | --- | --- |
| Browse sidebar filter selection and Today root counts | `352625332` | `7b957cdff` | `s-20261124T133940Z-143b9321`, `s-20261124T134532Z-9cd04dbb` |
| Deck row disclosure | `7b957cdff` | `545f18db2` | `s-20261124T133940Z-143b9321`, `s-20261124T134532Z-9cd04dbb` |
| Statistics box reuse | `545f18db2` | `1a5c201f7` | `s-20261124T134532Z-9cd04dbb`, `s-20261124T135602Z-5d30b93a` |
| Statistics chart bar widths | `1a5c201f7` | `3a11465c1` | `s-20261124T135356Z-f334544f` |

On a populated Statistics page, departure used to construct all 15 box presentations again, and reopening scheduled 15 more before fresh data arrived. The model can now reuse the last prepared presentation of each box through both operations. A fresh request still starts on every appearance. The review caught an equality and hashing regression caused by attaching versions to public part sets. The corrected version confines those versions to presentation invalidation.

Browse now enumerates visible filters only when a command-click needs them. Today reads the badge and heading totals from the roots. Both sections read disclosure from the displayed node with its pending override. Source-level review covered nested limits, the current and kept trees, and failed disclosure writes. Charts compute their bar width once per applicable plot, preserving the existing sections and keyboard and accessibility targets.

Commit `8c6faeb0d` addresses the review finding about locale and time-zone changes. Windows following system preferences invalidate their presentations and pending computations after a notification. Explicitly configured formatters retain their settings. Independent sessions `s-20261124T142356Z-950aa36a` and `s-20261124T142912Z-dd875d99` checked the source, tests and complete public files before admission. The reviewer resolved the PR thread at this commit.

The Rust investigation found three query-reader leases during a warm history request, including a lease held during knowledge calculations. Some Browse and Today queries share that reader. A second independent source review found that releasing the lease early would require preserving or revalidating the collection snapshot during replacement and rollover. Existing measurements do not attribute switch latency to this lease. The fresh request still has 19 parts, and its background cost grows with collection size. The [verification register](../verification-status.md) tracks a separate scrolling-during-refresh failure.

Commit `ca0e8fb91` makes repeated departure with unchanged Statistics histories submit zero saves. Previously each departure resubmitted both histories for serialization and a file write. A busy writer holds one active snapshot and the most recently queued successor per key. A failed write can be retried. A newly delivered identical part still has its own revision and can trigger a save. Independent cleanroom review `20261124T143808Z-88064` checked bounded queuing, eviction, drop epochs and failure handling. The complete-file privacy check passed in `private/20261124T144126Z-96857`.

Integrating `master` at `7ae65ada3` brought in an existing stale privacy pin for the native interface contract. The branch preserves both sets of path-specific admissions. An independent reviewer read the complete 1,538-line public contract in cleanroom `20261124T144437Z-4457`. Its SHA-256 on master is `2ba0dc17770ff4254aa00063825bf2bc29a8f2996b816a8c7e9fbddaa65ff6fc`.

Native compilation found two issues that hosted jobs did not compile. Commit `15c44b1f7` makes the weak reference in the model-release test immutable. Commit `a1a479f5a` explicitly imports the module that defines the deck node used by the Mac shell. Independent cleanroom reviews `20261124T154708Z-35579` and `20261124T154959Z-45773` verified these corrections.

The native Debug run `swift-test/20261124T154735Z-36909` passed all 221 selected tests, including 171 Statistics tests. It exercised each changed package suite, kept and fresh results, locale notifications, disk save coalescing and retries, optimistic disclosure and root totals. The core fixture came from `core-test/20261124T154738Z-38362`. MG verified the Debug core input fingerprint `9b1a3a83baea7f0b88a8743df70a45ae` on the merged source. The first hosted integration run passed all ten selected checks, including Linux selftest and similarity, at [0557f28b7](https://github.com/antoinefink/meguri/actions/runs/45418047695).

The iOS simulator app and test targets built in Debug at `a1a479f5a` (`build-app/20261124T160015Z-81012`). Local static and provenance checks passed from `7ae65ada3` through `a1a479f5a` (`static-checks/20261124T160842Z-13450`).

The Profile comparison uses the same core build from tree `042e7fdbe` and the same generated fixture. Its temporary test hook starts the clock immediately before shell selection and stops on the main-queue callback used by `stats.keptOpen`. The built-in interval begins after `model.appear()` and excludes earlier navigation work. Each run discards the first visit and records three kept revisits. The elapsed interval is a proxy for page entry, with no display-presentation timestamp. The temporary probe stays out of the product. The run receipts, observed times and functional Mac UI results are recorded in [PR #207](https://github.com/antoinefink/meguri/pull/207) as the shared VM completes them.

Cleanroom `20261124T161749Z-56223` traced the regular sidebar’s destination identity. A section switch remounts the Statistics view. Its chart controls also supply keyboard and accessibility navigation, including the Audio Graph and per-bin data points. Further layout work needs a trace of the remount and coverage of keyboard and VoiceOver traversal below the fold.

This work awaits the integration batch gate.
