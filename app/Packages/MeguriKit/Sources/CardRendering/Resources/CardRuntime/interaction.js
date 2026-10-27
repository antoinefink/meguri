// Meguri card interaction script: runs in Meguri's own content world, which
// shares the card page's DOM but none of its JavaScript (KTD11). Written for
// Meguri from specs/render/card-runtime.md C14 (U66). It forwards the page
// runtime's signals to native, reports what it observes in the DOM, and is
// native's way into the page: `meguriCard` below lives on this world's
// window only, out of card scripts' reach.
(function () {
  "use strict";

  const SIGNAL_EVENT = "meguri:signal";
  const NATIVE_EVENT = "meguri:native";
  const QUERY_EVENT = "meguri:query";
  const SIGNALS = "meguriCard";
  const CONFIRMATIONS = "meguriVisible";
  // Signals between the runtime and this script that native never sees.
  const INTERNAL = { started: true, visibleReady: true };
  const MEDIA_TAGS = { img: true, audio: true, video: true, source: true, link: true, object: true, embed: true, script: true };

  // The side of the update that started most recently.
  let side = null;
  let identity = null;
  // Only the current presentation may finish its delayed paint callback.
  let paintOwner = null;
  const owners = new WeakMap();
  const failed = new Map();

  function observed(signal, element) {
    const owner = element && owners.get(element) || identity;
    if (side !== null) signal.side = side;
    if (owner) {
      signal.card = owner.card;
      signal.generation = owner.generation;
      signal.side = owner.side;
    }
    post(SIGNALS, signal);
  }

  // Keep the identity of detached old nodes: an error/input queued before
  // re-prepare must never acquire the new update's identity at receipt.
  new MutationObserver(function (records) {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (!(node instanceof Element)) continue;
        if (identity) owners.set(node, identity);
        for (const child of node.querySelectorAll("*")) {
          if (identity) owners.set(child, identity);
        }
      }
    }
  }).observe(document, { childList: true, subtree: true });

  function post(handler, message) {
    try {
      window.webkit.messageHandlers[handler].postMessage(message);
    } catch (error) {
      // No handler: a page outside a card surface.
    }
  }

  function isNumber(value) {
    return typeof value === "number" && value === value;
  }

  function toRuntime(message) {
    document.dispatchEvent(new CustomEvent(NATIVE_EVENT, { detail: JSON.stringify(message) }));
  }

  // ---------------------------------------------------------------------------
  // The runtime's signals. This listener is the first on window, so card
  // scripts never see them.

  window.addEventListener(SIGNAL_EVENT, function (event) {
    event.stopImmediatePropagation();
    let signal;
    try {
      signal = JSON.parse(event.detail);
    } catch (error) {
      return;
    }
    if (signal === null || typeof signal !== "object" || typeof signal.signal !== "string") return;
    switch (signal.signal) {
      case "started":
        paintOwner = null;
        side = signal.side;
        identity = isNumber(signal.card) && isNumber(signal.generation)
          ? { card: signal.card, generation: signal.generation, side: side } : null;
        selected = false;
        focusState = "none";
        failed.clear();
        break;
      case "visibleReady":
        confirmVisible(signal);
        break;
      case "settled":
        // U12's confirmation contract: the prepare generation as the token.
        if (isNumber(signal.card) && isNumber(signal.generation)) {
          post(CONFIRMATIONS, { token: signal.generation, card: signal.card, generation: signal.generation, side: signal.side, phase: "settled" });
        }
        break;
      default:
        break;
    }
    if (!INTERNAL[signal.signal]) post(SIGNALS, signal);
  }, true);

  // Two frames after presentation, the content has painted (R1862).
  function confirmVisible(ready) {
    const owner = {};
    paintOwner = owner;
    requestAnimationFrame(function () {
      if (paintOwner !== owner) return;
      requestAnimationFrame(function () {
        if (paintOwner !== owner) return;
        paintOwner = null;
        const visible = { signal: "visible", token: ready.token, side: ready.side };
        if (isNumber(ready.card)) {
          visible.card = ready.card;
          visible.generation = ready.generation;
        }
        post(SIGNALS, visible);
        if (isNumber(ready.card) && isNumber(ready.token)) {
          post(CONFIRMATIONS, { token: ready.token, card: ready.card, generation: ready.generation, side: ready.side, phase: "visible" });
        }
      });
    });
  }

  // ---------------------------------------------------------------------------
  // What this world observes.

  // Failed loads of media and other subresources (R498). The runtime's own
  // preload copies are left out.
  window.addEventListener("error", function (event) {
    const target = event.target;
    if (!(target instanceof Element) || !MEDIA_TAGS[target.localName]) return;
    if (target.hasAttribute("data-meguri-preload")) return;
    const url = target.currentSrc || target.src || target.href || target.data || target.getAttribute("src") || "";
    if (failed.size >= 256) failed.delete(failed.keys().next().value);
    failed.set(target, String(url));
    observed({ signal: "mediaError", tag: target.localName, url: String(url) }, target);
  }, true);

  document.fonts.addEventListener("loadingerror", function (event) {
    const faces = event.fontfaces || [];
    for (let i = 0; i < faces.length; i++) {
      observed({ signal: "mediaError", tag: "font", url: "", family: String(faces[i].family) }, faces[i]);
    }
  });
  document.fonts.addEventListener("loading", function () {
    for (const face of document.fonts) {
      if (face.status === "loading" && !owners.has(face) && identity) owners.set(face, identity);
    }
  });

  // A bounded state report only, so native can balance shared ducking with
  // HTML media as well as its AV queue. No new page-world API.
  let htmlPlaying = false;
  function playbackChanged() {
    let playing = false;
    for (const media of document.querySelectorAll("audio,video")) {
      if (!media.paused && !media.ended) { playing = true; break; }
    }
    if (playing !== htmlPlaying) {
      htmlPlaying = playing;
      observed({ signal: "mediaPlayback", playing: playing });
    }
  }
  for (const name of ["playing", "pause", "ended", "emptied", "error"]) {
    window.addEventListener(name, playbackChanged, true);
  }

  // Whether a selection exists (R406), reported when that changes.
  let selected = false;
  document.addEventListener("selectionchange", function () {
    const selection = document.getSelection();
    const now = !!selection && selection.rangeCount > 0 && !selection.isCollapsed;
    if (now === selected) return;
    selected = now;
    observed({ signal: "selection", selected: now });
  });

  // Focus in editable elements (R425), reported when that changes.
  let focusState = "none";
  function editable(element) {
    if (!(element instanceof Element)) return false;
    const name = element.localName;
    return name === "input" || name === "textarea" || element.isContentEditable === true;
  }
  function focusChanged(element) {
    const isEditable = editable(element);
    const isTypeans = isEditable && element.id === "typeans";
    const state = isEditable ? (isTypeans ? "typeans" : "editable") : "none";
    if (state === focusState) return;
    focusState = state;
    observed({ signal: "focus", editable: isEditable, typeans: isTypeans }, element);
  }
  window.addEventListener("focusin", function (event) {
    focusChanged(event.target);
  }, true);
  window.addEventListener("focusout", function (event) {
    focusChanged(event.relatedTarget);
  }, true);

  // IME composition (R535).
  window.addEventListener("compositionstart", function (event) {
    observed({ signal: "composition", active: true }, event.target);
  }, true);
  window.addEventListener("compositionend", function (event) {
    observed({ signal: "composition", active: false }, event.target);
  }, true);

  // What is typed in #typeans (R538 continuity; the answer reads C10.3).
  window.addEventListener("input", function (event) {
    const target = event.target;
    if (target instanceof Element && target.id === "typeans") {
      observed({ signal: "typedText", text: String(target.value) }, target);
    }
  }, true);

  // ---------------------------------------------------------------------------
  // Native's way into the page.

  window.meguriCard = {
    // The identity of the next update native queues (C6.8).
    identity: function (card, generation) {
      toRuntime({ op: "identity", card: card, generation: generation });
    },
    // U67's native keyboard owner gates only automatic type-answer focus.
    setFocusOwnership: function (owned) {
      toRuntime({ op: "focusOwnership", owned: owned === true });
    },
    restoreTypedText: function (text) {
      toRuntime({ op: "restoreTypedText", text: String(text) });
    },
    // The page is now the one the user sees; `token` is the swap's (U12).
    present: function (token, card, generation) {
      paintOwner = null;
      const message = { op: "present", token: isNumber(token) ? token : null };
      // An answer may be queued while the question's shown hooks drain.
      // Keep an explicitly supplied target distinct from an untargeted C66 call,
      // including malformed/partial targets (validated by the runtime).
      if (arguments.length > 1) message.target = { card: card, generation: generation };
      toRuntime(message);
    },
    resume: function (token) {
      paintOwner = null;
      toRuntime({ op: "resume", token: isNumber(token) ? token : null });
    },
    unpresent: function () {
      paintOwner = null;
      toRuntime({ op: "unpresent" });
    },
    // Loads what `html` needs, without inserting it (C7.5).
    warm: function (html) {
      toRuntime({ op: "warm", html: String(html) });
    },
    // Retry load attributes, never the side renderer. CSS rules are parsed
    // afresh for a failed font/background; inputs and scripts stay intact.
    mediaArrived: function (names) {
      const wanted = new Set(names.map(function (name) { return String(name).normalize("NFC"); }));
      const retried = new Set();
      function decoded(path) {
        const input = new TextEncoder().encode(path);
        const bytes = [];
        for (let i = 0; i < input.length; i++) {
          const triplet = String.fromCharCode(input[i + 1], input[i + 2]);
          if (input[i] === 37 && /^[0-9a-f]{2}$/i.test(triplet)) {
            bytes.push(parseInt(triplet, 16));
            i += 2;
          } else bytes.push(input[i]);
        }
        return new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(bytes));
      }
      function matches(url, base) {
        try {
          const parsed = new URL(url, base || document.baseURI);
          if (parsed.protocol !== "meguri-media:" || parsed.host !== "media") return false;
          const name = decoded(parsed.pathname.slice(1)).normalize("NFC");
          let matched = false;
          for (const candidate of wanted) {
            if (candidate.toLowerCase() === name.toLowerCase()) {
              retried.add(candidate);
              matched = true;
            }
          }
          return matched;
        } catch (error) { return false; }
      }
      for (const [element, url] of failed) {
        // An already-started script cannot be retried safely in place.
        if (!element.isConnected || element.localName === "script") continue;
        let attr = element.localName === "link" ? "href" : element.localName === "object" ? "data" : "src";
        if (!element.hasAttribute(attr) && element.hasAttribute("srcset")) attr = "srcset";
        if (!element.hasAttribute(attr) || !matches(url)) continue;
        const value = element.getAttribute(attr);
        element.removeAttribute(attr);
        element.setAttribute(attr, value);
        if (element.localName === "source" && element.parentElement instanceof HTMLMediaElement) element.parentElement.load();
        if (element.localName === "img" && element.parentElement && element.parentElement.localName === "picture") {
          for (const source of element.parentElement.querySelectorAll("source[srcset]")) {
            source.setAttribute("srcset", source.getAttribute("srcset"));
          }
        }
        failed.delete(element);
      }
      for (const sheet of Array.from(document.styleSheets)) {
        const owner = sheet.ownerNode;
        if (!(owner instanceof Element)) continue;
        let touched = false;
        try {
          for (const rule of sheet.cssRules) {
            for (const url of rule.cssText.matchAll(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^)]*))\s*\)/gi)) {
              if (matches(url[1] || url[2] || url[3] || "", sheet.href)) touched = true;
            }
          }
        } catch (error) { /* A cross-origin sheet cannot be inspected. */ }
        if (touched && owner.localName === "style") owner.textContent = owner.textContent;
        if (touched && owner.localName === "link") owner.href = owner.href;
      }
      for (const link of document.querySelectorAll('link[rel="stylesheet"]')) {
        if (matches(link.href)) link.href = link.href;
      }
      // Some style sheets cannot reload in place. Let
      // the caller report names for which no in-place retry was available.
      return Array.from(wanted).filter(function (name) { return !retried.has(name); });
    },
    // Whether a card listener could see an input of `types` ("tap", "key"
    // or one event type) aimed at `element`; for keys without an element,
    // the focused one (C13). A missing response permits the event.
    hasCardListener: function (types, element) {
      const target = element || document.activeElement || document.body || document.documentElement;
      const query = new CustomEvent(QUERY_EVENT, { detail: String(types), bubbles: true, cancelable: true, composed: true });
      return target.dispatchEvent(query);
    },
    // A live appearance switch (C5, R490): only the classes and the theme
    // attribute change; added classes go last, removed ones go in place.
    setAppearance: function (dark, mac) {
      const html = document.documentElement;
      const body = document.body;
      if (dark) {
        html.classList.add("night-mode");
        if (body) {
          body.classList.add("night_mode", "nightMode");
          if (mac) body.classList.add("macos-dark-mode");
        }
      } else {
        html.classList.remove("night-mode");
        if (body) body.classList.remove("night_mode", "nightMode", "macos-dark-mode");
      }
      html.setAttribute("data-bs-theme", dark ? "dark" : "light");
    },
  };
})();
