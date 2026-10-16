# Coverage note: render


R-ID coverage map (unit → R ranges):
- U60: R529, R953–R962, R964 (parse/marker), R965–R968; supports R930/R942 (analysis API) and R1907–R1909.
- U61: R563 (HTML stripped inside math in `cloze:` output; parsing ignores math, R973 as amended after U61's review), R963, R969–R975, R977 (extraction); supports AE33.
- U62: R469–R470 (composition), R472 (data supply), R525–R528, R531, R559 (index), R564, R583, R960–R961 (values), R976; R1907/R1908 observation executor.
- U63: R990, R1003–R1012, R1013 (data side), R1016 (data side); AE33 (c0), AE38 (data side).
- U64: R530, R532, R533, R537, R545 (core), R941 (compare), R977 (compare), R1020 (core); AE16 (core), AE17 (compare).
- U65: spikes S1/S2; de-risks R465, R493–R503, R534, R1807, R1862.
- U66: R466–R474, R476, R478 (page), R479–R489, R491, R492, R499 (preload helper), R504, R505–R510, R512 (non-IO), R513–R522, R534–R536, R563, R565, R1960, R2038 (page); AE15, AE17 (IME), AE18 (page).
- U67: R465, R475, R477, R478 (state), R490, R493–R503, R511 (native), R547–R552, R554–R559, R560 (config), R1020 (playback), R2038; AE16 (playback), AE18, AE19.
- U68: R512 (IO), R539–R544, R546, R1960.
- U69: R553, R561, R562.
- U70: R454 (mechanism), R523, R524, R538, R560 (background), R566–R570, R571–R573 (card side), R574–R577, R579, R580 (card side), R2040; AE20 (rendering part).
- U71: R938–R941 (engine/web view side), R1835 (render share).

R-IDs in my assigned ranges owned elsewhere:
- R578 (buttons in one row), R581 (toasts): U175.
- R580 answer-button labels and R571 zoom setting persistence, ⌘+/⌘−/⌘0 and the ⋯ menu: U175 (setting storage U160).
- R573 chrome Dynamic Type and R574/R575 placement between bars: U175.
- R582 (reviewer config writes): U43/U175.
- R938 side-by-side/pane layout and R939 200 ms debounce: U200.
- R1013–R1016 editor UI (rebuild into groups, image change, deletion notice, error UI): U205; R1014 needs nothing from the core because coordinates are normalized.
- R1017: U27 (cloze numbers in extra fields count for card generation, via U61 `numbers.rs`) and U190/U205 (field list).
- R1018, R1019: U205.
- R990 consumers (Add-screen picker R991–R994): U195/U205.
- R1009–R1011 are core functions in U63, but U205 must call them. U205 must not renumber on its own.
- R1907–R1915 rendering conformance suites, the corpus fixtures (U1–U16, S1–S20) and the T3 private run: U240+. My units supply the `render` observation executor, the WebTestHarness and the runtime fixtures they reuse.
- R401–R409 and R425–R427 (tap zones, exemptions, shortcuts): U175 owns the rules. U66's isolated-world `interaction.js` emits the classified tap, selection, key and focus signals that U175 decides on.
- R443–R450 (pre-render policy and budgets): U175. U66/U67 provide `prepare`/`present`, the preload helper and its budgets, and media cache warming.

Cross-area contracts assumed:
1. Render FFI (U62/U64/U71, interactive lane):
   - `render(cardId, mode: review|preview|browser, host)` returns a `RenderedCard` with per-side display HTML, AV tags, type-in descriptor, error kind, ordinal, flag/mark, CSS and `reviewExtra`.
   - `compareTypedAnswer(descriptor, typed)` returns HTML.
   - `renderUncommitted(notetypeDraft, fields, ord, fillEmpty, simulateTypeIn)`.

   U43 should key pre-render validity on `(cardId, stateToken)` plus notetype and appearance generation (R447).
2. Template analysis API (U60) for U27 card generation and U29/U200 validation (referenced fields, non-empty requirement, parse errors).
3. Cloze numbers API (U61) for U27 and U200 (card picker R931).
4. Image-occlusion FFI (U63): detect, parse, write, allocate ordinals. Consumed by U190/U195/U205.
5. CardRendering component API (U67) for U175:
   - `CardPagePair` with `prepare`, `present`, `showAnswer`, `setAppearance`, `setCardState`, `mediaArrived`, `suspend`/`resume`;
   - events `shown`, `visibleConfirmed`, `bridgeAction`, interaction signals, `scriptError`, `missingMedia`;
   - `CardAudioController` session events;
   - `CardDiagnostics` read by U175's "Template Errors…" view (R522, R498).

   `CardPreviewView` (U71) is for U200, U190/U195 (Add/Edit preview), U210 (browser card preview) and the summary's missed-card display (R442).
6. Oracle driver (U3) observations and ops:
   - `render` (q/a HTML with play markers, AV tags, type marker, error kind);
   - `compare_answer`, IRI path encoding, cloze extraction for typing, cloze numbers;
   - uncommitted render with fill-empty;
   - IO note parsing (for validating Meguri-written Occlusion fields).
7. KTD2 parity pins to add: `blake3` (hint ids) and `difflib` (type-in diff), at the oracle lockfile versions with `=` pins.
8. U7 Info.plist: `NSAllowsArbitraryLoadsInWebContent` (http subresources in cards, R499; now listed in U7). No background audio mode (R556).
9. The U2 similarity scan must also cover JS/CSS under `app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/`, compared against Anki's `ts/` and `qt/aqt/data/web/` at every release since 2.1.50 (R1957, R1960).
10. THIRD_PARTY_NOTICES (U5 gate): jQuery 3.7.1 (MIT, upstream release, not an Anki asset) and the Xiph codecs (BSD).
11. MediaArrived from U111/U31 reaches U175, which calls `mediaArrived(names)` and re-prepares affected pre-rendered cards (spec-flow Q14 default).

Conflicts and open questions:
- Resolved in KTD11: the card web views use one persistent `WKWebsiteDataStore(forIdentifier:)` with a fixed identifier (R502), verified in S2(d).
- R516 says Meguri adds no page-world globals beyond the Anki-named ones, but R526 says the iOS replay button calls "a Meguri page function". Proposed resolution: the iOS onclick posts to the AnkiMobile `cb` message handler with a Meguri play message (no new global, and no backtick or `${`). Otherwise R516 must allow exactly one named function. Needs a product decision.
- R523 says "on in personal builds", but R2040 says web inspection only in debug builds. Personal builds are Release builds. Proposed: inspectable iff (Debug) or (personal build switch on and setting on), with the setting hidden in App Store builds. R2040 would need rewording to "debug or personal builds".
- R479 asks for Anki's base rules "exactly" while also modernizing fonts, and R1959 says Meguri authors its own base CSS. Resolution used: a Meguri-written stylesheet implementing the R479–R487 constants, with the font-family change listed as a known deviation (R1889).
- R1907's display normalization: replay-button markup and type-in box insertion live in Anki's Qt layer, which is not in the oracle package. These are verified by spec plus Meguri snapshots, not oracle goldens. IRI encoding and compare_answer are oracle-verified, if the driver exposes them (contract 6).
- The image-occlusion writer format (R1003–R1006) is produced by Anki's web editor, not the backend, so no oracle golden exists for writing. U63 uses spec-derived goldens, oracle parse-back, and T3 round trips of real fields.
- R534 (software keyboard without an extra tap after a script focus on iOS) may not be achievable with public WebKit API. S1(e) decides; the fallback is focus with the keyboard appearing on first tap, recorded as a deviation.
- Resolved: the card runtime lives at `app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/`, inside the CardRendering target, so SwiftPM bundles it as target resources and CardRenderingTests load it directly.
- U69 adds a vendored C target `app/Packages/MeguriKit/Sources/XiphCodecs/`, which is not in the Output Structure. It is needed for R553 unless AVFoundation on OS 27 decodes Vorbis and Speex natively (checked first in U69).
- The anchor "U64 type-in/IO/media render" is kept. IO storage, parsing and ordinals were split into U63, and U64 depends on it. U205 and U206 depend on both U63 and U64.
- Anchor U67 is at the upper end of unit size (page pair + scheme handler + audio queue). Ogg-family codecs and video moved to U69, and page behaviors (links, zoom, recovery, inspection) moved to U70, to keep U67 landable.
- Foldable-specific continuity (R576 fold/unfold) cannot be tested until the Xcode 27.1 SDK; U70 covers it through size-class changes only.
