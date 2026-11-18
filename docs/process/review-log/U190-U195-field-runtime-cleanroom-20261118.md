# U190 / U195 reusable field runtime — source-only handoff

Requested upstream base: `b14752095`. The supplied isolated checkout has only
the synthetic starting commit `b2bbf470ea2f8c19968490cb99565db4d9227332`
(“Permitted source baseline; no upstream history”). The upstream commit cannot
be resolved here. Role: implementer. No commits or upstream master updates.

## Scope and entry decision

This implements the user's explicit fallback: a Meguri-authored field runtime,
its Swift host, and its connection to an actual collection-bound Add draft.
It does **not** attach Home, the empty deck, or any other normal Add entry.
U190 and U195 remain incomplete. There is no alternate plain-text editor,
preview route, or temporary Basic-only writer.

Two existing seams prevent calling the production entry safe:

- `core/crates/meguri-ffi/src/notes.rs`'s `add_note` resolves the current process
  host, and its request has no expected collection generation, note-type layout
  or normal-deck precondition. An app check before awaiting this call cannot
  protect against host replacement or a reordered type/deleted deck before the
  writer runs. U27 intentionally accepts missing/filtered deck ids and can
  place cards elsewhere (`notes/note-records` A1). Checking the returned
  generation would detect a wrong write only after it happened.
- `ReviewCollection`'s entry/manual/provider activation and close/background
  paths have no editor flush/close admission participant. `AppNavigation.sheet`
  alone supplies neither the A7 guard nor the reservation that replacement
  must retain until editors close. Wiring only a new sheet would bypass these
  existing lifecycle owners. The shared asynchronous editor admission and
  writer-time identity checks must precede a production Add route.

The controlled test invokes U27 directly to establish the runtime-to-core
composition in one isolated test host. That is test evidence to obtain, not
a production admission implementation or a claim of an end-to-end UI journey.

## Implemented source

- `NoteEditorDraft` retains persisted field definitions, original stored-byte
  baselines, committed targets, mode/collapse/focus state and pending input.
  `AddNoteDraft` is held through the existing Navigation feature owner. Its
  preparation queries real D1 with the review card/deck preset and reads the
  actual `NotetypeService` record and generation-bearing normal-deck snapshot.
  Nonblank drafts retain their content/type; a valid preset changes only deck.
  Preparation is read-only and is explicitly not mutation admission.
- `FieldEditorHost` retains one WKWebView across layout mounts and transfers
  commits into that same draft. A held flush waits for prior sequence numbers
  and target conversion. Malformed types/keys/ordinals/booleans are refused.
  The bridge is confined to the editor content world, web view, origin and main
  frame. The existing `editor.openToEditable` interval ends on `loaded`.
- The runtime implements the field load/baseline/serialization and rich/HTML
  mode path, per-field idle/blur/flush commits, provisional Return/line-break
  handling, WebKit space editing, field histories and Tab/caret handling.
  Untouched fields retain stored bytes. HTML fragments are parsed inertly;
  scripts, handlers and links remain stored. Each rich field has its own shadow
  root, inside a paint/layout containment boundary outside the labels.
- A new pure `editor_field_html` bridge composes the existing media-name
  grammar with H3's NUL/empty handling and quote-safe decoding. No regex or
  HTML parser is duplicated in Swift. The instrumentation registry and its
  exact expectation include this export.
- Note-type snapshots now expose original stock kind and parsed cloze field
  ordinals. Image occlusion is refused before fields load, without guessing
  from names. D6 carry-over and D4 unchanged-state calculation preserve draft
  bytes before normalization and retain the selected deck/type/tags.
- The editor has a separate nonpersistent WebKit configuration and bundled
  document. CSP refuses page scripts, handler execution, remote/local resources,
  frames, forms and connections. Native navigation policy refuses subsequent
  navigation. The script-enabled review configuration is not reused.

The field inputs necessarily use WebKit under U190. The CSS containment and
editable minimum line area serve field isolation/editability; the persisted
field font/size/direction serve F2. No native Add sheet/panel composition has
been introduced or visually approved. The mount documents WebKit as the single
field scroll/focus owner; do not embed it in another scroll view or dispose it
on a layout change.

## Remaining work

Production Add admission and writer identity fences above; Home/empty-deck
entries; native iPhone large sheet/iPad page sheet/Mac resizable trailing
panel; shared guarded close/quit/background/replacement integration; D5 live
selection repair; note-type/deck choosers and D2 selection changes; stored pin
toggles/current options; Add confirmation/refusal/error/toast/undo integration;
tags and suggestions; History/Browse; native command registry and collection
undo routing; cross-editor session mode/collapse memory; Dynamic Type and full
accessibility/native keyboard evidence. The runtime uses disabled U192 option
flags rather than pretending the absent config option service is available.

The review media loader does not fit this editor's script/resource policy.
This slice blocks media display/resource loading and does not offer media
editing, image occlusion, formatting, paste/drop or History actions. Their
markup is retained. No Edit or Browse persistence route is implemented; S2's
atomic field-patch companion remains absent. Recovery from actual web-process
loss is explicit and restores committed targets/baselines; it cannot restore
the process's final uncommitted input. The full H4/H5 native-key/golden matrix,
F5 native menu fallback and F7 layout/process scenarios still need verification.

## Review / verification log

Author source review corrected malformed focus-leave payload handling,
sequence duplicates while a prior conversion is in flight, late/future load
admission, resource-load error propagation, recovery admission and the test's
study-count property. Existing fixtures, runners, gates, settings and specs are
unchanged. The only existing test edit is the required bridge-name registry
expectation; the existing package test target gains the Editor dependency.

Added, **not run**: `FieldEditorTests` for malformed payloads, real-runtime HTML
preservation/isolation/held input, sticky carry-over and obsolete/occlusion
refusal; `ReviewCollectionStartupTests.preparedAddDraftFlushFeedsOneDurableU27WriteAndExistingStudy`
for real Start Empty → D1/deck preset → field runtime/held flush → U27 → cold
reopen and existing Study; text-level H3 quote/empty/NUL regression source.

No builds, app/runtime launches, tests, network, private inputs, oracle,
reference or third-party editor source were used. Independent review,
privacy/provenance/similarity checks, compilation, focused execution and the
required integration/native gates belong to the integrator and remain pending.
The added export and snapshot members require regenerated Core bindings through
the existing MG build path; generated bindings were not edited in this runner.
The existing failed full-gate register is unchanged; this source handoff clears
no failure or milestone. No acceptance claim is made.
