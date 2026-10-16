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
        side = signal.side;
        break;
      case "visibleReady":
        confirmVisible(signal);
        break;
      case "settled":
        // U12's confirmation contract: the prepare generation as the token.
        if (isNumber(signal.card) && isNumber(signal.generation)) {
          post(CONFIRMATIONS, { token: signal.generation, card: signal.card, phase: "settled" });
        }
        break;
      default:
        break;
    }
    if (!INTERNAL[signal.signal]) post(SIGNALS, signal);
  }, true);

  // Two frames after presentation, the content has painted (R1862).
  function confirmVisible(ready) {
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        const visible = { signal: "visible", token: ready.token, side: ready.side };
        if (isNumber(ready.card)) {
          visible.card = ready.card;
          visible.generation = ready.generation;
        }
        post(SIGNALS, visible);
        if (isNumber(ready.card) && isNumber(ready.token)) {
          post(CONFIRMATIONS, { token: ready.token, card: ready.card, phase: "visible" });
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
    post(SIGNALS, { signal: "mediaError", tag: target.localName, url: String(url), side: side });
  }, true);

  document.fonts.addEventListener("loadingerror", function (event) {
    const faces = event.fontfaces || [];
    for (let i = 0; i < faces.length; i++) {
      post(SIGNALS, { signal: "mediaError", tag: "font", url: "", family: String(faces[i].family), side: side });
    }
  });

  // Whether a selection exists (R406), reported when that changes.
  let selected = false;
  document.addEventListener("selectionchange", function () {
    const selection = document.getSelection();
    const now = !!selection && selection.rangeCount > 0 && !selection.isCollapsed;
    if (now === selected) return;
    selected = now;
    post(SIGNALS, { signal: "selection", selected: now });
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
    post(SIGNALS, { signal: "focus", editable: isEditable, typeans: isTypeans });
  }
  window.addEventListener("focusin", function (event) {
    focusChanged(event.target);
  }, true);
  window.addEventListener("focusout", function (event) {
    focusChanged(event.relatedTarget);
  }, true);

  // IME composition (R535).
  window.addEventListener("compositionstart", function () {
    post(SIGNALS, { signal: "composition", active: true });
  }, true);
  window.addEventListener("compositionend", function () {
    post(SIGNALS, { signal: "composition", active: false });
  }, true);

  // What is typed in #typeans (R538 continuity; the answer reads C10.3).
  window.addEventListener("input", function (event) {
    const target = event.target;
    if (target instanceof Element && target.id === "typeans") {
      post(SIGNALS, { signal: "typedText", text: String(target.value) });
    }
  }, true);

  // ---------------------------------------------------------------------------
  // Native's way into the page.

  window.meguriCard = {
    // The identity of the next update native queues (C6.8).
    identity: function (card, generation) {
      toRuntime({ op: "identity", card: card, generation: generation });
    },
    // The page is now the one the user sees; `token` is the swap's (U12).
    present: function (token) {
      toRuntime({ op: "present", token: isNumber(token) ? token : null });
    },
    unpresent: function () {
      toRuntime({ op: "unpresent" });
    },
    // Loads what `html` needs, without inserting it (C7.5).
    warm: function (html) {
      toRuntime({ op: "warm", html: String(html) });
    },
    // Whether a card listener could see an input of `types` ("tap", "key"
    // or one event type) aimed at `element`; for keys without an element,
    // the focused one (C13). An unanswered query is a yes.
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
