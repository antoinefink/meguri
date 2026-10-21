# Session ownership

- `held` is the E2 compatibility owner, not production C2/C5 admission. It reuses U41 gather and U44 application and keeps no shown-card state.
- Queue restoration attaches to U44 AnswerMetadata and U25's existing registered AnswerRestorer. Never register one generic restorer per build: every surviving registration would restore the same step again.
- Never hold a presentation mutex while waiting on a collection operation. Writer callbacks only change owned values; they never wait for another operation.
