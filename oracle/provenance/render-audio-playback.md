# U67 native AV policy provenance

Session: s-20261019T224053Z-a9f830d4. Existing reference worktree pinned to tag26.09.3, commit29bb700b951e3f0c0cb69b77c0180fc1fe33e6ba. Only the following bounded queue/card selection research was needed; no source text or code structure is carried into neutral specs.

| Neutral scope | Pinned source reviewed | Finding |
|---|---|---|
| audio-playback A1 | pylib/anki/cards.py | Home deck uses original deck when present; preset supplies autoplay and replay-question policy. |
| audio-playback A2 | qt/aqt/sound.py | Replacing side audio drops old waiting items, optionally retaining the currently active item. |
| audio-playback A3 | qt/aqt/reviewer.py and qt/aqt/sound.py | Side list selection and question-before-answer replay order; direct entry lookup uses side/index. |

R550/R551 require explicit Replay and single-entry commands to stop existing playback, even with automatic interruption disabled. Those requirements take precedence over the generic replacement setting. Basenames and silent indexes come from render/av-tags. The new sequence table uses independently chosen inputs to test these facts and AE16/AE18/AE19; it imports no reference fixture, asset or code structure.

The rest of A1–A10 is Meguri platform/lifecycle/performance design: native controller ownership, generation guards, preloading, failure handling, interruption/route/background policy and later U69/U175 boundaries. Official Apple documentation was read on2026-10-19: https://developer.apple.com/documentation/avfaudio/avaudiosession/category-swift.struct/playback.md and https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/duckothers.md. It confirms silent-switch playback and activation-scoped ducking. Foreground-only behavior, no automatic interruption resume and no Now Playing metadata remain the product requirements, not a claim about the reference player's platform backend.

No headless-oracle or physical-device audio pass is claimed. U67 native tests remain to be implemented; the existing S2 device seek gap is preserved. No harvest link is added merely because a future native test is specified.
