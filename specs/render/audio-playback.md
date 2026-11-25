# Native card audio queue

Spec: render/audio-playback
Requirements: R547–R552, R554–R560, R1020, R2038, AE16, AE18, AE19
Anki: 26.09.3 (AV numbering and queue interoperability); session/platform policy follows the Product Contract
Scenarios: none. Native player and WebKit tests use synthetic AV lists and generated media.

U67 supplies one native audio controller to a card surface/session; U175 sends review events, and previews use the same policy with their own card identity. AV extraction and FrontSide behavior are `render/av-tags`; `render/media-serving` owns file lookup, page identity and message admission. Native `[sound:…]` playback is independent of the WKWebView, so a page swap alone never cuts a clip.

## A1. Inputs, settings and identities

A card payload carries separate ordered question and answer AV lists and its card/prepare identity. Use the rendered lists, not a second scan of fields or displayed HTML. A TTS item retains its original list index but is silent. Question replay markers inside the answer's FrontSide select the question list and do not become answer entries.

Use the card's **home deck**: original deck id when the card is in a filtered deck, otherwise its current deck id. Read that normal deck's preset through the existing deck-config view; a filtered deck does not supply a substitute preset. Missing preset/default handling follows `decks/deck-presets#P3`. Do not write a config value just because playback reads a default.

| Policy | Source / default | Meaning |
|---|---|---|
| autoplay | preset `disable_autoplay`, false by default; legacy `autoplay` is its inverse | side presentation automatically schedules that side's list |
| skip question on answer replay | preset `skip_question_when_replaying_answer`, false by default; legacy `replayq` is its inverse | answer replay uses only answer entries when true |
| interrupt on answer/next card | collection `interruptAudioWhenAnswering`, true by default | whether a side/card transition stops the current clip |

`interruptAudioWhenAnswering` is read with the strict boolean read of `sched/config-keys#G1` (`review/review-preferences` P2) and written, in Meguri's interface, only by Settings › Review (`review/review-preferences` P4).

The controller accepts already resolved typed policy values; caller/database parsing belongs to existing config APIs. Autoplay controls automatic native AV selection and, on the review pages, whether card HTML audio/video may start without a user gesture (A8). It never blocks manual replay. A setting change affects later commands; it does not restart a current clip merely because the setting changed.

Every queued entry retains its card, side and original AV index. Native player completion/load callbacks carry a playback generation so a stopped item's delayed callback cannot advance or terminate a newer queue. A deliberately surviving old clip under interrupt-off keeps its own identity until it ends; it is not relabeled as the new card's clip. Closing the owner stops playback and releases callbacks/resources.

## A2. Automatic side transitions

Pre-rendering prepares resources but emits no audible native playback. Schedule automatic tags only when the side becomes presented, once for that side presentation; duplicate `shown`/paint confirmations must not replay it. The answer uses only its own list, never question tags inserted through FrontSide. The next card uses its own question list.

A reveal or next-card transition replaces the **waiting** queue from the previous side/card. With interrupt on, stop the current clip too before starting the new list. With interrupt off, allow only the current clip to finish, then start the new list; do not keep old waiting clips ahead of the new side. If autoplay is disabled or the new list has no audible entries, waiting old clips are still removed. Interrupt-on still stops the current clip; interrupt-off leaves that one clip alone.

One identity covers departure and presentation. Process its stop/queue reset once, before the new list is installed. Interrupt-on may stop at departure while the next side is still preparing. Start new audio at presentation, never at idle prepare. Each later transition replaces the waiting list again.

A native clip may therefore outlive its card's page becoming idle when interrupt is off. This is distinct from HTML media, which must pause on swap (A8). Answering, reveal and next-card ordering use the native controller rather than pausing its audio as a side effect of view reparenting.

## A3. Replay and one-entry playback

Replay is an explicit stop-and-replace operation, regardless of the interruption setting:

| Current side | New replay list |
|---|---|
| question | question entries |
| answer, `replayq` true | question entries followed by answer entries |
| answer, `replayq` false | answer entries only |

Replay starts at the beginning, also when the current clip is paused. Manual replay is allowed with autoplay disabled. An empty/silent-only replay list stops the old queue and completes silently.

A valid replay-button command `play:q:N` or `play:a:N` stops the current clip, discards all waiting clips and plays **only** the selected entry from the named side of this card. It never continues into the remainder of that side's list. This applies to question buttons shown through FrontSide on the answer. If the selected entry is TTS, the old queue is stopped and no sound starts. Unknown/out-of-range/invalid commands are rejected by the bridge before reaching this operation and do not stop useful audio.

Explicit stop behavior here follows R550/R551. It is stronger than applying the configurable automatic-transition policy to every possible command. Do not reuse interrupt-off as a reason to delay a user-requested replay.

## A4. Pause, resume and seeking

Pause/resume acts on the current clip only. Pause retains its time and waiting entries; resume continues that clip rather than restarting it. With no current clip the commands are no-ops. A manual replay or accepted transition supersedes the paused queue under A2/A3. Interrupt-off does not silently resume a paused old clip to make the next list start: explicit resume remains required.

Seek affects the current clip (review: −5s/+5s). Clamp to zero and, when duration is finite, to that duration. Keep pause state. Reaching the end can complete the clip once; it cannot wrap, restart or move within a later clip. Refuse seek while the timeline is unknown, not seekable or nonfinite, leaving playback state intact. With no clip, do nothing. For overlapping asynchronous seeks, retain only the newest result for the current playback generation.

Review keyboard shortcuts and native menu/UI events are supplied by U175; the controller does not add global event handlers or intercept card-owned keys. The bridge's replay command is not permission for a card to seek, control system audio or call arbitrary player methods.

## A5. Files, silent entries and failures

Resolve each sound entry's safe basename as `render/media-serving#M2` specifies; never follow an arbitrary path or remote URL from AV metadata. Preload uses the same resolver. A missing file is skipped without a popup, and its logical name/card/side/index is added to diagnostics. Continue immediately with the next entry; no retry delay holds the queue. A silent TTS item is skipped without synthesis, player creation or an audio-session activation, while its list index stays intact.

If reading or native decoding fails, record the failure and continue to the next entry. Accept completion once, even if that failed item reports again. Distinguish absent files from decode failure or unsupported format in diagnostics. MIME is not evidence of successful decoding; never report an unimplemented codec as played.

U69 owns Ogg Vorbis/Opus/Speex decoding and video-tag presentation/unsupported-video behavior (R553, R561/R562). U67 supplies an explicit dispatch seam and tests current native audio formats. It must not ship a fake successful decoder or claim U69 complete. TTS stays intentionally silent under R559/R564, not pending a decoder.

## A6. Preloading and timing

During preparation, resolve and prepare the first playable native clip for the next side without playing the clip, activating ducking or fetching an entire large file into memory. Skip TTS/missing entries to find that first playable entry. Answer preparation may prepare its first clip while the question is visible. Bound prepared resources to the current/next side and release them on invalidation, replacement, owner closure or cancellation; a stale preparation cannot start audio.

R554 requires ≤50 ms from presentation until playback, and ≤100 ms between clips. Observe the player, with a monotonic clock. Include a missing middle file. A fake player proves policy only; use generated files to exercise playback, then hardware to verify device timing. API calls alone prove neither.

Do not delay page presentation until audio starts or require a JavaScript round trip to begin native playback. Under interrupt-off, the new side's first clip is intentionally held until the surviving old clip ends; that policy wait is reported separately from startup latency rather than treated as a slow first-start implementation.

For foreground native review on iPhone and iPad with Bluetooth audio output, start sending silence to the selected output when preparing the first playable native clip eligible for autoplay. With autoplay disabled, an explicit play or replay of a playable clip can start the stream. Start the clip at its usual presentation or manual command even if silent output has only just begun or could not start. Never add a warm-up delay or silence to the beginning of each clip.

## A7. iOS/iPadOS audio-session and lifecycle policy

R555 uses the playback category so clips play with the silent switch on. Duck other apps only while audible card media plays, including HTML media using the session. Restore their volume when the last such resource pauses, stops, fails or finishes.

The Bluetooth stream in A6 keeps the route carrying audio between words. Once started, it continues across reveals, card changes, empty queues and thinking pauses of any length during the same foreground review. Silence alone mixes at other apps' normal volume. Releasing ducking between clips must preserve output continuity. On other routes and in previews, preparation leaves the audio session inactive until playback needs it. HTML media alone does not start the Bluetooth stream.

Apple's [playback category](https://developer.apple.com/documentation/avfaudio/avaudiosession/category-swift.struct/playback) and [duckOthers option](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/duckothers) define the platform mechanisms. The following lifecycle policy is Meguri's requirement even where the platform permits more:

| Event | Required effect |
|---|---|
| app leaves foreground or owner suspends/closes | stop native clip and clear waiting/pending autoplay; pause page audio/video; do not auto-resume on return |
| interruption begins (call, Siri) | stop and clear native playback; invalidate delayed completions/preloads that could restart it |
| interruption ends, even with system resume suggestion | remain stopped; explicit user play/replay is needed |
| headphones/old audio route becomes unavailable | pause current clip and retain position, not audible fallback to speaker; explicit resume may continue on the new route |
| usable route selected, headphones retained | use that route; no extra automatic play or Replay |
| pause, queue empty or playback failure | release ducking when no card resource is still playing |

End the silent stream on foreground loss, review suspension or exit, interruption, or loss of its Bluetooth route. Release the session when no resource still needs it. Discard pending work that could restart the stream or play a pending clip over the speaker after disconnection. Returning to the review, a route notification or an interruption ending does not itself restart output. A fresh eligible review event can establish the stream again under A6 and the playback rules above.

Backgrounding/interruptions override interrupt-off. A pending load completion cannot restart sound in background. No background-audio capability is needed to continue card playback, because continuing it is forbidden here. Do not publish `MPNowPlayingInfoCenter` metadata or install lock-screen/remote transport controls (R557). This controller does not enable recording or request microphone access.

Mac uses its native playback path without an iOS AVAudioSession. Owner suspend/close and app-level review background policy still stop that session's card audio; an ordinary Mac window focus change is not itself an iOS background event. Tests must not conflate losing keyboard focus with closing the review session.

## A8. HTML media and native queue separation

Configure WebKit, before loading either page, for inline video playback on iPhone (R560). On the review page pair, the card's autoplay policy (A1, home-deck preset) also decides whether card HTML `<audio>` and `<video>` need a user gesture, as in Anki's reviewer:

- **Autoplay on**: no gesture is needed. An `autoplay` attribute and a script's `play()` start media when the side is presented (C66 C8.2).
- **Autoplay off**: media needs a user gesture in the page. An `autoplay` attribute and a script's `play()` outside a gesture do not start it, and C66 C8.2's resume at presentation does not start it either. A control the user taps, or a script run by the user's tap or key press in the page, plays it.

This is how R560 is read: its 'without a user gesture (honouring `autoplay`)' means no gesture is needed when the card's home-deck preset autoplays, and a gesture is needed when it does not, as in Anki.

The policy is read when the card's question is shown and applies to that card's answer too, so a reveal never changes it. Each card sets it again, so a gated card does not gate the next card, and the reverse. Manual and automatic native playback (A2, A3) are unaffected. WebKit's configuration-level gesture setting is fixed when a web view is created, and the page pair is reused across cards whose presets differ, so the rule cannot rest on that setting alone. A pre-rendered page takes the policy of the card it prepares. Implemented code must be checked against this rule: U67's WebKit configuration and page pair, which set no gesture requirement for every card, and U176's pre-rendering. The template and note previews (`render/preview`) follow V12 instead: card media there always needs a gesture. The Browse preview (`browser/preview` P7) applies this rule to the card it shows, with that card's home-deck autoplay, as Anki's browse previewer does.

Scheme MIME/Range support is M3/M4, not a guarantee that the VM has a real audio route.

C66 C8.2 holds HTML audio/video in an unpresented page and resumes remembered elements only when that page is presented. Swapping a page out pauses its HTML audio/video; backgrounding pauses both pages. C66 C6.3 unloads video before DOM replacement, including direct source/stream cleanup. These rules do not stop a surviving **native** clip allowed by A2. Web Audio and speech-synthesis limitations remain C66's explicit scope; this spec does not claim it can pause arbitrary browser APIs.

## A9. Synthetic sequences and assertions

Use small generated clips and a controllable player clock to test policy independently of decoding, then exercise the real native player path. The lists below are synthetic; they contain no stock templates or private media. `Q1(4)` means a four-second question clip; `A1` an answer clip. TTS and missing entries still occupy positions.

| Case | Input/events | Required result |
|---|---|---|
| AE16 playback | question list empty; answer list `[x.mp3]`, button hidden by the rendered wrapper; show question, reveal, Replay | silence on question; only x on reveal; x again on Replay. Hiding a button cannot suppress AV playback |
| AE19 interrupt | Q=`[Q1(4),Q2]`, A=`[A1]`; show Q; at1s reveal; interrupt on | Q1 stops at1s, Q2 discarded, A1 starts at answer presentation |
| AE19 queue | same lists/timing, interrupt off | Q1 continues to4s; Q2 discarded; then A1; never Q2 before A1 |
| next card | Q1 playing, A1 waiting under interrupt off; advance to card2 with Q=`[Q3]` | only current Q1 may finish, then Q3; discard A1 |
| disabled autoplay | Q1 playing; reveal to autoplay-disabled answer | discard waiting old entries; interrupt on stops Q1, off retains Q1 only; no new clip |
| replay answer | Q=`[Q1,Q2]`, A=`[A1]`, interrupt off; Replay on answer | stop old clip; Q1,Q2,A1 when replayq on; A1 only when off |
| one button | queue active; `play:q:1` from FrontSide on answer | stop old clip/queue; Q2 alone; no Q1 or A1 |
| TTS indexes | Q=`[TTS,Q1]`; autoplay, then `play:q:1`, then `play:q:0` | Q1, then restarted Q1, then silence; no TTS player/session |
| absent second file | Q=`[Q1,absent,Q3]` | Q1,Q3; record absent entry at index1; start Q3 within100ms after Q1 finishes |
| pause/seek | Q1 duration4 at1s; pause; seek−5; resume; seek+5 | paused at0 after first seek; resume Q1; clamp to4, complete once, then next item |
| interruption | current and waiting clips; begin/end with resume suggestion | stop and clear; no automatic restart |
| route loss | Q1 at1s, headphones removed | paused at1s; no speaker playback until explicit resume |
| background race | item still loading; background; load completes | no playback, no queue revival, no Now Playing metadata |
| stale completion | Replay replaces old generation; old completion fires twice | newer queue untouched by both callbacks |
| pre-render | idle payload includes Q1; prepare completes, later present | no sound/ducking while idle; Q1 starts once at present |
| Bluetooth review with autoplay | prepare Q1 on Bluetooth, present and finish Q1, spend several minutes thinking, reveal A1 | silent output starts during preparation and continues throughout the gap; other audio returns to normal volume during the gap; A1 starts at presentation |
| Bluetooth review with manual play | prepare and present the first card with autoplay off, then tap Replay | output stays inactive until Replay; Q1 starts on the tap without an added wait; subsequent silence preserves the route without ducking |
| Bluetooth teardown | during a thinking pause, background or close review, interrupt it or disconnect the headphones; deliver an old preparation callback | stream ends in each case; callback cannot restart it; no speaker playback or automatic replay on return |
| HTML media gate | card1 autoplay off, question `<audio src="x.mp3" autoplay controls>`, answer `{{FrontSide}}`; show, reveal, tap the element's play control; then card2 with autoplay on and the same question | card1: x silent on question and answer until the tap, then plays; card2: x starts when its question is presented. Native queue unchanged throughout |
| AE18 native admission | idle Mac front requests reveal; another card visible; then matching present | old card unchanged; reveal only the newly presented matching card. iOS has no pycmd and stays on question |

Also assert: home-deck rather than filtered-deck settings, autoplay-off manual Replay, repeated shown receipt, invalid command with active audio unchanged, filenames with raw percent text, empty AV lists, cancel during preload, seek while paused/loading, two card surfaces with independent identities and balanced shared session ducking. Injected spies must see no synthesized speech, native arbitrary path, remote file fetch, microphone request or Now Playing registration.

Check the audible onset on a physical iPhone or iPad with AirPods and a synthetic spoken clip that begins immediately. Compare the first presentation after a long thinking pause with immediate Replay, with other audio apps silent. Repeat with another app playing to check its volume during the word and the pause. A simulator can verify event ordering and resource lifetimes. Only the listening check establishes whether the first syllable reaches those headphones intact.

## A10. Evidence and pending owners

The queue rules derive from the existing AV contracts and pinned interoperability review, with R550/R551's explicit forced-stop policy taking precedence for user replay. Table A9 is a test specification, not a report that U67's new controller already passed. No headless-oracle golden can prove native output timing, audio-session effects or keyboard/page presentation.

M10 still lists the iOS 27 hardware seek checks as open. The VM cannot produce the required audio proof. Verify ring/silent behavior, ducking, interruption, route changes and start latency with the appropriate native or hardware test. U69 provides decoders/video; U175 connects review commands; U70 provides recovery/navigation. Keep these dependencies explicit.
