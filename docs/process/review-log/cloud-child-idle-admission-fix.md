# Child idle command admission fix

- Role: fixer under implementer restrictions. Starting checkout commit
  `354c7139d156ec6794a8e1b836d63f41556c991b`; complete supplied candidate from
  `d9df5e6ce59a6c60de93eda65f1dc208d745acf7` preserved. Original batch base:
  `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Reported combined run `core-test/20261031T165654Z-92377`: 158 cases ran;
  157 passed. The sole failure was
  `ordinary_ops_take_at_most_500_actual_entries_without_losing_the_tail`:
  receiver `hold` at `cloud_incoming_intents.rs:1147` returned BrokenPipe after
  36.82 seconds. Before the receiver sends its first command, the sender validly
  creates and syncs 251 Note/Card pairs. The idle receiver read timed out at
  30 seconds.
- Only outer-loop command admission now waits without a timeout for its first
  byte. The previous read timeout is restored before checking first-byte EOF/error
  and before reading the remaining header/body. Both frame readers share body
  decoding. Boot, parent responses, writes, provider/operation reads, IPC timeout,
  frame size, WAL/operation bounds and command count are unchanged. EOF still
  unwinds the host; parent Drop still kills/reaps.
- One private socket test covers partial-header/body timeouts with a 50 ms socket
  timeout, EOF before/during a frame, successful decoding and restored timeouts.
  No sleeps or long timeout regression added. The existing 502-entry case remains
  the actual red evidence. Existing source ownership/lifetime and native tests
  are intact. Scope: `child_driver.rs` and this note only.
- One source/callsite consistency pass: scoped diff whitespace and frame-reader
  callsites. No builds, tests, formatter, network or commits in this runner.
  Root owns the actual failed case, memory/native tests, independent review,
  lint/privacy/similarity and required gates from the original batch base.
  No Unit or full-sync acceptance claimed.
