# Public media fixture

Start: `f49e6bc7`. Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The host test creates four new cards, a PNG and a PCM WAV. It exports and imports the collection through real providers, compares rows and media bytes, then saves one answer in the imported copy. The saved package remains at four new cards and zero reviews. MG retains only the public package and its manifest. No product behavior changes.

Author: `cleanroom/20261112T171730Z-93271`. Fixer: `cleanroom/20261112T172121Z-94816`. Independent review: `cleanroom/20261112T172313Z-96854`, no P0–P2 findings. Compilation found an unsupported digest formatter. `cleanroom/20261112T172447Z-98451` changes only that expression; root compared every other byte.

Evidence: `core-test/20261112T172326Z-97300` retains the compile failure. `core-test/20261112T172531Z-98920` passes the test and retains the package. `similarity/20261112T172608Z-99835` passes one file, with no exceptions. Privacy and static checks pass; native playback and cloud transfer are pending. No unit or gate acceptance is claimed.
