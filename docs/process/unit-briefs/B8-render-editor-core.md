# Brief: B8-render-editor-core (U71, U190, U191, U192, U197)

Implementer-view facts as of `origin/master` cf03979b. Plan = `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` (U71 at line 7346, U190 12002, U191 12100, U192 12140, U197 12407). "Committed" means `git ls-files`. "Held" means an untracked file in the worktree that is not on master.

## 1. Batch

- Units: U71 template preview rendering (engine and page), U190 note editor component, U191 core editor services, U192 formatting, cloze and command routing, U197 paste and drop.
- Why grouped: they share the editor and preview render stack. U190 is the base page, bridge and session for U192 and U197. U191 supplies the field check and tag completion that U190 S3 and U193 call. U71 supplies the `CardPreviewView` that the Add and Edit previews embed (`specs/render/preview.md` V13).
- Milestones: U190, U191 and U192 are in M1. U71 and U197 are in M2 (`docs/process/milestones.yaml` lines 383 and 502). M2's user step includes the one-time capture of the paste corpus for U197.
- Suggested order:
  1. U191: Rust and FFI only. All its dependencies are merged.
  2. U71: mostly Rust, plus `CardPreviewView`. All its dependencies are merged.
  3. U190: the editor runtime, host and save pipeline. Do spikes E1 to E3 first.
  4. U192: depends on U190 and on U158, which is not merged.
  5. U197: the least reachable unit in the batch. It depends on U190 and on U194. U194 is an M2 unit outside this batch, and it depends on U190, U192, U31, U67 and U37 (plan U194, line 12251). U37 has no trailer, and U192 waits on U158. So U197 can start only after U190, U192 and U37 land and U194 is implemented. Do spike E4 before the iOS drop code.
- Gate: U71, U191 and the Rust half of U190 are core work. U190, U192 and U197 are Swift/WKWebView work. Run one full gate at the batch boundary with the original batch base. Record each unit's start commit (`docs/process/tiers.md` lines 7 to 13).
- Priority: per `docs/process/review-first-delivery.md` ("Next feature priority", 2026-10-27 and 2026-10-29), reviews and iCloud sync (U113, U114 and their dependencies) come first, then editing, search and stats. Master has no `Unit:` trailer for U113 or U114. This batch is editing work.
- Macos-first pass: run local Xcode MCP build and run checks. VM, simulator and device evidence stays outstanding (`AGENTS.md`). U71's R1835 timing runs on the host Mac as a proxy until the reference devices run it (`specs/render/preview.md` V14).

## 2. Dependency status (`git log origin/master --format=%B | grep '^Unit:'`)

| Unit | Depends on | Merged | Not merged (blockers) |
|---|---|---|---|
| U71 | U62, U64, U67, U29 | all | none |
| U190 | U6, U7, U9, U10, U12, U20, U24, U27, U29, U31, U67, U150, U151, U37 | U6, U7, U9, U10, U12, U20, U24, U27, U29, U31, U67, U150 | **U151** (CollectionStore, lifecycle and sync coordinator: the background flush hook, S1). RF1 mapped some of its parts (review-first-delivery.md "Execution"), but there is no trailer. **U37** (Collection FFI surface and CoreBridge services): no trailer, but `app/Packages/MeguriCore/Sources/CoreBridge/` already has `NoteService.swift` with `checkNoteFields`, among others. |
| U191 | U2, U3, U4, U20, U22, U23, U24, U27, U29, U30 | all | none |
| U192 | U190, U150, U158, U9, U23, U29, U60 | U150, U9, U23, U29, U60 | **U190** (this batch). **U158** (ShortcutRegistry): nothing named `ShortcutRegistry` exists in `app/Packages`. |
| U197 | U190, U194, U2, U3, U4 | U2, U3, U4 | **U190** (this batch). **U194** (M2, outside this batch; media insertion, `MediaImporter`, `editor/paste` P4): nothing named `MediaImporter` exists in `app/Packages`. U194 itself depends on U190, U192, U31, U67 and U37; U31 and U67 are merged, U190, U192 and U37 are not. |

Consumers outside this batch (not blockers), from the plan's Dependencies lines:
- on U190: U145, U147, U184, U193, U194, U195, U196, U200, U202, U207, U220, U221, U222 and U253 (U251 also wires U190 in when it lands);
- on U191: U193, U195 and U218; on U192: U194, U195 and U196;
- on U197: U206.

U159 is not a plan dependent of U190. It is linked only through field-html F5: with no field focused, ⌘Z and ⇧⌘Z go to the collection undo (U159).

## 3. Per unit

### U71: template preview rendering

- **Spec ids:**
  - `render/preview` V1 to V17 (main: V2, V5, V6, V7, V9, V10, V12, V14).
  - `conformance/render-ops#Q1` (Q1.2, Q1.3, Q1.4 step 0) and `#Q3`.
  - `render/type-answer#A11.1`.
  - `render/card-render-pipeline` K1, K4, K7, K9, K11.
  - `render/display-transform` D1, D2, D7, D9.
  - `render/card-runtime` C2.2, C2.3, C3.1, C5, C6, C9.
  - `render/special-fields` P2, P3; `render/cloze` C5, C6, C15; `render/template-syntax` T1, T6, T9 to T11; `text/field-emptiness` E1. Requirements R938 to R941 and R1835.
- **Meguri docs to write:** none are marked "(Meguri)" in the plan's file list. `specs/render/preview.md` is committed already.
- **Committed scenarios, all inactive:** `render/preview/drafts` and `render/preview/type-in`, with their goldens. `conformance/scenarios/render/area.yaml` does not list `preview/*`. U71 activates `preview/*` (render-ops Q3).
- **Not in the tree:** `render/preview/fill-empty` and `render/preview/fill-empty-cards`. Preview V16 says their goldens are recorded, but they stay out of `conformance/` until the testkit accepts `fill_empty`. No file with those names exists on any branch visible to implementers. They must be landed by the spec or oracle side after U71 adds the key.
- **Plan tests mapped:**

| Plan test | Coverage |
|---|---|
| Unsaved draft back template and unsaved field text (R938) | `render/preview/drafts` card cases and new-note cases. The V2 note choice and the preview note: Meguri engine tests. |
| Fill-empty gives `(Extra)` and the sample cloze (R939) | `render/preview/fill-empty` and `fill-empty-cards` (not in the tree) |
| iPad and dark on a Mac give the classes (R940) | Meguri-only: `CardPreviewTests` (V12) |
| `{{type:Romaji}}` shows "example" and compares it with "sample" (R941) | `render/preview/type-in` |
| No notes: empty fields, no error | `render/preview/drafts` (the "No notes of the type" cases), plus an engine test of the V2 fallback |
| Draft with a 30 KB script within the R1835 share | Meguri-only V14 timing test: 25 ms p95, 20 warm-up runs and 200 measured runs, release build |

### U190: note editor component

- **Spec ids:**
  - `editor/field-html` E1, E2, H1 to H6, S1 to S3, F1 to F8, B1 to B4, X1, T1 to T3, D1.
  - `notes/note-records` U1 to U3 and A4.
  - `engine/undo` M1 and A3; `engine/op-model` O2, O7; `engine/lifecycle` L4; `engine/change-stream` S3.
  - `text/media-references` M6 to M8; `notes/field-check` (for S3).
  - Requirements R825 to R833, R820, R821, R823, R248, R254, R5 and AE40.
- **Meguri docs to write:** `app/Packages/MeguriKit/Sources/Features/Editor/Resources/EditorRuntime/AGENTS.md`. It records the bridge protocol (as in field-html B1 to B4) and the clean-room rule: no Anki editor source or assets (R1960). The plan lists it among the files; no file is marked "(Meguri)".
- **Committed scenarios, all inactive (`editor/area.yaml`: `active: []`):**
  - `editor/field-html/patch/other-writer`
  - `editor/field-html/patch/revert-to-baseline`
  - `editor/field-html/patch/merge-window`
  - `editor/field-html/patch/script-kept`
  - `editor/field-html/display/media-names`

  Each has a golden.
- **Requested, not written:** every field-html T1 row (Return, empty, space, serialization, media, HTML mode and timing groups). No editor-page oracle or runner step exists (field-html T1 and E2).
- **Plan tests mapped:**

| Plan test | Coverage |
|---|---|
| AE40: open and close make no op | Meguri-only (T2). T1 `open-close-untouched` is not written. |
| AE40: Sentence byte-identical after editing Translation | `patch/other-writer` (core half), plus a Meguri test (editor half) |
| `editor/field-html/*` Return, Shift-Return and delete-all goldens | T1 rows, not written. They are blocked on the editor-page oracle (E2). |
| `onerror` and `<script>` inert and kept | `patch/script-kept`, plus a Meguri F6 test |
| Tab order and caret restore; collapse memory; HTML default and quotes or dashes; RTL; hidden Image Occlusion fields; field undo routing; save timings and background flush; layout switch without reload; R1833 perf; content mode | Meguri-only (field-html T2) |
| Saves 20 s apart merge, 61 s apart split | `patch/merge-window`, plus a Meguri test |
| Media name with a space | `display/media-names`, plus a Meguri test |
| (no plan test) baseline revert | `patch/revert-to-baseline` |

### U191: core editor services

- **Spec ids:**
  - `notes/add-defaults` D1, D2, D6, D7.
  - `editor/config-keys` K1, K4 (K4.1, K4.2), K7.
  - `notes/field-check` K1 to K6; `notes/note-records` A4.
  - `tags/complete` C1 to C7; `search/case-folding` C3.
  - Requirements R798 to R801, R807, R809 to R812, R883 and AE34.
- **Meguri docs to write:** none.
- **Committed scenarios with goldens, all inactive:**
  - `notes/add-defaults/`: `after-add`, `last-deck`, `missing-default-deck`, `no-notetype`, `review-card` (`notes/area.yaml` active `[]`).
  - `notes/fields-check/`: `states`, `duplicates`.
  - `tags/complete/`: `components`, `folding`, `order-limit`, `registry`, `separators` (not in `tags/area.yaml` active).
  - `editor/config-keys/`: `add-defaults-by-notetype`, `add-defaults-current-deck`, `first-notetype-fallback`, `notetype-change`.
- **Held:** `notes/fields-check/cloze-unicase`, `notes/fields-check/entity-markup` and `notetypes/structure/first-field-duplicates` (scenarios and goldens untracked). They are prepared, held until the spec-side change lands. The uncommitted `specs/notes/field-check.md` K6 diff marks all three recorded; the committed K6 says "not yet recorded". `first-field-duplicates` covers field-check K4 and AE34 after another field moves to position 0 (`notetypes/structure-changes` S5). Its activation probably belongs to the notetype-structure owner, but until it lands the committed scenarios do not fully cover U191's duplicate check.
- **Already on master (do not redo):**
  - The field check: `core/crates/meguri-engine/src/notes/check.rs` (U27), FFI `core/crates/meguri-ffi/src/notes.rs` `check_note_fields`, Swift `NoteService.checkNoteFields`, and the `fields_check` executor adapter (`core/crates/meguri-testkit/src/exec/notes.rs`).
  - The R801 key writes when `update_add_defaults` is set: `core/crates/meguri-engine/src/notes/add.rs` lines 79 to 88.
  - The plan's `src/editor/fields_check.rs` must reuse `notes/check.rs`, not duplicate it.
- **Plan tests mapped:**

| Plan test | Coverage |
|---|---|
| AE34: Kaishi `<b>私</b>` is a duplicate; another note type is normal | `notes/fields-check/duplicates` (plus the held `notetypes/structure/first-field-duplicates` for a reordered first field) |
| `addToCur` on, filtered current deck, review card's home deck | `notes/add-defaults/review-card` and `editor/config-keys/add-defaults-current-deck` |
| `addToCur` off, last deck deleted | `notes/add-defaults/last-deck`, `editor/config-keys/add-defaults-by-notetype` and `notetype-change` |
| An add with `updateAddDefaults` writes three keys; without, none | with: `notes/add-defaults/after-add`. Without: Meguri test (add-defaults D7, note-records A4). |
| Cloze states and `<br>` empty | `notes/fields-check/states` (plus the held `cloze-unicase`) |
| `jap::aud` and `verb::mod` ordering and limit | `tags/complete/components` and `order-limit` (plus `folding`, `separators`, `registry`) |
| `preset_deck`, D3 to D5, sticky carry-over | Meguri-only (add-defaults D7) |
| (no plan test) | `first-notetype-fallback`, `no-notetype`, `missing-default-deck` |

### U192: formatting, cloze tools and command routing

- **Spec ids:**
  - `editor/formatting` F1 to F19.
  - `editor/cloze-wrap` W1 to W10.
  - `editor/config-keys` K8 to K11.
  - `editor/editing-strings` (Format menu and option strings, as applicable).
  - `notes/card-generation` G4, G7.
  - Requirements R834 to R848, R857 to R861, R864, R865, R249 to R251, R253, R258 and AE10.
- **Meguri docs to write:** none are marked "(Meguri)". Per the root `AGENTS.md`, extend `EditorRuntime/AGENTS.md` for new bridge messages (formatting F13).
- **Committed scenarios with goldens, inactive:**
  - `editor/formatting/options/key-writes`
  - `editor/cloze/save/new-and-same-number`
  - `editor/cloze/save/empty-note`
  - `editor/cloze/output/read-back`
- **Requested, not written:** every formatting F17 row and every cloze-wrap W9 row (editor-page oracle). Also config-keys K7's `bool-defaults`, `first-day-of-week` and `write-scalar`, which need the draft `editor_config_set` and `editor_config` (editor-ops E6 to E8).
- **Plan tests mapped:**

| Plan test | Coverage |
|---|---|
| ⌘B over `<strong>`; oblique ⌘I; sup/sub exclusive; F7 colours; Remove Formatting with Highlight kept; pending ⌘B; lists, indent and the outside-list message | F17 rows (`bold-over-strong`, `italic-oblique`, `sup-sub-exclusive`, `color-*`, `remove-keep-highlight`, `pending-bold`, `list-number`, `indent-item`, `indent-outside-list`), not written. Meguri `FormattingConformanceTests` use the spec tables. |
| Palette swatch written to `customColorPickerPalette` | stored half: `formatting/options/key-writes`. K9 add and remove arithmetic: Meguri tests. |
| Block buttons disabled in the HTML source | Meguri-only (F18) |
| Cloze `{{c3::word}} ` with the space outside, Same Number reuses 3, empty note `{{c1::}}` | W9 `new-after-other-field`, `same-after-other-field`, `new-empty-note` (not written). Save side: `cloze/save/new-and-same-number` and `cloze/save/empty-note`. Read side: `cloze/output/read-back`. |
| ⇧⌘C in a non-cloze field; group hidden for Basic | W9 `not-cloze-field` (not written), plus Meguri tests (W9 "Meguri tests without goldens") |
| AE10 typing "e"; ⌘, in a field and in the tags | Meguri-only (F14) |
| R200 registry test: no duplicates per context | Meguri-only `EditorShortcutRegistryTests` (F14). Needs U158. |

### U197: paste and drop

- **Spec ids:**
  - `editor/paste` P1 to P25 (main: P7 to P17, P22, P24).
  - `editor/paste-filter` X1 to X10.
  - `editor/paste-sources` S1 to S15.
  - `conformance/editor-ops` E1 to E5.1.
  - `editor/config-keys` K2, K3.
  - `media/media-files` M3; `text/media-filenames` F1.
  - Requirements R866 to R877 and R139.
- **Meguri docs to write:** none are marked "(Meguri)". The plan's file list is short one item: editor-ops E4 and paste P10.2 (decided 2026-11-10) put the URL fetch and naming in the Rust core, so `meguri-cli` can run `add_media_from_url`. That core module is a plan follow-up.
- **Committed scenarios with goldens, inactive:**
  - `editor/paste/store/image-names`
  - `editor/paste/store/fetched-names`
  - `editor/paste/store/snippet-save`
- **Held (untracked):** `editor/paste/url/{file,names,status,stored-name,timeout}` (scenarios and goldens). They are prepared and held until the testkit models `add_media_from_url`, `http_fixtures` and `url_files`, and they need the deviation entries of editor-ops E5.1 when activated.
- **Requested, not recorded:** every P24 row (editor-page oracle). `corpus` also waits for the M2 paste-corpus capture (`conformance/corpora/paste/` does not exist).
- **Plan tests mapped:**

| Plan test | Coverage |
|---|---|
| HTML beats text; file URLs; image beats URL | P24 `order-*` (not recorded). Meguri `PasteProcessorTests`. |
| Plain text `a\tb   c\nd<e>` | P24 `text-plain` (not recorded). Meguri `PlainTextToHTMLTests` against the P13 table. |
| Word corpus in keep and strip mode | P24 `corpus`: blocked on the M2 capture and the editor oracle (paste-filter X10) |
| PNG bitmap `paste-<sha1>.jpg` or `.png` | `paste/store/image-names` (store side). P24 `image-encode` (not recorded). |
| `data:image` to a media file | `paste/store/image-names` (the decoded data image). P24 `html-img-sources`. |
| URL `a.mp3?x=1` gives `[sound:a.mp3]`; a page URL gives a link | held `paste/url/names`, `paste/store/fetched-names` and `paste/store/snippet-save`. P24 `text-keep-tokens`. |
| Remote `<img>` fetched; 31 s delay | held `paste/url/timeout` and `status`. Meguri `RemoteMediaFetcherTests` with a local HTTP fixture server. |
| Dark keep mode drops colour; internal paste keeps colour and drops font-size | Meguri `PasteFilterConformanceTests` (paste-filter X9). P24 `keep-filter-dark` and `internal-paste` (not recorded). |
| ⇧⌘V inverts strip | Meguri-only |
| Mac drop onto field 3, onto a collapsed field, Shift-drop | P24 `drop` (not recorded). Meguri `PasteDropUITests` (runs in the VM). |

## 4. Testkit and adapter work (`core/crates/meguri-testkit/src/scenario/`, `src/exec/`)

**Already modelled and registered** (no work):
- the `template` observation, with `template.card` (`exec/render.rs:22`), `type_answer`, `cloze`, `text` (with `iri_encode`, `iri_decode`, `media_refs`, `media_filename`), `fields_check`, `ids`, `media_dir` and `undo_status`;
- the ops `db_exec`, `set_config`, `edit_note`, `add_note`, `delete_notes`, `delete_decks`, `set_current_deck`, `reopen`, `advance`, `undo`, `add_media` and `clear_unused_tags`;
- the setup sections `decks`, `notetypes`, `notes` and `media_files`.

**Needed:**

| Form | Used by | State | Defined in |
|---|---|---|---|
| `template` case key `fill_empty`, plus the capability `template.fill_empty` | U71: `render/preview/fill-empty`, `fill-empty-cards` | Rejected by the model's key allowlists (`model.rs:3220` card case, `:3240` new-note case). Not registered. | `conformance/render-ops` Q1.2, Q1.3, Q1.4 step 0, Q3; `render/preview` V6, V16 |
| `add_defaults` observation (`{review_card?}`) | U191: `notes/add-defaults/` `after-add`, `missing-default-deck`, `no-notetype`, `review-card` (not `last-deck`); `editor/config-keys/add-defaults-by-notetype`, `add-defaults-current-deck`, `first-notetype-fallback` | Parsed (`ObservationKind::AddDefaults`; `prepared.rs` `AddDefaultsRequest`). No executor adapter. | `notes/add-defaults` D7 |
| `notetype_last_deck` observation (`{notetype}` or `{notetype_id}`) | U191: `notes/add-defaults/last-deck`, `after-add`, `editor/config-keys/notetype-change` | Parsed (`prepared.rs` `NotetypeLastDeckRequest`). No adapter. | `notes/add-defaults` D7 |
| `tag_complete` observation (`{cases: [{input, limit}]}`) | U191: `tags/complete/*` | Parsed (`prepared.rs` `TagCompleteRequest`). No adapter. | `tags/complete` C7 |
| The `edit_note` step run through the field-patch form | U190: `editor/field-html/patch/*`, `editor/cloze/save/*`, `editor/formatting/options/key-writes` | Today it runs the whole-note update | `editor/field-html` T3 (decided 2026-11-10), S2 |
| `add_media_from_url` op (rejection `fetch_failed`); setup sections `http_fixtures` and `url_files` | U197: held `editor/paste/url/*` | Not in the model (no match in `scenario/`). Spec status: draft. | `conformance/editor-ops` E2, E3, E4; `conformance/dsl` D6.9, D6.10 and the op table |
| `editor_config_set` op; `editor_config` observation | U192, later: config-keys K7 `bool-defaults`, `first-day-of-week`, `write-scalar` (not written) | Draft, with no driver op, schema or model (editor-ops E8) | `conformance/editor-ops` E6, E7 |

Activation: list each scenario in its `area.yaml` in the change that lands its adapter (`conformance/AGENTS.md`).

## 5. Open questions and decisions

| Item | Source | Blocking? |
|---|---|---|
| The fill-empty scenarios and goldens are absent from the tree; preview V16 says they are recorded | `specs/render/preview.md` V16, `render-ops` Q3 | Blocks U71's full activation only. Add the key, then ask the spec side to land the files. |
| `notes/note-records` U1 has no field-patch input form yet. field-html S2 is normative for it (decided 2026-10-27). The plan's U190 file list should add the patch form to U27's note update and to the FFI. | `editor/field-html` D1 | Not blocking: follow S2 |
| Editor-page oracle (quarantined Anki editor image) does not exist; every T1, F17, W9 and P24 golden is unrecorded | field-html E2, T1; formatting F17; cloze-wrap W9; paste P24; paste-filter X10 | Not blocking for implementation. Use the spec tables in Meguri tests. A later golden that contradicts the spec is a finding against the spec (E2). |
| Paste corpus capture (M2 user step; Google Docs needs Antoine's account; agents stop there) | `docs/process/milestones.yaml` line 501; plan U197 execution note | Blocks only U197's `corpus` tests |
| Spikes: E1 key routing to a focused WKWebView on Mac and iPad; E2 no quote or dash substitution in the in-page textarea (fallback: a native TextKit editor per field); E3 31-field open ≤100 ms; E4 iOS drop interception point | plan U190 and U197 execution notes | Blocking for the parts they gate (E1 to E3 before the save pipeline lands; E4 before iOS drop) |
| The U197 URL fetch lives in the Rust core (one core fetch service). The plan's file list must gain that module. | `conformance/editor-ops` E4; `editor/paste` P10.2, P22 (2026-11-10) | Not blocking: decided |
| `add_media_from_url`, `http_fixtures` and `url_files` are draft (oracle has them; Meguri model and executor do not) | `editor-ops` header and E4; `dsl.md` op table | Blocks the held `paste/url/*` only |
| `editor_config_set` and `editor_config` have no driver op or schema yet | `editor-ops` E8 | Not blocking: those K7 scenarios are not written |
| Stale status text: add-defaults D7 says the four config-keys K4 scenarios are "not recorded yet", but their goldens are committed and config-keys K7 says recorded | `specs/notes/add-defaults.md` D7; `specs/editor/config-keys.md` K7 | Not blocking: a spec-owner fix |
| Plan wording superseded: U202's query lane for previews (now interactive); U202's "fill-empty forced on" (now the initial state only); the first-note fallback sits in `renderUncommitted` | `render/preview` V17 | Not blocking: follow the spec |
| Plan correction: `notes/add-defaults` D1 takes `preset_deck` (corrected 2026-10-27) | plan U191; add-defaults D1 | Not blocking |
| `editor/paste` P21's joined image label (R191) | `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md` "Strings specs" | Not blocking: a U194 and U197 strings follow-up |
| The page editor's field-copy order is not yet observed (proposed deviation `editor-paste-field-copy`) | `editor/paste-sources` around line 264 | Not blocking |

## 6. Traps

**Clean room**
- Editor goldens are observed values only. No Anki editor source, page or assets may enter tests or the app (R1960; field-html E2; plan U190).
- The page CSP forbids inline script and handlers. Fields go in through `innerHTML` and the stored HTML is never rewritten (plan U190 Approach, "Page origin and security"; R831). field-html F6 states the outcome: no script, handler or `javascript:` URL in a field runs, and the stored HTML keeps them unchanged.

**U71: preview**
- Preview values skip save normalization. U+0001 and `<b\rr>` are not empty in a preview (V5 table).
- Fill-empty fills only from the question template's cloze references. Every other empty field becomes `(Name)` unescaped. The cloze card list comes from the unfilled values (V6).
- The sample text is English in every language and has no String Catalog entry (V7).
- The type-in pair order is `[expected "example", typed "sample"]` (V9).
- A preview writes nothing and records no undo or change event (V10).
- The page always calls `_showAnswer` and never `_showQuestion`. Platform-class changes go through the isolated world. `reviewExtra` is left out. No autoplay (V12).
- Cache parsed templates by template text, never by note type id (V14).
- Refusals: too many values or an out-of-range target gives `InvalidArgument`; a missing template gives `NotFound` (V2).

**U190: editor and save pipeline**
- Never re-serialize an untouched or focused-but-unchanged field. Anki rewrites fields on open and Meguri must not (field-html X1, AE40).
- The patch is a second input form of U27's update, applied in one transaction. Emulating it in Swift (read, then whole-note write) is not allowed (S2).
- On `Stale`, `NotFound`, `InvalidArgument` and the other failures, the last sent value goes back to the last committed value. Every failure is shown to the user (S2).
- Every patch uses `UserUndoable`, the step "Update Note" and the 60 s merge key. A patch that registers a tag or generates a card gets its own step (S2, `engine/undo` A3).
- From U190 on, the executor's `edit_note` runs the patch form (T3).

**U191: core services**
- Every `add_note`, setup notes included, writes `curModel`, `_nt_<id>_lastDeck` and `_deck_<id>_lastNotetype`. Arrange those keys with `db_exec` after setup and never observe real ids (`conformance/AGENTS.md`).
- The oracle reopens the collection before `add_defaults` and `notetype_last_deck`. Never undo across those observations (add-defaults D7).
- Error precedence depends on `addToCur`: `true` resolves the deck first, so `NotFound`; `false` resolves the note type first, so `InvalidArgument` (D1).
- Tag completion uses simple case folding with no normalization, takes the first fitting component, and returns `limit` results. The oracle passes `limit` − 1 to Anki's `match_limit` (C3, C7). `limit` 0 reads nothing. The query is cancellable between rows (C1).

**U192: formatting and cloze**
- Formatting uses the runtime's own DOM transforms, not `execCommand`. Each transform is one field-history entry (plan U192).
- Spaces typed while a format is pending become `&nbsp;`. Any non-typing insertion cancels pending states (formatting F5).
- Cloze: N is computed over every field's session values with raw media names. Non-breaking spaces in the selection are kept as they are; Anki turns them into spaces (proposed deviation `editor-cloze-nbsp-kept`, entered with its golden). Saturate at 2^53 − 1 (cloze-wrap W3, W4, W8).
- ⌘, means bulleted list only while a rich-text field has focus. Otherwise it opens Settings (R843; formatting F14).
- Option keys are written only when the value changes, with no undo step. The palette write applies K9's add or remove and keeps unknown slots' bytes (config-keys K8 to K10).

**U197: paste and drop**
- The URL fetch and naming live in the core (editor-ops E4).
- Meguri's `add_media_from_url` output is the stored name, not the derived name. Differing rows need the deviations listed in editor-ops E5.1, entered when the scenarios activate.
- A scenario never names a real host. Use `{fixture}` and `{files}` placeholders only (E1, R1876).
- Media names in scenarios must differ under NFC plus unicase (`conformance/AGENTS.md`, media).

**Running tests (all units)**
- Simulators and Mac UI suites run only in the VM through `tools/mg`. Never run macOS XCUITest on the host (root `AGENTS.md`).
