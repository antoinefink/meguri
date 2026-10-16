# CardRuntime

What every card page loads (specs/render/card-runtime.md, U66): `page.html` (skeleton with `{{…}}` markers that `CardRuntime.pageDocument` fills), `base.css`, `runtime.js` (page world), `interaction.js` (Meguri's content world), upstream jQuery. Tests: `Tests/CardRenderingTests/Runtime/`.

## Traps

- **No extra page globals** (C9). `runtime.js`'s top level is one call of a function expression; globals come only from the C9.1 assignments, plus `pycmd`/`bridgeCommand` defined non-configurable (as by `var`). Any helper a card can see, whatever its name, breaks `GlobalsSurfaceTests`, which also checks the top level as text (every line between the first and last is indented).
- **Classic-script semantics** (C6.5). Never change a card script's `type`, add `"use strict"` or a wrapper, snapshot the script list (the walk reads a live collection by index), or re-run a script with `cloneNode`. Copies are fresh elements with cloned `Attr` nodes (`setAttributeNode`: no name validation) and the text nodes' data. After each script the walk awaits one microtask, so the promise jobs it queued run before the next script.
- **Run first.** `runtime.js` is the first script in `<head>`; its window capture listeners (IME state, media hold, `meguri:native`, `meguri:query`, errors) exist before any card script, `reviewExtra` included. `interaction.js` is injected at document start, so its `meguri:signal` listener is the first on window and stops the event: card scripts never see signals.
- **Built-ins are captured at start.** Card scripts replace prototypes (`addEventListener` is wrapped by the runtime itself). Call DOM and JS built-ins through the captured functions (`call(fn, self, …)`), never through properties a card can reach later.
- **Handler properties are per world.** `onclick` set in the page world reads `null` from the interaction world: the registry reads them in the page world.
- **Page-world handlers:** only iOS `cb` (native registers it). `meguriCard` and `meguriVisible` exist in the interaction world only; the Mac page world has no `window.webkit`.
- **The page document loads at the media origin root** (`meguri-media://media/`), not under `/_anki/`: WebKit resolves `import()` in inline scripts against the document URL and ignores `<base>`, so a page at `/_anki/page.html` sends `import("./_m.js")` to `/_anki/_m.js` (R520). Spec C2.1 still says `/_anki/`; its update is pending. Assets stay under `/_anki/`.
- **No Anki assets** (R1960). Only upstream jQuery 3.7.1, unmodified (hash in `THIRD_PARTY_NOTICES.md`).
- **`base.css` values are exact text** (C4); do not normalize colours. The order is Meguri's (tokens alphabetical, flags descending, rules grouped): reorder again if the similarity scan flags a region. The Mac's WebKit drops `-webkit-text-size-adjust` (no text autosizing); iOS applies it.

## Channel (C14)

DOM events on `document` with a JSON string `detail`: `meguri:signal` (runtime → interaction), `meguri:native` (interaction → runtime: `identity`, `present`, `unpresent`, `warm`), `meguri:query` (dispatched on the target element; the runtime cancels it when no card listener could see the input, so an unanswered query reads as yes).

Native → page, in the interaction world: `meguriCard.identity(card, generation)` before `_showQuestion`/`_showAnswer`, `present(token)`, `unpresent()`, `warm(html)`, `hasCardListener("tap"|"key"|type, element?)`, `setAppearance(dark, mac)` (C5's class switch; the web view's own appearance is native's).

Page → native: `meguriCard` messages `{signal, side, card?, generation?, …}`: `settled`, `shown`, `visible {token}`, `updateFailed {error}`, `command {command}` (pycmd) or `{reveal: true}` (`_typeAnsPress`), `error {kind, message, line?, column?, source?, phase, index?}`, `mediaError {tag, url, family?}`, `selection {selected}`, `focus {editable, typeans}`, `composition {active}`, `typedText {text}`. `meguriVisible` gets U12's `{token, card, phase}`: `settled` with the generation as token, `visible` with the swap's token. Everything from the page is untrusted (R2038).
