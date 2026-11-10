# B10-integrations: unit brief (implementer view)

Base read: `21fc5c1f`, 2026-11-10. Facts rechecked at `5cae7aef` (origin/master, 15:38). Two commits came after the base. `cf03979b` (spec writer) changes browser, stats and storage specs and nothing under `specs/servers/`. `5cae7aef` (oracle maintainer) adds the AnkiConnect recorder under `oracle/ankiconnect/`, the harness and client transcripts (§3) and the `conformance/AGENTS.md` recordings line. Neither carries a `Unit:` trailer, so §2 is unchanged. Implementer-view sources only: no `oracle/`, `refs/`, `local-data/` or Anki/add-on source was read. "Held" means present in the worktree but not in `git ls-files`.

## 1. Batch

- Units: U135 (server host, security, integrations store), U136 (transcript harness), U141 (AnkiConnect envelope), U142 (collection actions), U143 (media actions, external folder), U146 (GUI actions, `sync`, `searchNote`). Also U137–U140 (Migaku port 44432): status not yet decided (below).
- Why grouped: all are M4 "Mac integration servers" units (`docs/process/milestones.yaml` M4: U135–U147, U164, U255). U136's executor drives U135's host (`specs/servers/transcripts.md` T20, `specs/servers/host-security.md` H22). U141 defines the registry that U142, U143 and U146 fill (`specs/servers/ankiconnect-envelope.md` E12).
- Suggested order: U135 → U136 → U141 → U142 → U143 → U146. Plan dependencies fix this order (plan §U135–U146).
- Batch size: batches hold 3–5 related units, Swift/FFI included. Pure Rust internals may have up to 20 units per full gate. Record the batch base first (`docs/process/tiers.md`, "Current verification cadence"). U135 creates `core/crates/meguri-ffi/src/servers.rs` (absent at `5cae7aef`), and U143 later modifies it to add the `ingest_external_media` and `media_folder_granted` exports (plan U143 Files). U146 has Swift work. So this batch is not pure Rust.
- First group: only U135 → U136 → U141 can start now, and it gates as one group. U142 and U143 cannot join it: U142 also needs U86 and U87 (`B1-browser.md`, this folder) and U95 (`B6-stats-core.md`, this folder), none of them merged, and U143 needs U142. U142 and U143 wait for U86, U87 and U95 to merge. U146 is blocked further (§2).
- Migaku: Antoine decided on 2026-11-10: "migaku actually is just anki connect so you can skip the extension work. If ankiconnect is replicated then Migaku would work." (`docs/process/review-log/migaku-via-ankiconnect-20261110.md`). No extension capture is pursued. The transcript work for U136 and U141–U143 records Migaku's AnkiConnect call shapes. Whether U137–U140 are dropped, reduced or kept is a plan change for Antoine and the orchestrator (same note). Do not start U137–U140 until that decision is recorded.
- Priority: reviews and iCloud sync come first, then editing, search and statistics (`docs/process/review-first-delivery.md`, "Next feature priority" 2026-10-27/29). The Anki **sync-server** freeze (U13/U105/U106/U107/U109/U110/U111/U116) does not cover this batch. "local Migaku/AnkiConnect integrations remain required" (same file, "Anki server freeze"). Anki-server sync stays hidden in the UI, and iCloud is the only sync offered (AGENTS.md, N12.4). So `sync` (U146) is the iCloud exchange only (`specs/servers/ankiconnect-gui.md` G9).

## 2. Dependency status

"merged" means a `Unit: <id>` trailer exists in `git log` at `21fc5c1f`. The two later commits up to `5cae7aef` add no `Unit:` trailer.

| Unit | Depends on (plan) | Merged | Not merged (blockers) |
|---|---|---|---|
| U135 | U1, U2, U5, U6, U22, U24, U38 | all | none |
| U136 | U2, U3, U4, U135 | U2, U3, U4 | **U135** |
| U141 | U24, U135, U136 | U24 | **U135, U136** |
| U142 | U21, U27, U28, U29, U30, U41, U43, U45, U48, U60, U80, U86, U87, U95, U141 | U21, U27, U28, U29, U30, U41, U43, U45, U48, U60, U80 | **U141**; **U86** (bulk framework), **U87** (Change Deck op: `changeDeck`, A5.5), **U95** (day boundaries: stats actions, A14) |
| U143 | U31, U135, U141, U142 | U31 | **U135, U141, U142** |
| U146 | U141, U142, U143, U144, U150, U151, U175, U195, U210 | U150 | **U141, U142, U143, U144, U151, U175, U195, U210** (Swift side not buildable yet) |
| U137 | U135, U136 | none | U135, U136 |
| U138 | U20, U28, U29, U135, U137 | U20, U28, U29 | U135, U137 |
| U139 | U20, U24, U27, U31, U60, U62, U137, U138 | U20, U24, U27, U31, U60, U62 | U137, U138 |
| U140 | U25, U27, U28, U29, U86, U87, U138, U139 | U25, U27, U28, U29 | U86, U87, U138, U139 |

- Only variants U82a and U85a are merged, not U82 or U85. U142's `deck:current` follows Browse's rule (plan U142 Approach), which the merged U80 search provides through `curDeck` (`specs/servers/ankiconnect-actions.md` A16).
- U146 passes Anki column ids through. "Mapping of Anki browser column ids to Meguri columns is owned by U84/U210" (plan U146 Approach). Neither U84 nor U210 is merged.
- `core/crates/meguri-servers/` exists only as a one-line stub (`src/lib.rs`, Cargo.toml without dependencies). axum and hyper are not yet in `core/Cargo.toml`. tokio and reqwest are allowed in meguri-servers, and axum only there (`core/AGENTS.md`).

## 3. Per unit

Committed at `5cae7aef`: 50 oracle transcripts, each a `.yaml` scenario with its `.json` golden, under `conformance/recordings/servers/ankiconnect/`: `harness/` 3 {version, ping, tokens}; `clients/agent/` 7; `clients/asbplayer/` 16; `clients/mpvacious/` 6; `clients/yomitan/` 18. The commit message says "51" and "agent sessions (8)", but `git ls-files` lists 50 and 7. Nothing exists under `conformance/scenarios/servers/` or `conformance/goldens/servers/`. The committed `conformance/AGENTS.md` line says these files live under `recordings/` until the DSL suite and goldens-check skip `servers/`, and that `oracle/ankiconnect/record.sh` writes them ("a `tools/mg` job later"). `conformance/deviations.yaml` has no AnkiConnect entry. The other specs say "Scenarios: none yet" and list the planned ones as tables. No unit Files list marks a "(Meguri)" design doc, and U135–U143 write no `docs/design/` file. `specs/servers/` has 13 tracked files: 12 `.md` specs and `interop-vocab.txt`.

Held (prepared, not on master), from `git status` at `5cae7aef`. Re-list before use:
- `conformance/recordings/servers/ankiconnect/clients/migaku/` 8 transcripts with goldens {add-note-errors, card-creation, connect, destination-by-id, intercept-add-window, learning-status, media-lists, srs-import}: prepared, held until committed by the oracle maintainer.
- `conformance/recordings/servers/ankiconnect/undo/` 21 scenarios with no goldens (`api-undo-history` Y4): prepared, held until recorded and committed by the oracle maintainer. They mainly concern U147.
- `specs/servers/api-undo-history.md` (R1795, R1796, AE80; written 15:32, after this brief's first draft): prepared, held until committed by the spec writer. It mainly concerns U147, but it names U136's add-on pin and the `recordings/servers/ankiconnect/undo/` tree.
- `conformance/fixtures/servers/ankiconnect/pixel.png` (`specs/servers/ankiconnect-clients.md` C6): prepared, held until committed. Committed transcripts already depend on it: `clients/mpvacious/{add,add-note-media-dir,update-last-card}.yaml` reference it by `fixture{path,sha256}`, and `clients/asbplayer/{add-note-data-picture,update}.yaml` inline its bytes. Those three mpvacious scenarios cannot replay until it is committed.

### U135

- Spec trailers: `servers/host-security#H2`–`#H13`, `#H15`, `#H16`, `#H19`–`#H22`. The plan also cites R1663, R1665, R1666, R1668, R1670, R1672–R1678, R1789, R1790, R1949, AE78 and R1659.
- Planned declared scenarios are the ids of `conformance/scenarios/servers/host/` (host-security H17). None is written. They need U136's runner, so until then the U135 Rust tests (`tests/host_*.rs`, `loopback_only.rs`, `lifecycle.rs`, `keepalive.rs`) carry them.

| Plan test | Scenario id (H17) or Meguri-only test |
|---|---|
| AE78 helper on 127.0.0.1:8765 → PortInUse, nothing on [::1]; release → both within 10 s | `port-in-use-ipv4-holder`, `port-in-use-release`; `port-in-use-retry-time` (`next_retry_at` 10 → 20) |
| wildcard 0.0.0.0:44432 → PortInUse | `port-in-use-wildcard-holder` |
| Host accepted / refused (R1676) | `host-accepted`, `host-refused`, `host-absolute-target`, `host-before-origin-and-route`, `host-websocket` |
| R1949 non-loopback refused; no wildcard in listeners | `docker-non-loopback-refused`, `bind-no-wildcard`; the "status never lists another address" check is non-transcript (H17 list) |
| body limit+1 with and without Content-Length; exactly at limit | `body-over-limit-length`, `body-over-limit-chunked`, `body-at-limit`, `expect-continue` |
| keep-alive, 5 s idle close, `Connection: close` | `keepalive-two-requests`, `keepalive-idle-close`, `keepalive-idle-before-first`, `connection-close`, `http10`, `pipelined` |
| ExclusiveBusy keeps listening; CollectionReplaced uses g+1; listener crash restarts | `exclusive-phase-listening`, `generation-change`, `listener-crash`, `handler-panic`, `revalidate-after-wake` |
| no server before collection open; launch path does no server work | `start-before-open`, `failed-open-listens`; the launch-path check is a non-transcript Rust test (H17) |
| toggle off ≤1 s; store survives restart; `col.mod` untouched | `toggle-off`, `config-persist` (`no_effect: true`), `store-newer-version`, `port-change`, `port-refused-values` |
| `resolve_interaction` unknown/expired no-op; timeout outcome | `interaction-unknown-id`, `interaction-timeout` |
| iOS export returns `Unsupported` | non-transcript: iOS simulator build test plus U6's symbol scan (H17, H20) |
| (not in plan list) | `bind-both-addresses`, `ipv6-unavailable`, `malformed-head`, `head-too-large`, `connection-limit` |

### U136

- Spec trailers: `servers/transcripts#T1`–`#T20`. It uses `conformance/dsl#D1`, `#D2`, `#D3`, `#D4`, `#D5`, `#D6` (with `#D6.8`, `#D6.9`), `#D7.1`, `#D7.2` and `#D10`–`#D13`; `conformance/observations#O1`, `#O2`, `#O4`, `#O6`, `#O7` and `#O8`; `conformance/normalized-dump#N2` and `#N5`; and `conformance/render-ops#Q5.4`.
- Recorder status (T18, T19): the recorder is committed under `oracle/ankiconnect/` (`5cae7aef`, "No tools/mg job yet"). T19.1 is settled: the driver add-on's `allow`, `deny` and `deny_ignore` give T7.2's replies and list changes, and `none` stays declared-only. T19.3 is settled: ids are deterministic, collide across kinds and are reused after a delete. T18's `version` and `ping` replies are settled (spec-ahead log, U136 section). T19.2 is still open and no probe can settle it: the pinned 25.11.9.0 archive cannot be checked byte for byte against the user's lost installed copy, though its 121 actions match. Not yet present: the `tools/mg servers-contract` job; `tools/mg oracle record` support for `servers/ankiconnect` (no `ankiconnect` hit under `tools/mg`, `tools/mg.d`, `tools/conformance` or `tools/gate`); an `oracle-ankiconnect` service in `docker/compose.yml`; `conformance/schema/server-transcript.schema.json`; and `ankiconnect_addon_sha256` and `ankiconnect_image` in `conformance/oracle.json` (T9's staleness rule needs them).
- Implementer scope: `core/crates/meguri-testkit/src/servers/{mod,transcript,normalize,equivalence}.rs`, `core/tools/meguri-cli/src/servers.rs`, the schema, deviation entries and the `tools/mg servers-contract` job, which must be placed in `tools/gate/tiers.yaml` in the same change (`docs/process/tiers.md`). The `oracle/ankiconnect/**` recorder and `tools/scripts/similarity-corpus-servers.sh`, which reads add-on sources, belong to the oracle maintainer and are not implementer work (plan U136 Files and Execution note; AGENTS.md roles).
- Unassigned U136 Files items: plan U136 Files also lists `docker/compose.yml` ("add `oracle-ankiconnect` service") and `tools/mg` "`oracle record` support for `servers/ankiconnect`", with no role. Both wrap the recorder image and `oracle/ankiconnect/record.sh`, whose interface is under `oracle/`, which implementers may not read. Proposed owner: the oracle maintainer. The orchestrator confirms the owner before U136 starts. The implementer does not write them.

| Plan test | Scenario / check |
|---|---|
| `version` and empty-body ping record and replay against recorder and stub | `servers/ankiconnect/harness/version`, `harness/ping` (T17 row 1, T18). Committed with goldens at `5cae7aef` under `conformance/recordings/` |
| two recordings byte-identical | T17 row 2 |
| wrong reply → report names scenario, step, pointer | T17 row 3 |
| deviation suppresses exactly its field | T17 row 4 |
| Migaku equivalence → oracle note matches; mutated field fails | T17 row 5 (T14). No scenario id is named, and `servers/migaku/harness/declared-srs-check` (T18) is the declared harness row |
| (spec extra) tokens, `unknown_ref`, `reused_ref`, deleted-id error text, unlisted id position | `harness/tokens` (committed); T17 rows 6–10 |

### U141

- Spec trailers: `servers/ankiconnect-envelope#E1`–`#E19` (E3 order of checks, E7 origins, E8 key, E9 `requestPermission`, E10 `apiReflect`, E11 `multi`, E12 registry, E13 read-only, E14 wait, E15 strings, E16 activity log, E17 snippet, E18 differences).
- Scenarios: E19.1 `envelope/` (36), E19.2 `policy/` (55) and E19.3 `multi/` with `apiReflect` (25) are all planned, none is committed and none is held. Rows marked "Rec." need oracle goldens. Rows marked "Meguri" are declared scenarios or `core/crates/meguri-servers/tests/` tests. Stand-in actions (one read, one mutating, one gui) go through `test-hooks` until U142 and U146 land (E19.4).

| Plan test | E19 id |
|---|---|
| AE75 `multi` with `deckNames` and `nope` | `multi/ae75` |
| AE76 origins; prompt; Deny+ignore | `policy/origin-extension`, `origin-none`, `origin-evil`, `origin-null`, `permission-deny-ignore`, `permission-deny-ignore-lists` |
| AE77 key | `policy/key-missing`, `permission-key`, `key-unset-supplied` (Meguri, E18) |
| version 4/5/6, errors always wrapped | `envelope/version-default`, `version-4`, `version-5`, `version-6`, `unsupported-v4` |
| invalid JSON, `action: ""`, `params: []`, missing/extra param | `envelope/invalid-json`, `action-empty`, `params-list`, `missing-param`, `extra-param` |
| localhost rule; list without `http://localhost`; `*` | `policy/origin-127-port`, `origin-https-localhost`, `origin-safari`, `list-empty`, `list-star` |
| OPTIONS + PNA | `policy/preflight-pna` (also `preflight`, `preflight-disallowed`, `preflight-pna-case`) |
| prompt timeout 120 s; Allow appends | `policy/permission-timeout`, `permission-allow`, `permission-allow-lists` |
| read-only | `policy/read-only`, `multi/sub-readonly` |
| ExclusiveBusy >30 s / 3 s; Check Database slot | `policy/busy-timeout`, `busy-short`, `slot-held` |
| key required, no key applied yet | `policy/key-pending` |
| CORS of preflight and `requestPermission` for a not-yet-allowed origin | `policy/preflight-disallowed`, `permission-allow` |
| `apiReflect` | `multi/reflect-all`, `reflect-filter`, `reflect-scopes-string` |
| activity log, agent snippet, fuzz | Meguri-only (E19.4) |

### U142

- Spec trailers: `servers/ankiconnect-actions#A1`–`#A20` (A5 decks, A6 note types, A7 adding, A8 update and delete, A9 notes query, A10 tags, A11 cards, A12 card state, A13 rescheduling, A14 stats, A15 preservation, A16 search, A17 texts, A18 deviations) and `servers/ankiconnect-clients#C3`, `#C5`–`#C7`.
- Scenarios: A20.1–A20.7 groups `decks/`, `models/`, `notes/`, `tags/`, `cards/`, `search/` and `stats/` are planned, none committed or held. `clients/` (A20.8 = clients C7): agent, asbplayer, mpvacious and yomitan are committed, `migaku` is held, and `none`/`card` are not written (§3 header). Not probed: `notes/preservation`, `cards/reschedule` against the UI ops, `search/current` with cards in B, and the too-deep deck error (A20 intro).

| Plan test | A20 / C7 id |
|---|---|
| `createDeck("A::B::C")`, repeat; `deleteDecks` | `decks/create-nested`, `decks/delete` |
| `addNote` field matching and errors; filtered deck | `notes/add-match`, `notes/add-errors`, `notes/add-filtered-deck` (Meguri) |
| duplicate scopes | `notes/dupe-scope` |
| `addNotes` one bad of three | `notes/add-notes` (Meguri `ankiconnect-addnotes-rollback`); `clients/yomitan/add-notes-all-or-nothing` |
| `canAddNotesWithErrorDetail` | `notes/dupe-scope`, `clients/yomitan/can-add-notes` |
| `updateNoteFields` case-sensitive, `updateNote` tags, `notesModTime`, preservation | `notes/update`, `notes/preservation` |
| `deleteNotes` graves + deletion log | `notes/delete` |
| `findNotes("über")`; `deck:current`; `"("`; ascending ids | `search/folding` (R1777 deviation), `search/current`, `search/error`, `search/empty` |
| `notesInfo` / `cardsInfo` FSRS | `notes/info`, `search/notes-info-query`, `cards/info` |
| `suspend` already suspended → false; `areSuspended` null; `getIntervals` negative | `cards/suspend` (restate per A21 Q3, §5), `cards/intervals` |
| `forgetCards`, `setDueDate "3-7"`, `"1!"` equal to UI ops | `cards/reschedule` |
| `getNumCardsReviewedByDay` 4 AM rollover | `stats/by-day-rollover`, `stats/today` |
| client inventory (R1945) | `clients/{yomitan,asbplayer,mpvacious,agent,none,card}/*` (C7) |
| AE75–AE77 with real actions | E19.3 `ae75` and the E19.2 `key-*` rows with `deckNames` and `addNote` (A20.8) |
| (spec extra) | `decks/names-ids`, `get-decks`, `name-from-id`, `change-deck`, `deck-stats`; `models/*`; `notes/add-no-card`, `add-defaults`, `can-add-media`; `tags/*`; `cards/info-errors`, `to-notes` |

### U143

- Spec trailers: `servers/ankiconnect-media#X1`–`#X14` and `#X16` (X3 sources, X4–X9 actions, X10 attachments, X11 external folder, X12 texts, X13 deviations, X16 observed add-on behavior). X16 is normative: "Meguri follows these observations except where X12 gives a Meguri text or X13 a deviation". X15 holds open questions only, so it gets no trailer. It relies on `media/media-files` M1, M3, M4, M5, M6, M8, M10, M13 and M14, and on `media/media-journal` J5.
- Scenarios: X14's `media/*` (14) are planned, none committed or held. The external-folder tests are Meguri-only (`ankiconnect_extmedia.rs`, X14 second table). The `url` stub tests are Meguri-only (`ankiconnect_media.rs`, X14).

| Plan test | X14 id / Meguri test |
|---|---|
| `storeMediaFile` base64, `skipHash`, `deleteExisting: false` | `media/store-data`, `media/store-conflict`, `media/skip-hash`; `clients/asbplayer/store-media-keep-existing` |
| `retrieveMediaFile` traversal, missing | `media/read` (also `media/store-traversal`) |
| `getMediaFilesNames("*.mp3")` | `media/names` |
| `deleteMediaFile` to trash, deletion pending | `media/delete`, `media/delete-names` |
| `path` inside a readable folder; outside or a symlink out → not permitted | `media/path` |
| `url` 6 redirects / `file://`; failing attachment URL → escaped text | Meguri stub tests; `media/attach-fail`; `clients/none/add-note-url-media` |
| `audio` + `picture` arrays appended in order | `media/attach` |
| AE79 core | extmedia "AE79 core"; `clients/mpvacious/add-note-media-dir`, `mpvacious/media-dir-pickup` (declared) |
| scan idempotent; re-ingest; case kept; mid-write | extmedia "idempotent scan", "re-ingest", "case kept", "mid-write" |
| grant baseline; reference ingests one; relaunch | extmedia "baseline", "baseline referenced", "closed app" |
| `addNotes` referencing a baseline file | extmedia "batch" |
| `path` inside `getMediaDirPath` folder; root via symlink `/tmp` | extmedia "path root"; the symlinked-root case has no X14 row (Meguri test in `ankiconnect_paths.rs`) |
| `getMediaDirPath` unset → error | `media/dir` |

### U146

- Spec trailers: `servers/ankiconnect-gui#G1`–`#G12`. It relies on `sync/policy#P3`, `#P12` (P12.1–P12.6) and `#P21` for G9.
- Scenarios: G12 lists 56 ids, none committed. The committed `clients/yomitan/gui-*`, `asbplayer/gui-add-cards`, `mpvacious/update-last-card` and `yomitan/sync-not-configured` cover client shapes. Window rows are asserted in `app/MeguriUITests/Integrations/GuiActionsUITests.swift`, which runs only in the macOS test VM (AGENTS.md).

| Plan test | G12 id |
|---|---|
| `guiBrowse` with `reorderCards noteCrt` descending | `browse-reorder` (+ `browse-reorder-undo`, `-read-only`, `-default-cols`) |
| `guiSelectCard`; `guiSelectedNotes` hidden | `select-in-results`, `select-no-browse`, `select-not-in-results`, `selected-none`, `selected-rows` |
| `guiAddCards` empty panel, draft Keep/Replace, `closeAfterAdding` | `add-empty-panel`, `add-draft-keep`, `add-draft-replace`, `add-close-after` |
| `guiEditNote(999)`; valid | `edit-unknown`, `edit-valid` |
| unsaved sheet refuses | `sheet-refuses`, `add-draft-refuses-browse`, `add-refused-keeps-media` |
| review session left | `review-left` (blocked, §5) |
| bring to front on/off | `front-on`, `front-off`, `front-off-closed`, `select-no-front` |
| `sync` with no iCloud; signed/fake lane | `sync-not-connected`, `sync-ok` (+ `sync-review`, `sync-read-only`, `sync-joins`, `sync-yomitan-params`, `sync-waiting-no-class`) |
| `/create` `searchNote` → Browse `nid:` | `search-note`, `search-note-huge` |

### U137–U140 (status pending; see §1)

- U137: `servers/migaku-protocol#P1`–`#P16`, 39 planned endpoint scenarios (P16), with tests in `migaku_endpoints.rs` until U136 lands. Plan tests map one to one onto P16 ids `preflight-*`, `srs-check-*`, `origins-*`, `ws-open-two`, `ws-ignored-frames`, `srs-import-routes`, `not-found`, `method-not-allowed` and `anki-hello`.
- U138: `servers/migaku-routing#D1`–`#D16`, 31 planned routing scenarios (D15.1). The synthetic corpus `conformance/corpora/migaku-routing-synthetic.jsonl` is **committed** (D15.2). AE71 is `tie-recent-route`. The private AE87 and R1718 gate is U249's.
- U139: `servers/migaku-receive#P1`–`#P24` and `servers/migaku-syntax#S1`–`#S9`. P24 lists 34 receive scenarios, among them AE72 `ae72-japanese`, AE73 `ae73-remote-404` and `ae73-slow-retry`. The syntax rules have unit tests only (S9).
- U140: `servers/migaku-received-log#L1`–`#L17` and `#C1`–`#C5`. It has 19 moves scenarios (L17, AE74 `ae74-cross`, `ae74-reviewed`) and 12 `/create` scenarios (C5). It adds `move_received_card` to the `undo_step` enum of `conformance/schema/observation.schema.json` (absent at HEAD) and `migaku_move` to `engine/op-model` O9 (absent). The V4 row is in conflict: the spec-ahead log ("Settled in this pass", L18 Q4) says V4 "now lists `move_received_card`" and that U140 "still adds" only the enum value and O9 entry. But `specs/conformance/undo-ops.md` line 98, inside V4, still reads "Pending Meguri-only row: U140 adds `move_received_card` …", and the V4 table has no such row. Ask the spec writer which source is right before U140 starts. Until then, follow line 98: U140 adds the V4 row with the engine's step name.

## 4. Testkit and adapter work

`core/crates/meguri-testkit` has no transcript support: there is no `src/servers/` and no hit for `transcript`, `host_env`, `apply_config`, `ws_open`, `media_dir_write`, `interaction`, `equivalence` or `fixture_base`. All of the following is new (U136), each defined in `specs/servers/transcripts.md`:

- File and top-level keys `id`, `description`, `tags`, `server` (`ankiconnect`|`migaku`|`host`), `source` (`oracle`|`declared`), `clock`, `compare`, `server_config`, `host_env`, `setup`, `steps` and `equivalence` (T2, T3). It needs its own schema, `conformance/schema/server-transcript.schema.json`.
- `server_config` members `cors_origins`, `deny_origins`, `api_key`, `read_only`, `extra_origins`, `media_dir`, `readable_folders`, `enabled` and `port` (T4). `host_env` members `ipv6`, `start: before_open` and `store_user_version` (T4.1).
- `http` step members `method`, `target`, `headers` (an ordered list, with a `fill` value), `body` forms (`empty`, `json` in exact compact serialization, `text`, `base64`, `fixture{path,sha256}`, `fill{size,byte,prefix}`), `connection`, `server`, `address` (`ipv4`|`ipv6`|`non_loopback`), `http_version`, `raw`, `expect_continue`, `async`, `no_host`, `chunks` and `declared_length` (T6.1–T6.3). Requests are written as raw socket bytes, never through an HTTP client (T6.2).
- Connection steps `pipeline`, `open{name,count}`, `close` and `close_wait` (T6.4).
- Steps `advance`, `observe`, `await` and `media_dir_write` (T7.1); `permission{answer}` (T7.2); `apply_config`, `config`, `lifecycle` (`exclusive`, `ready`, `long_op`, `long_op_end`, `close`, `open`, `open_fail`, `{replace}`), `restart_host`, `helper`, `fault`, `revalidate`, `resolve`, `events`, `sockets`, `logs`, `media_dir_scan`, `wait`, `ws_open`, `ws_send`, `ws_expect`, `status{until,within_ms}` and `parallel` (T7.3); `interaction{kind,answer}` with per-kind defaults (T7.5).
- Reply model (T8): the compared header set, body classification, `closed`, `interim` and the `bad_framing` check. Golden format and staleness (T9).
- Normalization (T10): entity tokens `#dN`/`#mN` alongside `#nN`/`#cN`, retired tokens, `#unknown`, tokens inside error strings by message kind, request substitution `${#nN}`/`${port}`/`${fixture_base}`/`$${`, the `reused_ref` error, `mod` → `@n`, `<fixture>`, `<media-dir>`, `<profile>`, `#rand` and U+2068/U+2069 removal in `nextReviews`; the T10.5 id-position table.
- Declared `expect` members (T13): `status`, `headers` with `null` meaning absent, `body`, `closed`, `connection_closed_before`, `outcome`, `within_ms`, `not_before_ms`, `interim` and `no_effect`.
- Equivalence (T14): build a DSL scenario of `media_files` plus `add_note` steps from the add records and compare `db_dump` with N2 masks.
- Deviation transforms `absent: true` and `any_string: true`, and an optional `tags` filter per entry (T12). Neither `conformance/schema/deviations.schema.json` (only `replace`, `apple_overlay`) nor testkit's `Transform` (`src/suite.rs`) has them.
- Failure report `runner: "servers"`, statuses and the job filters (T15, T16).
- DSL setup gap: transcript `setup` is `conformance/dsl` D6 unchanged (T5), but testkit's `Setup` (`src/scenario/model.rs`, `ORDER` stops at `media_files`) does not parse `http_fixtures` (D6.9, draft; present in `conformance/schema/scenario.schema.json`) or `url_files` (D6.10). `clients/none/add-note-url-media` and every `${fixture_base}` scenario need D6.9.
- Host test construction for the executor: `bind` `Ephemeral`|`Fixed`, test clock, `TestHost`, the test hooks and socketless serving (`servers/host-security#H22`). This is U135 work that U136 consumes.

## 5. Open questions and decisions

Source: `docs/process/review-log/spec-ahead-servers-open-questions-20261110.md`, unless another source is named.

Blocking:
- **U142, A21 Q2** blocks A8. The missing-note text is the add-on's `Note was not found: <id>` / `Card was not found: <id>` against R1738's lower case. Interim: the add-on's text. Recommended: amend R1738. `guiEditNote` keeps the lower case either way (G7).
- **U142, A21 Q3** blocks A12.1 and A12.2. `suspend` returns `true` for two or more already-suspended cards, and `unsuspend` returns `null` (the add-on), against R1767. Interim: the add-on's results. The plan test must be restated as "a one-card list of an already suspended card returns false".
- **U146, G2 rule 6** blocks `review-left`. The screen shown when a navigating action ends a review is U150/U175's and is unspecified.
- **U137–U140 scope**: plan change not yet decided (`migaku-via-ankiconnect-20261110.md`).
- **R1679 vs R1744** (clients C9.6) may block `card/allowed-when-listed`. If the card webview sends `Origin: null`, the opt-in cannot work. Recommended: give the webview a non-opaque origin.

Not blocking (interim rule in the spec):
- U135: H18.3 proxies (direct; keep); H18.6 128 connections and 10 s head timeout (confirm); H18.7 listen after a failed open (confirm); H18.1, H18.2, H18.4 and H18.5 are U147 facts. Plan product decision: no destination restriction on remote media fetches (plan "Deferred product decisions", Remote media destinations).
- U141: E7.3 no-Origin ACAO `*` vs the add-on's `http://localhost` (recommended `http://localhost`, which drops that E18 row and the R1739 `no-origin` deviation); E7.2/E9 `null`, empty and repeated Origin refused even with `*` (confirm); E12.3 `sync` and `guiAddCards` count as mutating (confirm); `guiBrowse` with `reorderCards` refused in read-only mode (confirm).
- U142: A21 Q1 profile name `User 1` (if chosen, the recorder runs as `User 1`); Q4 review-count exclusions (deviation `ankiconnect-review-counts-exclusions`); Q5 the A17 texts; Q7 no "no card generated" refusal; Q8 the cloze text; Q9 string `tags` gets a member error; Q10 `list index out of range` for a card with no revlog (the add-on's text interim); A18 `ankiconnect-canadd-no-media` should also name the `false` reply.
- U143: X15 Q7 a failing attachment without `fields` fails the note (amend R1762); Q8 ingest notices are a host event, not activity log (the plan says activity log; follow X11.9); Q9 relocation call (not now); Q1–Q4 texts, the 256 MiB `path` cap, copy-only, dot-names; Q6 the X8 label must match U144's screen.
- U146: G6 filtered `deckName` (recommended: refuse at once); G2 rule 5 bring-to-front scope; G2 rule 4 selection refused under a sheet; G6 reply moment; G9 `sync:` texts and no timeout; G11 sort direction not probed; G2 note on asbplayer's double `guiBrowse`.
- U136: T19.5 real extension ids (recommended: no); T19.2 is open as a fact. The log's three recorder items are stale or need a re-check, because the recordings were rewritten after the log and committed at `5cae7aef`:
  - `asbplayer/request-permission-key` "was not probed" is out of date. The recording exists (`.yaml` 15:21, `.json` 15:22) with `server_config: {api_key: k1}`, a granted `requestPermission` reply with `requireApikey: true`, and `valid api key must be provided` for `deckNames` without the key.
  - "record `mpvacious/find-models-by-name` again (`did` null on both sides)": the copy was rewritten at 15:23, and its golden still shows `"did": null`. Ask the oracle maintainer whether that re-recording settles the item. Do not assume it does.
  - C9.12 (fix the browser selection before recording `mpvacious/update-last-card`): that recording is committed too. Confirm the same way.
- U137–U140 (if kept): P17.8; D17.3, D17.9, D17.11, D1 `it`; P25 Q1 pending reply (switch off until recorded; plan Open Questions), Q2, Q4; L18 Q1–Q3, Q5–Q7; S10 Q1–Q6. P17.1 is not pursued (Migaku decision).

## 6. Traps

- Transcript scenarios are **not** DSL scenarios. They have their own schema, runner and job (T1). The transcripts sit under `conformance/recordings/servers/`, not `scenarios/` and `goldens/`, until the DSL suite and goldens-check skip `servers/` (`conformance/AGENTS.md`, committed at `5cae7aef`). T2, T9 and T16 say only `tools/mg oracle record servers/ankiconnect` writes goldens, but that job does not exist yet. Until it does, `oracle/ankiconnect/record.sh`, run by the oracle maintainer, writes them (`conformance/AGENTS.md`: "a `tools/mg` job later"). Either way, implementers never write or edit a golden, and never bless Meguri output.
- The recorder reuses freed ids and gives equal ids across kinds. Do not reuse a retired token after a new entity of its kind exists (`reused_ref`), and do not delete before adding when listing notes or cards unless they are compared as a set (T10.1, T10.2).
- `server: ankiconnect|migaku` scenarios bind `127.0.0.1:0` (ephemeral: no probe, no `[::1]`). Only `server: host` uses the fixed ports, each in its own network namespace (T20, H22). Fetch and connection limits run in real time; host timers follow `advance` (H21).
- Oracle scenarios never send `Transfer-Encoding`: the add-on reads chunked bodies as a ping. `closed` is never compared for oracle scenarios (T11, T12).
- No-Origin oracle scenarios need the tag `no-origin` for the R1739 deviation (T12; clients C5). The held `clients/migaku/*` use the tag `client:migaku` and shapes that no committed spec lists yet (clients C2 and C5 name only yomitan, asbplayer, mpvacious, agent, card and none).
- T10.5's id-position table has no rows for many actions. U141–U143 add rows in `specs/servers/transcripts.md` with their spec change when they add actions to scenarios (T10.5). An unlisted id stays raw and the recorder warns.
- Transcript setup is D6 only (T5). The rows that need a filtered deck (`decks/create-nested`, `decks/change-deck`, `notes/add-filtered-deck`, gui G6), raw database writes (`notes/info` n2, `cards/info-errors`, `notes/preservation`) or a set `curDeck` (`search/current`) need a setup form that T5 and D6 do not define (dsl D6; `specs/conformance/deck-ops.md` K13 uses `db_exec` for filtered decks). Raise it with the spec writer before writing those rows.
- Plan U142's test says `deck:current` "resolves against the pushed selected deck". The spec and plan Approach use `curDeck` and never the `UiContext` deck (A16, `search/current`). Follow the spec.
- API adds never write `curModel`, `_nt_*_lastDeck` or `_deck_*_lastNotetype`; the add-on does (A18 `ankiconnect-add-defaults`; plan U139 U27 contract). `guiAddCards` writes no config either (G11).
- `addNotes` is one op, all or nothing, with no graves. The add-on adds the passing notes and then deletes them (A18 `ankiconnect-addnotes-rollback`). `multi` is **not** atomic: each sub-action is its own op (E11).
- Error strings come from one constants module (plan U141), the A17, X12 and G2 tables and E15. Never format them ad hoc. `invalid columnId`/`invalid card order` values use compact JSON, not runtime text (G2).
- `apiReflect` and the agent snippet are generated from the registry, which holds exactly E12.3's 73 actions. E12.4's 48 reference actions reply `unsupported action` (E12, E19.3 `reflect-all`).
- Media: `/tmp` is a symlink on macOS but not in Linux Docker, so the symlinked-root test must create its own link. Media names are case-insensitive in Meguri on every volume, while the add-on's volume is case-sensitive (X13 `ankiconnect-media-case-variants`, `conformance/AGENTS.md` media bullet). `store-traversal` expects `"collection.anki2"` stored as media (last component kept).
- `media_folder_granted` records a baseline only after a user pick, never at launch-time bookmark resolution (plan U143; X11.2).
- Fault points use `meguri_model::faultpoint!` with names from U250's catalog. `test-hooks` is never a default feature (`core/AGENTS.md`).
- Engine layering: only meguri-storage writes SQL against `collection.anki2` (`core/AGENTS.md`; `core/tools/layering-check/edges.toml` row 3). meguri-servers never issues SQL against the collection. It does own its own SQLite file, `meguri-integrations.db`, which U135 creates and writes (`servers/host-security#H10.1`, plan U135 Approach). Servers open collections through `CoreHost::open`. Handlers are thin adapters to engine ops of `source=LocalApi` and server readers (`core/AGENTS.md`; `engine/op-model` O1; A15).
- FFI exports must be identical on every slice. On iOS they are `Unsupported` stubs, and the iOS slices must contain no axum or meguri-servers symbols (plan U135 Verification; H20).
- Mac UI suites (U146 `GuiActionsUITests`, U144) run only in the macOS test VM and never drive this Mac (AGENTS.md).
- The servers specs were never similarity-scanned: the scan skips `specs/**`, and the AnkiConnect and Migaku corpus does not exist before U136 (spec-ahead log, "Process"). Run the R1801 similarity check on implementation code as usual.
