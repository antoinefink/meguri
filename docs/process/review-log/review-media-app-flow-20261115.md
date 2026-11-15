# Review and media: app-model checks

`swift-test/20261115T135637Z-39827` passes both selected tests on `20cddcab4b269f06b3cae68219755406f8398bb3` in 9.967 seconds.

- `realServiceReviewAndRegisteredMediaSurviveReopenAndFreshAdoption`: the production app model records an answer. The controlled Service returns actual Saved outcomes for the card, review log, media name and blob. A reopens its retained files. B adopts the current records, then reopens. Both retain equal card and review-log cells and exact media bytes.
- `realServiceExternalMediaReachesRetainedBindingIncrementally`: an external file enters through the normal media feed. B keeps its binding and paths and receives the exact bytes incrementally. Collection and review counts remain unchanged.

These checks exercise the production core and app model with a controlled transport. They do not prove a native renderer, Apple CKSyncEngine behavior, a physical-device exchange or final acceptance. The retained phone automation failure remains unverified; no unchanged attempt was repeated. Structural reports, normal generation integration and required gates remain open. No Unit trailer.
