# CardRuntime

What every card page loads (specs/render/card-runtime.md, U66): `page.html` (skeleton with `{{…}}` markers that `CardRuntime.pageDocument` fills), `base.css`, `runtime.js` and `image-occlusion.js` (separate page-world IIFEs), `interaction.js` (Meguri's content world), upstream jQuery. Tests: `Tests/CardRenderingTests/Runtime/`.

## Traps

- **C9 surface only.** Keep each page-world asset inside its own immediately invoked expression; only C9.1 names may become page globals. Define `pycmd` and `bridgeCommand` as non-configurable. `GlobalsSurfaceTests` also checks the source indentation.
- **Classic-script semantics** (C6.5). Never change a card script's `type`, add `"use strict"` or a wrapper, snapshot the script list (the walk reads a live collection by index), or re-run a script with `cloneNode`. Copies are fresh elements with cloned `Attr` nodes (`setAttributeNode`: no name validation) and the text nodes' data. After each script the walk awaits one microtask, so the promise jobs it queued run before the next script.
- **Script kinds are WebKit's** (C6.5 waits only for `src` scripts the engine fetches). WebKit strips ASCII whitespace around a JavaScript `type` but not around `module`, so `type=" module "` is a data block there, unlike in the HTML standard; a `language` value is never stripped. `ScriptSemanticsTests.walkWaitsExactlyForTheScriptsTheEngineFetches` checks each form against the engine: add new forms there.
- **Run first.** `runtime.js` is the first script in `<head>`; its window capture listeners (IME state, media hold, `meguri:native`, `meguri:query`, errors) exist before any card script, `reviewExtra` included. `interaction.js` is injected at document start, so its `meguri:signal` listener is the first on window and stops the event: card scripts never see signals.
- **Built-ins are captured at start.** Card scripts replace prototypes (`addEventListener` is wrapped by the runtime itself). Call DOM and JS built-ins through the captured functions (`call(fn, self, …)`), never through properties a card can reach later.
- **Handler properties are per world.** `onclick` set in the page world reads `null` from the interaction world: the registry reads them in the page world.
- **Page-world handlers:** only iOS `cb` (native registers it). `meguriCard` and `meguriVisible` exist in the interaction world only; the Mac page world has no `window.webkit`.
- **The page document loads at the media origin root** (`meguri-media://media/`), not under `/_anki/`: WebKit resolves `import()` in inline scripts against the document URL and ignores `<base>`, so a page at `/_anki/page.html` sends `import("./_m.js")` to `/_anki/_m.js` (R520). Assets stay under `/_anki/`. The S2 decision record and spec C2.1 agree.
- **No Anki assets** (R1960). Upstream jQuery 3.7.1 and MathJax 4.1.3 are unmodified. MathJax is lazy, offline and confined to the bundled component allowlist; editor/storage TeX stays literal. The owner's revised C10.8 permits base TeX, AMS (including align/cases) and mhchem only. `require`/autoload implement that policy; optional packages compiled into the combined engine must stay disabled. Failed expressions retain delimited source beside a visible/accessibility label.
- **`base.css` values are exact text** (C4); do not normalize colours. The order is Meguri's (tokens alphabetical, flags descending, rules grouped): reorder again if the similarity scan flags a region. The Mac focus rule's selector is `.isMac:not(:root)`, not the descendant form, which matched an Anki stylesheet's text (U66-E1): same elements and specificity, which `BaseCSSTests` pins. The Mac's WebKit drops `-webkit-text-size-adjust` (no text autosizing); iOS applies it.

## Channel (C14)

DOM events on `document` with a JSON string `detail`: `meguri:signal` (runtime → interaction), `meguri:native` (interaction → runtime: `identity`, `present`, `unpresent`, `warm`), `meguri:query` (dispatched on the target element; the runtime cancels it when no card listener could see the input, so a missing response permits the event).

Native → page, in the interaction world: `meguriCard.identity(card, generation)` before `_showQuestion`/`_showAnswer`, `present(token)`, `unpresent()`, `warm(html)`, `hasCardListener("tap"|"key"|type, element?)`, `setAppearance(dark, mac)` (C5's class switch; the web view's own appearance is native's).

Page → native: `meguriCard` messages `{signal, side, card?, generation?, …}`: `settled`, `shown`, `visible {token}`, `updateFailed {error}`, `command {command}` (pycmd) or `{reveal: true}` (`_typeAnsPress`), `error {kind, message, line?, column?, source?, phase, index?}`, `mediaError {tag, url, family?}`, `selection {selected}`, `focus {editable, typeans}`, `composition {active}`, `typedText {text}`. `meguriVisible` gets U12's `{token, card, phase}`: `settled` with the generation as token, `visible` with the swap's token. Everything from the page is untrusted (R2038).

U67's observer retains emission identity (detached nodes included), and isolated `mediaPlayback {playing}` balances A7 ducking. `mediaArrived(names)` retries attributes/CSS without invoking the renderer. The internal `resume` operation discards HTML playback attempts held during suspension before restoring presentation; normal `present` retains C8.2. No page globals or side/script semantics change.

## Image occlusion (U68)

- Implement from `render/image-occlusion-review` only; no Anki assets or editor dependencies. Only `anki.imageOcclusion` and its exact `anki.setupImageCloze` alias are added.
- Capture the already-installed listener registry's add/remove: IO key/button listeners count as card handlers. No tap/key listener or exempt style on the canvas, image or container.
- `setup` fulfils before drawing; its detached frame work reports through C12. Never add a settling barrier or extend C6.6's budget. External `#err` belongs to the template.
- Preserve Number/parseFloat differences, signed remainder, point strings, rounding and M6 operation order. The common 4096 reduction bounds both dimensions (D5).
- Options stay live: read hooks at each pass. Each setup resets hidden state; key/redraw use latest options, each button listener its own call's options. Resize/DPR redraws neither wire buttons nor reset hidden state.
