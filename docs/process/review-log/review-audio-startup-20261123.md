# Native review audio startup on iPhone

Base `54c1a38c999aba5e1969d0d1411335ca4f4a8e91`.

On iPhone with AirPods, the first syllable of a short autoplayed review clip sometimes drops. Immediate Replay restores the word. The native player now prepares after session activation and before reading the clock for its 5 ms start offset. This keeps native preparation inside playback and follows `render/audio-playback` A6–A7 without adding a fixed wait. [Apple's scheduling contract](https://developer.apple.com/documentation/avfaudio/avaudioplayer/play%28attime%3A%29)

Filtered implementation `20261123T210017Z-43046` and independent filtered review `20261123T210209Z-48194` passed their boundary checks. The review found no source issues and classified the complete Swift file and both session records as public. The orchestrator matched these SHA-256 values.

| Path or session record | SHA-256 |
|---|---|
| `app/Packages/MeguriKit/Sources/CardRendering/Audio/AVQueuePlayback.swift` | `3d0ea830aab597d4da13041e41bc82bed3ea94e27a22ae5b7d1925c29617accd` |
| `s-20261123T205834Z-00333b63.json` | `f994d6194eb22197a70cb9b35fe24be9e08d798ed5aaf2e0a70dc4d7b8089d6d` |
| `s-20261123T210148Z-348c93a3.json` | `8e406bd2b503be5214a4c1cb0bd223cddc7578443437a46bc26e3803260d0b59` |

Privacy check `20261123T210255Z-52854` passed for the source patch. iPhone simulator job `20261123T210230Z-49551` is in progress. The PR runs the similarity gate. AirPods playback needs a device check. [Inherited timing issue](U69.md)
