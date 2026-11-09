# Final validation under held Runtime State

- Role: fixer. Starting commit: `2881beec`.
- Test-only follow-up to the passing held-Driver regression. Uses the existing
  real Host/start/registration fixture and Rust-only Runtime State hold hook.
- Positive mpsc observation of the original State lock precedes the validation
  worker. Exact successful validation must arrive within the existing five-second
  completion bound before release. The scoped release guard drops before joining;
  both workers join and the real Host closes before the expected RED assertion.
- Added a full nextest test-thread reservation; existing tests and limits remain.
- No production changes, new exports, synchronous operation-activity requirement,
  provider/Swift callbacks under the lock, or replacement-owner sampling. This
  isolates original Runtime mutex contention, not Swift SDK enqueue/Started proof.
- Expected RED on current code, retained pending a separately reviewed repair.
  MG execution and independent review belong to the root integrator; no test run
  or observed RED result is claimed by this filtered fixer session.

Independent review finds no P0–P2 issue in the test or its teardown. Core
`53211` observes the hold, then fails the pre-release completion check after
five seconds. Both workers finish and Host close completes before assertion.
This RED is retained and blocks acceptance. It does not establish the full
Swift invocation path or change production behavior.
