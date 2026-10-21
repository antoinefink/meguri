# Session ownership

- `held` is the E2 compatibility owner, not production C2/C5 admission. It reuses U41 gather and U44 application and keeps no shown-card state.
- Queue restoration attaches to U44 AnswerMetadata and U25's existing registered AnswerRestorer. Never register one generic restorer per build: every surviving registration would restore the same step again.
- Never hold a presentation mutex while waiting on a collection operation. Writer callbacks only change owned values; they never wait for another operation.
- C5 `service` owns production admission and shown-card state. Its file worker owns no collection handle. Prepared receipts acknowledge before the executor commit lock; Done requires actual WAL coverage. Keep replay opening-only and recorded-time; current D8 remains the last open step.
- Observer work only captures row images or wakes the existing ordered-stream subscription. Never wait for the review worker from a writer callback, or take the service state mutex there.
- Session C6 uses Review and the native platform at peek/start/renderHead; a different platform is refused. U62's owned capture supplies both key and payload, including on cache hits; never attach a new render snapshot to a prior key.
