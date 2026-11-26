// Meguri card runtime: the page-world script every card page loads first.
// Written for Meguri from specs/render/card-runtime.md (U66); it holds no
// Anki code (R1960). The C-numbers below are that spec's sections. Read
// AGENTS.md in this directory before changing anything: the top level must
// stay this one function call, and every global it defines is in C9.1.
(function () {
  "use strict";

  // ---------------------------------------------------------------------------
  // Built-ins, taken before any card script can replace them.

  const win = window;
  const doc = document;
  const apply = Reflect.apply;
  const reflectGet = Reflect.get;
  const ProxyCtor = Proxy;
  const toPrimitiveSymbol = Symbol.toPrimitive;
  const describeProperty = Object.getOwnPropertyDescriptor;
  const defineProperty = Object.defineProperty;
  const createObject = Object.create;
  const PromiseCtor = Promise;
  const promiseThen = Promise.prototype.then;
  const promiseResolve = Promise.resolve;
  const stringify = JSON.stringify;
  const parseJSON = JSON.parse;
  const StringCtor = String;
  const isSafeInteger = Number.isSafeInteger;
  const ErrorCtor = Error;
  const URLCtor = URL;
  const CustomEventCtor = CustomEvent;
  const CSSStyleSheetCtor = CSSStyleSheet;
  const WeakMapCtor = WeakMap;
  const WeakRefCtor = WeakRef;
  const SetCtor = Set;
  const ElementCtor = Element;
  const HTMLElementCtor = HTMLElement;
  const InputCtor = HTMLInputElement;
  const TextAreaCtor = HTMLTextAreaElement;
  const inputValueSetter = describeProperty(InputCtor.prototype, "value").set;
  const textAreaValueSetter = describeProperty(TextAreaCtor.prototype, "value").set;
  const SVGElementCtor = SVGElement;
  const BodyCtor = HTMLBodyElement;
  const HtmlCtor = HTMLHtmlElement;
  const ShadowRootCtor = ShadowRoot;
  const mathMax = Math.max;
  const stringSlice = String.prototype.slice;
  const stringLower = String.prototype.toLowerCase;
  const stringSplit = String.prototype.split;
  const stringIndexOf = String.prototype.indexOf;
  const regexpExec = RegExp.prototype.exec;
  const weakMapGet = WeakMap.prototype.get;
  const weakMapSet = WeakMap.prototype.set;
  const weakRefDeref = WeakRef.prototype.deref;
  const setHas = Set.prototype.has;
  const setAdd = Set.prototype.add;

  const setTimeoutFn = win.setTimeout;
  const clearTimeoutFn = win.clearTimeout;
  const requestFrame = win.requestAnimationFrame;
  const scrollToFn = win.scrollTo;
  const performanceNow = Performance.prototype.now;
  const performanceObject = win.performance;
  const eventGetter = getter(win, "event") || getter(Window.prototype, "event");

  const nativeAdd = EventTarget.prototype.addEventListener;
  const nativeRemove = EventTarget.prototype.removeEventListener;
  const nativeDispatch = EventTarget.prototype.dispatchEvent;
  const stopImmediate = Event.prototype.stopImmediatePropagation;
  const preventDefault = Event.prototype.preventDefault;
  const composedPath = Event.prototype.composedPath;
  const eventTarget = getter(Event.prototype, "target");
  const customDetail = getter(CustomEvent.prototype, "detail");
  const abortedGetter = getter(AbortSignal.prototype, "aborted");

  const getElementById = Document.prototype.getElementById;
  const createElement = Document.prototype.createElement;
  const documentQueryAll = Document.prototype.querySelectorAll;
  const documentBody = getter(Document.prototype, "body");
  const documentHead = getter(Document.prototype, "head");
  const documentFonts = getter(Document.prototype, "fonts");
  const fontsStatus = getter(FontFaceSet.prototype, "status");
  const fontsReady = getter(FontFaceSet.prototype, "ready");
  const fragmentQueryAll = DocumentFragment.prototype.querySelectorAll;
  const elementQueryAll = Element.prototype.querySelectorAll;
  const byTagName = Element.prototype.getElementsByTagName;
  const getAttribute = Element.prototype.getAttribute;
  const replayLabel = document.currentScript ? apply(getAttribute, document.currentScript, ["data-replay-label"]) || "Play audio" : "Play audio";
  const setAttribute = Element.prototype.setAttribute;
  const removeAttribute = Element.prototype.removeAttribute;
  const hasAttribute = Element.prototype.hasAttribute;
  const setAttributeNode = Element.prototype.setAttributeNode;
  const attributesOf = getter(Element.prototype, "attributes");
  const localNameOf = getter(Element.prototype, "localName");
  const scrollIntoView = Element.prototype.scrollIntoView;
  const innerHTMLSetter = describeProperty(Element.prototype, "innerHTML").set;
  const cloneNode = Node.prototype.cloneNode;
  const replaceChild = Node.prototype.replaceChild;
  const appendChild = Node.prototype.appendChild;
  const removeChild = Node.prototype.removeChild;
  const parentNodeOf = getter(Node.prototype, "parentNode");
  const firstChildOf = getter(Node.prototype, "firstChild");
  const nextSiblingOf = getter(Node.prototype, "nextSibling");
  const nodeTypeOf = getter(Node.prototype, "nodeType");
  const isConnectedOf = getter(Node.prototype, "isConnected");
  const baseURIOf = getter(Node.prototype, "baseURI");
  const textContentOf = describeProperty(Node.prototype, "textContent");
  const characterData = getter(CharacterData.prototype, "data");
  const shadowHost = getter(ShadowRoot.prototype, "host");
  const templateContent = getter(HTMLTemplateElement.prototype, "content");
  const styleOf = getter(HTMLElement.prototype, "style");
  const offsetHeightOf = getter(HTMLElement.prototype, "offsetHeight");
  const focusHTML = HTMLElement.prototype.focus;
  const focusSVG = SVGElement.prototype.focus;
  const setStyleProperty = CSSStyleDeclaration.prototype.setProperty;
  const scriptSource = getter(HTMLScriptElement.prototype, "src");
  const imageComplete = getter(HTMLImageElement.prototype, "complete");
  const imageDecode = HTMLImageElement.prototype.decode;
  const mediaPause = HTMLMediaElement.prototype.pause;
  const mediaPlay = HTMLMediaElement.prototype.play;
  const mediaLoad = HTMLMediaElement.prototype.load;
  const mediaSourceObject = describeProperty(HTMLMediaElement.prototype, "srcObject");
  const replaceSheet = CSSStyleSheet.prototype.replaceSync;
  const sheetRules = getter(CSSStyleSheet.prototype, "cssRules");
  const fontFaceStyle = getter(CSSFontFaceRule.prototype, "style");
  const getPropertyValue = CSSStyleDeclaration.prototype.getPropertyValue;
  const CSSFontFaceRuleCtor = CSSFontFaceRule;

  const pageProtocol = doc.location.protocol;
  const pageHost = doc.location.host;
  const pageURL = doc.URL;

  function getter(object, name) {
    const descriptor = describeProperty(object, name);
    return descriptor ? descriptor.get : undefined;
  }

  function call(fn, self, ...args) {
    return apply(fn, self, args);
  }

  function noop() {}

  // The platform native wrote on this script element (C2.1).
  const platform = (function () {
    const script = doc.currentScript;
    return script ? call(getAttribute, script, "data-platform") : null;
  })();
  const isMac = platform === "mac";

  // ---------------------------------------------------------------------------
  // Promises and timing, on the captured built-ins.

  function promise(executor) {
    return new PromiseCtor(executor);
  }

  function resolved(value) {
    return apply(promiseResolve, PromiseCtor, [value]);
  }

  function then(value, onFulfilled, onRejected) {
    return apply(promiseThen, value, [onFulfilled, onRejected]);
  }

  // Settles when the first of `promises` settles, with its value.
  function race(...promises) {
    return promise(function (resolve) {
      for (let i = 0; i < promises.length; i++) then(promises[i], resolve, resolve);
    });
  }

  // Settles when every one of `promises` has settled.
  function all(promises) {
    return promise(function (resolve) {
      let left = promises.length;
      if (left === 0) resolve();
      const done = function () {
        left -= 1;
        if (left === 0) resolve();
      };
      for (let i = 0; i < promises.length; i++) then(promises[i], done, done);
    });
  }

  function delay(milliseconds) {
    return promise(function (resolve) {
      call(setTimeoutFn, win, resolve, milliseconds);
    });
  }

  function now() {
    return call(performanceNow, performanceObject);
  }

  // Two animation frames, one asked for inside the other's callback.
  function twoFrames() {
    return promise(function (resolve) {
      call(requestFrame, win, function () {
        call(requestFrame, win, function () {
          resolve();
        });
      });
    });
  }

  // Settles on the element's next `load` or `error`.
  function loadOrError(element) {
    return promise(function (resolve) {
      call(nativeAdd, element, "load", resolve, { once: true });
      call(nativeAdd, element, "error", resolve, { once: true });
    });
  }

  // ---------------------------------------------------------------------------
  // Signals to the interaction script, and native's messages (C14). Both are
  // DOM events with a JSON string, caught on window in the capture phase.

  const SIGNAL_EVENT = "meguri:signal";
  const NATIVE_EVENT = "meguri:native";
  const QUERY_EVENT = "meguri:query";

  function emit(signal) {
    let text;
    try {
      text = apply(stringify, null, [signal]);
    } catch (error) {
      return;
    }
    call(nativeDispatch, doc, new CustomEventCtor(SIGNAL_EVENT, { detail: text }));
  }

  // A signal about `update`, with its side and identity (C6.8).
  function updateSignal(name, update, fields) {
    const signal = { signal: name, side: update.side };
    if (update.identity) {
      signal.card = update.identity.card;
      signal.generation = update.identity.generation;
    }
    if (fields) {
      for (const key in fields) signal[key] = fields[key];
    }
    emit(signal);
  }

  // ---------------------------------------------------------------------------
  // Page state.

  // The original hook arrays (C6.4): only these ever run.
  const updateHooks = [];
  const shownHooks = [];

  let presented = false;
  // Standalone C66 pages retain their normal automatic focus. U67 supplies
  // native ownership before entry/presentation when a panel can own keys.
  let automaticTypeAnswerFocus = true;
  // The identity native announced for the next queued update (C6.8).
  let announced = null;
  // The side and identity of the update that started most recently.
  let side = null;
  let latest = null;
  // The element the last question's shown callback remembered (C10.3).
  let typedElement = null;
  // Updates parked after step 8 in an unpresented page (C6.6).
  let parked = [];
  // Media paused while unpresented, to play at presentation (C8.2).
  let held = [];
  // The presentation still owed a visible confirmation (C14).
  let pendingPresentation = null;
  // Font URLs this page has asked for, for its whole life (C7.1).
  const askedFonts = new SetCtor();

  // ---------------------------------------------------------------------------
  // Diagnostics (C12).

  // Current script index for error reports.
  let runningScript = 0;
  let awaitedScript = null;
  let hookPhase = null;

  function describe(value) {
    let text;
    try {
      if (value instanceof ErrorCtor) {
        text = StringCtor(value.name) + ": " + StringCtor(value.message);
      } else {
        text = StringCtor(value);
      }
    } catch (error) {
      text = "<unprintable>";
    }
    return text.length > 2000 ? call(stringSlice, text, 0, 2000) : text;
  }

  function report(fields) {
    const signal = { signal: "error", side: side };
    if (latest && latest.identity) {
      signal.card = latest.identity.card;
      signal.generation = latest.identity.generation;
    }
    for (const key in fields) signal[key] = fields[key];
    emit(signal);
  }

  function reportHook(value, phase) {
    report({ kind: "hook", message: describe(value), phase: phase });
  }

  call(nativeAdd, win, "error", function (event) {
    if (call(eventTarget, event) !== win) return;
    let phase = "later";
    let index;
    if (runningScript) {
      phase = "script";
      index = runningScript;
    } else if (awaitedScript && event.filename === awaitedScript.url) {
      phase = "script";
      index = awaitedScript.index;
    } else if (hookPhase) {
      phase = hookPhase;
    }
    const value = event.error;
    const filename = event.filename;
    if (filename && call(stringIndexOf, filename, mathRoot) === 0) {
      mathError(value === undefined || value === null ? event.message : value,
        mathOwner || mathStartupOwner, filename);
      return;
    }
    report({
      kind: "exception",
      message: describe(value === undefined || value === null ? event.message : value),
      line: event.lineno,
      column: event.colno,
      source: !filename || filename === pageURL || filename === doc.URL ? "inline" : filename,
      phase: phase,
      index: index,
    });
  });

  call(nativeAdd, win, "unhandledrejection", function (event) {
    report({ kind: "rejection", message: describe(event.reason), phase: "later" });
  });

  // ---------------------------------------------------------------------------
  // Listener registry (C13).

  const TAP_TYPES = ["touchstart", "touchend", "pointerdown", "pointerup", "mousedown", "mouseup", "click"];
  const KEY_TYPES = ["keydown", "keypress", "keyup"];
  const tracked = createObject(null);
  for (let i = 0; i < TAP_TYPES.length; i++) tracked[TAP_TYPES[i]] = true;
  for (let i = 0; i < KEY_TYPES.length; i++) tracked[KEY_TYPES[i]] = true;

  // target -> type -> null-prototype indexed entries with an explicit length.
  // Each entry holds { callback: WeakRef, capture, once }. Native owns
  // active callbacks; stale once entries must not own old card content.
  const registry = new WeakMapCtor();

  function isObject(value) {
    return value !== null && (typeof value === "object" || typeof value === "function");
  }

  // Observe only the conversions native actually performs, after its receiver
  // and arity checks. Empty proxy targets avoid invariants on frozen inputs;
  // getters and conversion methods still receive the caller's original object.
  function listenerArguments(original) {
    const observed = { args: createObject(null), type: undefined, capture: false, once: false, signal: undefined };
    const args = observed.args;
    args.length = original.length;
    for (let i = 0; i < original.length; i++) {
      const supplied = describeProperty(original, i);
      if (supplied) args[i] = supplied.value;
    }
    const type = args.length > 0 ? args[0] : undefined;
    observed.type = type;
    if (isObject(type)) {
      args[0] = new ProxyCtor(createObject(null), {
        get: function (_, key) {
          const method = reflectGet(type, key, type);
          if ((key !== toPrimitiveSymbol && key !== "toString" && key !== "valueOf") || typeof method !== "function") return method;
          return function () {
            const value = apply(method, type, arguments);
            // Native must itself reject Symbols and failed ToPrimitive results.
            if (!isObject(value) && typeof value !== "symbol") observed.type = StringCtor(value);
            return value;
          };
        },
      });
    }
    const options = args.length > 2 ? args[2] : undefined;
    if (isObject(options)) {
      args[2] = new ProxyCtor(createObject(null), {
        get: function (_, key) {
          const value = reflectGet(options, key, options);
          if (key === "capture") observed.capture = !!value;
          else if (key === "once") observed.once = !!value;
          else if (key === "signal") observed.signal = value;
          return value;
        },
      });
    } else {
      // The boolean branch of Web IDL's (dictionary or boolean) union also
      // accepts non-object primitives. Null/undefined have the default false.
      observed.capture = !!options;
    }
    return observed;
  }

  function removeEntryAt(entries, index) {
    for (let j = index + 1; j < entries.length; j++) entries[j - 1] = entries[j];
    delete entries[entries.length - 1];
    entries.length -= 1;
  }

  function pruneEntries(entries) {
    for (let i = entries.length - 1; i >= 0; i--) {
      if (call(weakRefDeref, entries[i].callback) === undefined) removeEntryAt(entries, i);
    }
  }

  function entriesOf(target, type, create) {
    let byType = call(weakMapGet, registry, target);
    if (!byType) {
      if (!create) return null;
      byType = createObject(null);
      call(weakMapSet, registry, target, byType);
    }
    let entries = byType[type];
    if (entries) pruneEntries(entries);
    if (!entries && create) {
      entries = createObject(null);
      entries.length = 0;
      byType[type] = entries;
    }
    return entries || null;
  }

  // removeEventListener: WebKit holds at most one registration per type,
  // callback and capture, and it is gone now.
  function untrack(target, type, callback, capture) {
    const entries = entriesOf(target, type, false);
    if (!entries) return;
    for (let i = entries.length - 1; i >= 0; i--) {
      if (call(weakRefDeref, entries[i].callback) === callback && entries[i].capture === capture) removeEntryAt(entries, i);
    }
  }

  // A signal's abort: only the entry its own add made, if still there.
  function untrackEntry(target, type, entry) {
    const entries = entriesOf(target, type, false);
    if (!entries) return;
    for (let i = 0; i < entries.length; i++) {
      if (entries[i] === entry) {
        removeEntryAt(entries, i);
        return;
      }
    }
  }

  // Keep this closure outside track's scope: a live controller must retain
  // neither the target nor the original callback, even indirectly.
  function abortCleanup(targetRef, type, entryRef) {
    return function () {
      const target = call(weakRefDeref, targetRef);
      const entry = call(weakRefDeref, entryRef);
      if (target && entry) untrackEntry(target, type, entry);
    };
  }

  function track(target, type, callback, observed) {
    if (!tracked[type] || callback === null || callback === undefined) return;
    const capture = observed.capture;
    const signal = observed.signal;
    if (signal && call(abortedGetter, signal)) return;
    const entries = entriesOf(target, type, true);
    // A live entry means WebKit ignored this add, signal included. A `once`
    // entry may have fired already, so the add may be a new registration:
    // it gets its own entry (counting too much is allowed, C13).
    for (let i = 0; i < entries.length; i++) {
      const entry = entries[i];
      if (call(weakRefDeref, entry.callback) === callback && entry.capture === capture && !entry.once) return;
    }
    const entry = { callback: new WeakRefCtor(callback), capture: capture, once: observed.once };
    entries[entries.length] = entry;
    entries.length += 1;
    if (signal) {
      call(nativeAdd, signal, "abort", abortCleanup(new WeakRefCtor(target), type, new WeakRefCtor(entry)), { once: true });
    }
  }

  // A call with no `this` reaches the window, as the platform defines.
  function targetOf(self) {
    return self === undefined || self === null ? win : self;
  }

  EventTarget.prototype.addEventListener = function addEventListener(type, callback) {
    const observed = listenerArguments(arguments);
    const result = apply(nativeAdd, this, observed.args);
    try {
      track(targetOf(this), StringCtor(observed.type), callback, observed);
    } catch (error) {
      // Never let bookkeeping fail a card's call.
    }
    return result;
  };

  EventTarget.prototype.removeEventListener = function removeEventListener(type, callback) {
    const observed = listenerArguments(arguments);
    const result = apply(nativeRemove, this, observed.args);
    try {
      untrack(targetOf(this), StringCtor(observed.type), callback, observed.capture);
    } catch (error) {
      // As above.
    }
    return result;
  };

  // Check registered handlers; return true if the registry cannot decide.
  function listensOn(node, types) {
    for (let i = 0; i < types.length; i++) {
      const type = types[i];
      const entries = entriesOf(node, type, false);
      if (entries && entries.length > 0) return true;
      try {
        const handler = node["on" + type];
        if (handler !== null && handler !== undefined) return true;
        if (node instanceof ElementCtor && call(hasAttribute, node, "on" + type)) return true;
      } catch (error) {
        return true;
      }
    }
    return false;
  }

  function parentOf(node) {
    const parent = call(parentNodeOf, node);
    if (parent) return parent;
    return node instanceof ShadowRootCtor ? call(shadowHost, node) : null;
  }

  function hasCardListener(types, target) {
    if (listensOn(win, types) || listensOn(doc, types)) return true;
    for (let node = target; node && node !== win; node = parentOf(node)) {
      if (listensOn(node, types)) return true;
    }
    return false;
  }

  // Review taps (U180): a tap listener on the target or an ancestor below
  // <body>. Listeners on window, document, <html> and <body> listen to the
  // whole page and do not make one element a card control.
  function hasElementTapListener(target) {
    for (let node = target; node && node !== win && node !== doc; node = parentOf(node)) {
      if (node instanceof BodyCtor || node instanceof HtmlCtor) return false;
      if (listensOn(node, TAP_TYPES)) return true;
    }
    return false;
  }

  // The interaction world asks by dispatching a cancelable query on the
  // element: the runtime cancels it when no card listener could see the
  // input. An incomplete response permits the event.
  call(nativeAdd, win, QUERY_EVENT, function (event) {
    call(stopImmediate, event);
    try {
      const asked = StringCtor(call(customDetail, event));
      const path = call(composedPath, event);
      const target = path.length > 0 ? path[0] : call(eventTarget, event);
      if (asked === "tapTarget") {
        if (!hasElementTapListener(target)) call(preventDefault, event);
        return;
      }
      const types = asked === "tap" ? TAP_TYPES : asked === "key" ? KEY_TYPES : [asked];
      if (!hasCardListener(types, target)) call(preventDefault, event);
    } catch (error) {
      // Preserve the event if the check fails.
    }
  }, true);

  // ---------------------------------------------------------------------------
  // IME state for _typeAnsPress (C10.4), tracked ahead of every card listener.

  let composing = false;
  let imeKeystroke = false;
  let justCommitted = false;

  function imeEvent(event) {
    return event.isComposing === true || event.keyCode === 229;
  }

  call(nativeAdd, win, "compositionstart", function () {
    composing = true;
  }, true);
  call(nativeAdd, win, "compositionend", function () {
    composing = false;
    justCommitted = true;
  }, true);
  call(nativeAdd, win, "keydown", function (event) {
    imeKeystroke = imeEvent(event);
    if (!imeKeystroke) justCommitted = false;
  }, true);
  call(nativeAdd, win, "keyup", function () {
    imeKeystroke = false;
    justCommitted = false;
  }, true);

  function enterBelongsToIME(event) {
    return composing || imeEvent(event) || imeKeystroke || justCommitted;
  }

  // ---------------------------------------------------------------------------
  // Media in an unpresented page (C8.2).

  function isMedia(target) {
    try {
      const name = call(localNameOf, target);
      return name === "audio" || name === "video";
    } catch (error) {
      return false;
    }
  }

  call(nativeAdd, win, "play", function (event) {
    const target = call(eventTarget, event);
    if (presented || !isMedia(target)) return;
    try {
      call(mediaPause, target);
    } catch (error) {
      return;
    }
    for (let i = 0; i < held.length; i++) if (held[i] === target) return;
    held[held.length] = target;
  }, true);

  function present(token, target) {
    if (target !== undefined && (target === null || typeof target !== "object"
      || !isSafeInteger(target.card) || !isSafeInteger(target.generation) || target.generation < 0)) return;
    presented = true;
    const resume = held;
    held = [];
    for (let i = 0; i < resume.length; i++) {
      const media = resume[i];
      if (!call(isConnectedOf, media)) continue;
      try {
        const playing = call(mediaPlay, media);
        if (playing) then(playing, noop, noop);
      } catch (error) {
        // A refused play() is ignored.
      }
    }
    pendingPresentation = token === undefined || token === null ? null : { token: token, identity: target || null };
    confirmVisibleWhenShown();
    wakeParked("present");
  }

  function unpresent() {
    presented = false;
    pendingPresentation = null;
    const media = call(documentQueryAll, doc, "audio, video");
    for (let i = 0; i < media.length; i++) {
      try {
        call(mediaPause, media[i]);
      } catch (error) {
        // Nothing to pause.
      }
    }
  }

  // The interaction script confirms with two frames of its own once the
  // page is presented and its latest update has passed step 8 (R1862).
  function confirmVisibleWhenShown() {
    if (!presented || pendingPresentation === null || !latest || !latest.shown) return;
    const target = pendingPresentation.identity;
    if (target && (!latest.identity || latest.identity.card !== target.card || latest.identity.generation !== target.generation)) return;
    updateSignal("visibleReady", latest, { token: pendingPresentation.token });
    pendingPresentation = null;
  }

  // ---------------------------------------------------------------------------
  // Preloading (C7).

  function resolveURL(url) {
    try {
      return new URLCtor(url, call(baseURIOf, doc)).href;
    } catch (error) {
      return null;
    }
  }

  function startImage(url) {
    const image = call(createElement, doc, "img");
    image.decoding = "async";
    const done = loadOrError(image);
    image.src = url;
    try {
      then(call(imageDecode, image), noop, noop);
    } catch (error) {
      // The decode is only a request.
    }
    return done;
  }

  // A print-media copy in <head>, removed once it loaded or failed.
  function startHeadCopy(element) {
    const head = call(documentHead, doc);
    if (!head) return resolved();
    call(setAttribute, element, "data-meguri-preload", "");
    const done = loadOrError(element);
    call(appendChild, head, element);
    return then(done, function () {
      const parent = call(parentNodeOf, element);
      if (parent) call(removeChild, parent, element);
    });
  }

  function hasToken(list, token) {
    const tokens = call(stringSplit, call(stringLower, StringCtor(list)), /\s+/);
    for (let i = 0; i < tokens.length; i++) if (tokens[i] === token) return true;
    return false;
  }

  const URL_PATTERN = /url\(\s*(?:"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'|([^)\s]*))\s*\)/g;

  // The url() sources of the top-level @font-face rules in `css`.
  function fontSources(css) {
    const sources = [];
    let rules;
    try {
      const sheet = new CSSStyleSheetCtor();
      call(replaceSheet, sheet, css);
      rules = call(sheetRules, sheet);
    } catch (error) {
      return sources;
    }
    for (let i = 0; i < rules.length; i++) {
      const rule = rules[i];
      if (!(rule instanceof CSSFontFaceRuleCtor)) continue;
      const text = call(getPropertyValue, call(fontFaceStyle, rule), "src");
      URL_PATTERN.lastIndex = 0;
      let match;
      while ((match = call(regexpExec, URL_PATTERN, text)) !== null) {
        const url = match[1] !== undefined ? match[1] : match[2] !== undefined ? match[2] : match[3];
        if (url) sources[sources.length] = url;
      }
    }
    return sources;
  }

  // Starts loading what `html` will need; returns the loads and the budget
  // to wait for them (C7.1, C7.2). With `imagesOnly`, only its images (C7.3).
  function startPreload(html, imagesOnly) {
    const loads = [];
    let budget = 0;
    const template = call(createElement, doc, "template");
    call(innerHTMLSetter, template, StringCtor(html));
    const content = call(templateContent, template);

    const images = call(fragmentQueryAll, content, "img[src]");
    for (let i = 0; i < images.length; i++) {
      const url = resolveURL(call(getAttribute, images[i], "src"));
      if (url === null) continue;
      loads[loads.length] = startImage(url);
      budget = mathMax(budget, 200);
    }
    if (imagesOnly) return { loads: loads, budget: budget };

    const links = call(fragmentQueryAll, content, "link[rel][href]");
    for (let i = 0; i < links.length; i++) {
      if (!hasToken(call(getAttribute, links[i], "rel"), "stylesheet")) continue;
      const url = resolveURL(call(getAttribute, links[i], "href"));
      if (url === null) continue;
      const copy = call(createElement, doc, "link");
      call(setAttribute, copy, "rel", "stylesheet");
      call(setAttribute, copy, "media", "print");
      call(setAttribute, copy, "href", url);
      loads[loads.length] = startHeadCopy(copy);
      budget = mathMax(budget, 500);
    }

    const styles = call(fragmentQueryAll, content, "style");
    for (let i = 0; i < styles.length; i++) {
      const css = StringCtor(call(textContentOf.get, styles[i]));
      if (call(stringIndexOf, call(stringLower, css), "@import") >= 0) {
        const copy = call(createElement, doc, "style");
        call(setAttribute, copy, "media", "print");
        call(textContentOf.set, copy, css);
        loads[loads.length] = startHeadCopy(copy);
        budget = mathMax(budget, 500);
      }
      const sources = fontSources(css);
      for (let j = 0; j < sources.length; j++) {
        const url = resolveURL(sources[j]);
        if (url === null || call(setHas, askedFonts, url)) continue;
        call(setAdd, askedFonts, url);
        const link = call(createElement, doc, "link");
        call(setAttribute, link, "rel", "preload");
        call(setAttribute, link, "as", "font");
        call(setAttribute, link, "crossorigin", "");
        call(setAttribute, link, "href", url);
        loads[loads.length] = startHeadCopy(link);
        budget = 800;
      }
    }
    return { loads: loads, budget: budget };
  }

  async function preloadAndWait(html) {
    const started = startPreload(html);
    if (started.loads.length === 0) return;
    await race(all(started.loads), delay(started.budget));
  }

  // Calls `callback` once every <img> now in the document has finished
  // loading (C7.3).
  function afterImages(callback) {
    const images = call(documentQueryAll, doc, "img");
    const waits = [];
    for (let i = 0; i < images.length; i++) {
      if (!call(imageComplete, images[i])) waits[waits.length] = loadOrError(images[i]);
    }
    then(all(waits), function () {
      try {
        callback();
      } catch (error) {
        // Nothing the card sees.
      }
    });
  }

  // ---------------------------------------------------------------------------
  // The script walk (C6.5).

  const JAVASCRIPT_TYPES = createObject(null);
  const javascriptTypeList = [
    "application/ecmascript", "application/javascript", "application/x-ecmascript", "application/x-javascript",
    "text/ecmascript", "text/javascript", "text/javascript1.0", "text/javascript1.1", "text/javascript1.2",
    "text/javascript1.3", "text/javascript1.4", "text/javascript1.5", "text/jscript", "text/livescript",
    "text/x-ecmascript", "text/x-javascript",
  ];
  for (let i = 0; i < javascriptTypeList.length; i++) JAVASCRIPT_TYPES[javascriptTypeList[i]] = true;

  // HTML's ASCII whitespace at the ends of a value (tab, line feed, form
  // feed, carriage return, space), never the other characters
  // String.prototype.trim removes.
  const ASCII_WHITESPACE_ENDS = /^[\t\n\f\r ]*([^]*?)[\t\n\f\r ]*$/;

  // "classic", "module" or null (a data block), from the type and language
  // attributes as WebKit reads them (ScriptSemanticsTests checks each form
  // against the engine). A JavaScript type may have ASCII whitespace around
  // it; "module" may not, and a language value is used as it is.
  // Lowercasing is enough for the case-insensitive match: the only
  // non-ASCII letter it turns into an ASCII one is the Kelvin sign, and no
  // name it is compared with has a k.
  function scriptKind(script) {
    const type = call(getAttribute, script, "type");
    const language = call(getAttribute, script, "language");
    if (type !== null) {
      if (type === "") return "classic";
      if (JAVASCRIPT_TYPES[call(stringLower, call(regexpExec, ASCII_WHITESPACE_ENDS, type)[1])]) return "classic";
      return call(stringLower, type) === "module" ? "module" : null;
    }
    if (language === null || language === "") return "classic";
    return JAVASCRIPT_TYPES[call(stringLower, "text/" + language)] ? "classic" : null;
  }

  // Whether the engine will fetch the script's src, and so fire load or error.
  function fetchesSource(script) {
    if (!call(hasAttribute, script, "src")) return false;
    const kind = scriptKind(script);
    if (kind === "module") return true;
    return kind === "classic" && !call(hasAttribute, script, "nomodule");
  }

  function isRemote(url) {
    try {
      const parsed = new URLCtor(url);
      return parsed.protocol !== pageProtocol || parsed.host !== pageHost;
    } catch (error) {
      return false;
    }
  }

  // The data of the script's text children, as the DOM holds them.
  function scriptText(script) {
    let text = "";
    for (let node = call(firstChildOf, script); node; node = call(nextSiblingOf, node)) {
      const type = call(nodeTypeOf, node);
      if (type === 3 || type === 4) text += call(characterData, node);
    }
    return text;
  }

  // A new script element with the old one's attributes, in order, and text.
  function freshCopy(script) {
    const fresh = call(createElement, doc, "script");
    const attributes = call(attributesOf, script);
    for (let i = 0; i < attributes.length; i++) {
      call(setAttributeNode, fresh, call(cloneNode, attributes[i]));
    }
    call(textContentOf.set, fresh, scriptText(script));
    return fresh;
  }

  async function walkScripts(qa) {
    const scripts = call(byTagName, qa, "script");
    let remoteLeft = 800;
    for (let index = 0; ; index++) {
      const old = scripts[index];
      if (!old) break;
      const fresh = freshCopy(old);
      const waits = fetchesSource(old);
      const loaded = waits ? loadOrError(fresh) : null;
      runningScript = index + 1;
      try {
        call(replaceChild, call(parentNodeOf, old), fresh, old);
      } finally {
        runningScript = 0;
      }
      if (!loaded) {
        // The promise jobs the script queued run before the next script;
        // no task runs in between.
        await resolved();
        continue;
      }
      const url = call(scriptSource, fresh);
      awaitedScript = { index: index + 1, url: url };
      try {
        if (isRemote(url)) {
          if (remoteLeft > 0) {
            const started = now();
            await race(loaded, delay(remoteLeft));
            remoteLeft -= now() - started;
          }
        } else {
          await loaded;
        }
      } finally {
        awaitedScript = null;
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Hooks (C6.4).

  // Waits for a value a hook returned; rejections are recorded.
  function settleHookValue(value, phase) {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) return resolved();
    let thenMethod;
    try {
      thenMethod = value.then;
    } catch (error) {
      reportHook(error, phase);
      return resolved();
    }
    if (typeof thenMethod !== "function") return resolved();
    return promise(function (resolve) {
      try {
        apply(thenMethod, value, [
          function () {
            resolve();
          },
          function (reason) {
            reportHook(reason, phase);
            resolve();
          },
        ]);
      } catch (error) {
        reportHook(error, phase);
        resolve();
      }
    });
  }

  async function runHooks(hooks, phase) {
    const waits = [];
    for (let i = 0; i < hooks.length; i++) {
      const hook = hooks[i];
      let value;
      hookPhase = phase;
      try {
        value = apply(hook, undefined, []);
      } catch (error) {
        reportHook(error, phase);
        continue;
      } finally {
        hookPhase = null;
      }
      waits[waits.length] = settleHookValue(value, phase);
    }
    await all(waits);
  }

  // ---------------------------------------------------------------------------
  // Updates (C6).

  const queue = [];
  let queueHead = 0;
  let drainingUpdates = false;

  function queued() {
    return queueHead < queue.length;
  }

  function wakeParked(reason) {
    const pendingResolutions = parked;
    parked = [];
    for (let i = 0; i < pendingResolutions.length; i++) pendingResolutions[i](reason);
  }

  function enqueue(update) {
    update.identity = announced;
    announced = null;
    queue[queue.length] = update;
    // A queued update supersedes one parked unpresented (C6.6).
    wakeParked("superseded");
    if (!drainingUpdates) drainUpdates();
  }

  async function drainUpdates() {
    drainingUpdates = true;
    while (queued()) {
      const update = queue[queueHead];
      queue[queueHead] = undefined;
      queueHead += 1;
      await runUpdate(update);
    }
    queue.length = 0;
    queueHead = 0;
    drainingUpdates = false;
  }

  function findQA() {
    const qa = call(getElementById, doc, "qa");
    if (!qa) throw new ErrorCtor("the page has no #qa element");
    return qa;
  }

  function setOpacity(qa, value) {
    call(setStyleProperty, call(styleOf, qa), "opacity", value);
  }

  // Step 4: videos let go of their media before they are removed (R471).
  // The reload finds nothing to select only once every source is gone: the
  // child elements, the element's own src and an assigned media object.
  function releaseVideos(qa) {
    const videos = call(elementQueryAll, qa, "video");
    for (let i = 0; i < videos.length; i++) {
      const video = videos[i];
      try {
        call(mediaPause, video);
        for (let child = call(firstChildOf, video); child; child = call(firstChildOf, video)) {
          call(removeChild, video, child);
        }
        call(removeAttribute, video, "src");
        if (call(mediaSourceObject.get, video) !== null) call(mediaSourceObject.set, video, null);
        call(mediaLoad, video);
      } catch (error) {
        // A video that cannot be released is removed anyway.
      }
    }
  }

  // Fonts idle and the non-lazy images in #qa loaded and decoded (C6.6).
  function contentReady(qa) {
    try {
      // Lay out first, so that the fonts the side uses start loading.
      call(offsetHeightOf, qa);
    } catch (error) {
      // Not an HTML element: nothing to lay out.
    }
    const waits = [];
    const fonts = call(documentFonts, doc);
    waits[0] = promise(function (resolve) {
      const check = function () {
        if (call(fontsStatus, fonts) !== "loading") resolve();
        else then(call(fontsReady, fonts), check, check);
      };
      check();
    });
    const images = call(byTagName, qa, "img");
    for (let i = 0; i < images.length; i++) {
      const image = images[i];
      const loading = call(getAttribute, image, "loading");
      if (loading !== null && call(stringLower, loading) === "lazy") continue;
      try {
        waits[waits.length] = then(call(imageDecode, image), noop, noop);
      } catch (error) {
        // Not decodable: nothing to wait for.
      }
    }
    return all(waits);
  }

  // Parks an update that passed step 8 in an unpresented page: it settles
  // beneath (unless woken first), then waits for presentation or a newer
  // update. Resolves "present" or "superseded".
  async function parkUnpresented(update, qa) {
    const step8 = now();
    let wake;
    const woken = promise(function (resolve) {
      wake = resolve;
    });
    parked[parked.length] = wake;
    const ready = then(race(contentReady(qa), delay(mathMax(0, 800 - (now() - step8)))), twoFrames);
    const first = await race(then(ready, function () {
      return "settled";
    }), woken);
    if (first !== "settled") return first;
    updateSignal("settled", update);
    return await woken;
  }

  // Offline MathJax. The facade preserves the template API before first use;
  // ordinary cards fetch no math assets. Only upstream bundled components may
  // be loaded, including autoloaded TeX packages and dynamic SVG glyphs.
  const mathRoot = "meguri-media://media/_anki/mathjax/";
  let mathLoading = null;
  let mathEngine = null;
  let mathTypeset = null;
  let mathClear = null;
  let mathOwner = null;
  let mathStartupOwner = null;

  function mathError(error, update, source) {
    // Late work belongs to its requesting side, never the page's next user.
    if (!update || update !== latest) return;
    updateSignal("error", update, {
      kind: "math", message: describe(error), phase: "typeset", source: source || mathRoot + "tex-svg.js",
    });
  }

  function mathStartup() {
    const update = latest;
    const waiting = mathDeadline(loadMath(), 8000);
    then(waiting, noop, function (error) {
      if (update) update.mathFailed = true;
      mathError(error, update);
    });
    return waiting;
  }

  function mathDeadline(work, milliseconds) {
    return promise(function (resolve, reject) {
      const timer = call(setTimeoutFn, win, function () {
        reject(new ErrorCtor("Bundled MathJax timed out"));
      }, milliseconds);
      then(work, function (value) {
        call(clearTimeoutFn, win, timer);
        resolve(value);
      }, function (error) {
        call(clearTimeoutFn, win, timer);
        reject(error);
      });
    });
  }

  function loadMathScript(path) {
    return promise(function (resolve, reject) {
      let url;
      try {
        url = new URLCtor(path, mathRoot);
        // The Swift manifest admits the exact release files. Here constrain
        // requests to that immutable directory, including on the harness host.
        // URL normalizes dot segments; encoded separators/dots are refused too.
        const root = new URLCtor(mathRoot);
        if (url.protocol !== root.protocol || url.host !== root.host
            || url.search || url.hash || url.username || url.password
            || call(stringIndexOf, url.pathname, root.pathname) !== 0
            || call(stringIndexOf, url.pathname, "%") !== -1
            || call(stringIndexOf, url.pathname, "\\") !== -1) {
          throw new ErrorCtor("Unbundled MathJax component refused");
        }
      } catch (error) { reject(error); return; }
      const owner = mathOwner || mathStartupOwner;
      const script = call(createElement, doc, "script");
      let finished = false;
      const finish = function (error) {
        if (finished) return;
        finished = true;
        const parent = call(parentNodeOf, script);
        if (parent) call(removeChild, parent, script);
        if (error) {
          mathError(error, owner, url.href);
          reject(error);
        } else resolve();
      };
      // Removing a classic script cannot cancel execution. Keep its constrained
      // configuration and real completion alive; only callers have deadlines.
      call(nativeAdd, script, "load", function () { finish(); });
      call(nativeAdd, script, "error", function () { finish(new ErrorCtor("Bundled MathJax component failed")); });
      call(setAttribute, script, "src", url.href);
      call(appendChild, call(documentHead, doc), script);
    });
  }

  function limitMathAlignments(engine) {
    // Version-pinned application integration; the combined tex-svg bundle
    // already registered AMS. Replacing AmsMethods alone misses its stored
    // function references. Guard the registered environment parser instead.
    if (engine.version !== "4.1.3") throw new ErrorCtor("Unsupported bundled MathJax version");
    const tex = engine._.input.tex;
    const environments = tex.MapHandler.MapHandler.getMap("AMSmath-environment");
    const parseEnvironment = environments.parser;
    const alignAt = tex.ams.AmsMethods.AmsMethods.AlignAt;
    const xalignAt = tex.ams.AmsMethods.AmsMethods.XalignAt;
    const TeXError = tex.TexError.default;
    environments.parser = function (parser, name, method, args) {
      if (method === alignAt || method === xalignAt) {
        // Read the same argument AMS will use, including alignedat's optional
        // placement, then rewind. Macro expansion has already reached this
        // parser. Leave syntax validation and ordinary inputs to upstream.
        const position = parser.i;
        try {
          const command = "\\begin{" + name + "}";
          if (name === "alignedat") parser.GetBrackets(command);
          const pairs = parser.GetArgument(command);
          if (call(regexpExec, /^[0-9]+$/, pairs) && +pairs > 64) {
            throw new TeXError("MeguriAlignmentLimit", "At most 64 alignment pairs are allowed");
          }
        } finally {
          parser.i = position;
        }
      }
      return call(parseEnvironment, this, parser, name, method, args);
    };
  }

  function loadMath() {
    if (mathLoading) return mathLoading;
    mathStartupOwner = latest;
    // tex-svg contains the base font and menu constructor. Disable saved
    // menu preferences for this document only, before its constructor runs.
    // In particular enableMenu:false alone does not stop mergeUserSettings.
    const configuration = {
      loader: {
        paths: {
          mathjax: call(stringSlice, mathRoot, 0, -1),
          fonts: "[mathjax]/fonts",
          "mathjax-newcm": "[mathjax]/font",
        },
        load: ["[tex]/ams", "[tex]/mhchem", "a11y/assistive-mml"],
        // Never pass Meguri's window.require to the upstream component loader.
        // The release's other require references are guarded Node-only paths.
        require: loadMathScript,
      },
      startup: {
        typeset: false,
        ready: function () {
          const engine = win.MathJax;
          limitMathAlignments(engine);
          const Menu = engine._.ui.menu.Menu.Menu;
          engine.config.options.MenuClass = class extends Menu {
            mergeUserSettings() {}
          };
          engine.startup.defaultReady();
          mathEngine = engine;
          mathTypeset = engine.typesetPromise;
          mathClear = engine.typesetClear;
          // Keep template calls bounded and side-scoped after startup too.
          engine.typeset = mathFacade.typeset;
          engine.typesetPromise = mathFacade.typesetPromise;
          engine.typesetClear = mathFacade.typesetClear;
        },
      },
      // Templates can make several requests while the first script is in flight.
      typeset: mathFacade.typeset,
      typesetPromise: mathFacade.typesetPromise,
      typesetClear: mathFacade.typesetClear,
      tex: mathFacade.tex,
      options: {
        compileError: function (document, item, error) {
          mathError(error, mathOwner);
          document.compileError(item, error);
        },
        typesetError: function (document, item, error) {
          mathError(error, mathOwner);
          document.typesetError(item, error);
        },
        enableMenu: false,
        menuOptions: { settings: {
          enrich: false, speech: false, braille: false,
          assistiveMml: true, inTabOrder: false,
        } },
        enableExplorer: false, enableEnrichment: false,
        enableSpeech: false, enableBraille: false, enableAssistiveMml: true,
      },
    };
    // This getter is only for templates before the script executes. Upstream
    // creates a separate startup object with its own promise. Capture that
    // original before exposing bounded template waits below (a script load
    // event also fires after a JS evaluation error).
    defineProperty(configuration.startup, "promise", { get: mathStartup });
    win.MathJax = configuration;
    mathLoading = promise(function (resolve, reject) {
      configuration.loader.failed = reject;
      then(then(loadMathScript(mathRoot + "tex-svg.js"), function () {
        const engine = win.MathJax;
        if (engine.version !== "4.1.3" || !engine.startup || !engine.startup.promise) {
          throw new ErrorCtor("Bundled MathJax did not initialize");
        }
        // Until startup installs the real methods, preserve template calls.
        if (!engine.typeset) engine.typeset = mathFacade.typeset;
        if (!engine.typesetPromise) engine.typesetPromise = mathFacade.typesetPromise;
        if (!engine.typesetClear) engine.typesetClear = mathFacade.typesetClear;
        const startup = engine.startup.promise;
        // The bundle resolves startup via promiseResolve, not this property.
        // Keep the original privately, while template waits remain bounded.
        defineProperty(engine.startup, "promise", { get: mathStartup });
        return then(startup, function () {
          mathStartupOwner = null;
          return engine;
        });
      }), resolve, reject);
    });
    then(mathLoading, noop, noop);
    return mathLoading;
  }

  function requestMath(elements) {
    const update = latest;
    const targets = elements || [findQA()];
    let expired = false;
    const work = then(loadMath(), function (engine) {
      return engine.whenReady(async function () {
        if (expired || update !== latest) return;
        mathOwner = update;
        try {
          await call(mathTypeset, engine, targets);
        } finally {
          mathOwner = null;
          if (expired || update !== latest) call(mathClear, engine, targets);
        }
      });
    });
    const waiting = mathDeadline(work, 8000);
    then(waiting, noop, function (error) {
      expired = true;
      if (update) update.mathFailed = true;
      mathError(error, update);
    });
    return waiting;
  }

  const mathFacade = {
    tex: {
      inlineMath: [["\\(", "\\)"]], displayMath: [["\\[", "\\]"]],
      processEscapes: false, packages: { "[+]": ["ams", "mhchem"] },
      formatError: function (jax, error) {
        // TeXError is not an Error subclass in this release. Preserve its
        // original id/message rather than reporting "[object Object]".
        mathError(error.id + ": " + error.message, mathOwner);
        return jax.formatError(error);
      },
    },
    typeset: function typeset(elements) { requestMath(elements); },
    typesetPromise: function typesetPromise(elements) { return requestMath(elements); },
    typesetClear: function typesetClear(elements) { if (mathClear) call(mathClear, mathEngine, elements); },
    startup: {},
  };
  defineProperty(mathFacade.startup, "promise", { get: mathStartup });

  function mathText(node) {
    if (call(nodeTypeOf, node) === 3) return call(characterData, node);
    // The bundle's default skipped tags. Inspect the live tree without
    // cloning custom elements or media just to detect literal delimiters.
    if (call(nodeTypeOf, node) === 1
        && call(stringIndexOf, ",script,noscript,style,textarea,pre,code,math,select,option,mjx-container,",
          "," + call(localNameOf, node) + ",") >= 0) return "";
    let text = "";
    for (let child = call(firstChildOf, node); child; child = call(nextSiblingOf, child)) text += mathText(child);
    return text;
  }

  function typesetSide(qa) {
    // A template already exhausted this side's math wait; still reveal it.
    if (latest && latest.mathFailed) return null;
    const text = mathText(qa);
    if (call(stringIndexOf, text, "\\(") < 0 && call(stringIndexOf, text, "\\[") < 0) return null;
    return then(requestMath([qa]), noop, noop);
  }

  async function runUpdate(update) {
    latest = update;
    if (update.side !== null) side = update.side;
    else update.side = side;
    updateSignal("started", update);
    try {
      // 1. Hook reset.
      updateHooks.length = 0;
      shownHooks.length = 0;
      updateHooks[0] = update.onUpdate;
      shownHooks[0] = update.onShown;
      // 2. Preload wait.
      await preloadAndWait(update.html);
      // 3. Hide.
      let qa = findQA();
      setOpacity(qa, "0");
      // 4. Video release.
      releaseVideos(qa);
      // Release MathItems before their source nodes leave the document.
      if (mathEngine) {
        try {
          const oldQA = qa;
          const cleanup = mathEngine.whenReady(function () { call(mathClear, mathEngine, [oldQA]); });
          if (mathOwner) {
            // Timed-out rendering may resume and rescan its target. Let it
            // finish against the detached old node, then clear its MathItems.
            qa = call(cloneNode, oldQA, false);
            call(replaceChild, call(parentNodeOf, oldQA), qa, oldQA);
            then(cleanup, noop, function (error) { mathError(error, update); });
          } else {
            await cleanup;
          }
        } catch (error) { mathError(error, update); }
      }
      // 5. Insert.
      held = [];
      call(innerHTMLSetter, qa, StringCtor(update.html));
      labelGeneratedReplays(qa, update.replayIDs);
      // 6. Script walk.
      await walkScripts(qa);
      // 7. Update hooks.
      await runHooks(updateHooks, "update-hook");
      // Scripts and hooks can add math. Finish it before reveal/settled on
      // reviewer faces and previews alike; math failures still reveal the side.
      const mathReady = typesetSide(findQA());
      if (mathReady) await mathReady;
      // 8. Show.
      qa = findQA();
      setOpacity(qa, "1");
      update.shown = true;
      confirmVisibleWhenShown();
      // 9. Shown phase, once presented.
      if (!presented) {
        if (queued()) return;
        if ((await parkUnpresented(update, qa)) !== "present") return;
      }
      await runHooks(shownHooks, "shown-hook");
      updateSignal("shown", update);
    } catch (error) {
      updateSignal("updateFailed", update, { error: describe(error) });
    }
  }

  // ---------------------------------------------------------------------------
  // The page-world functions (C6.1, C10).

  function setBodyClass(value) {
    const body = call(documentBody, doc);
    if (body) call(setAttribute, body, "class", StringCtor(value));
  }

  function focus(element) {
    try {
      if (element instanceof HTMLElementCtor) call(focusHTML, element);
      else if (element instanceof SVGElementCtor) call(focusSVG, element);
    } catch (error) {
      // Not focusable.
    }
  }

  // D4: the host passes IDs emitted by the display transform, never IDs
  // inferred from a class, prefix or data marker in card-authored markup.
  function labelGeneratedReplays(qa, ids) {
    if (!ids) return;
    const anchors = call(elementQueryAll, qa, "a[id]");
    for (let i = 0; i < ids.length; i++) {
      let target = null;
      for (let j = 0; j < anchors.length; j++) {
        if (call(getAttribute, anchors[j], "id") !== ids[i]) continue;
        // Ambiguous markup cannot establish transform ownership.
        if (target) { target = null; break; }
        target = anchors[j];
      }
      if (target) call(setAttribute, target, "aria-label", replayLabel);
    }
  }

  function _showQuestion(q, a, bodyclass, replayIDs) {
    enqueue({
      side: "question",
      html: q,
      replayIDs: replayIDs,
      onUpdate: function () {
        call(scrollToFn, win, 0, 0);
        setBodyClass(bodyclass);
      },
      onShown: function () {
        typedElement = call(getElementById, doc, "typeans");
        if (typedElement && automaticTypeAnswerFocus) focus(typedElement);
        afterImages(function () {
          startPreload(a, true);
        });
      },
    });
  }

  function _showAnswer(a, bodyclass, replayIDs) {
    enqueue({
      side: "answer",
      html: a,
      replayIDs: replayIDs,
      onUpdate: function () {
        if (typeof bodyclass === "string" && bodyclass !== "") setBodyClass(bodyclass);
        afterImages(function () {
          const answer = call(getElementById, doc, "answer");
          if (answer) call(scrollIntoView, answer, true);
        });
      },
      onShown: noop,
    });
  }

  function _updateQA(html, unused, onupdate, onshown) {
    return then(runUpdate({ side: null, html: html, identity: null, onUpdate: onupdate, onShown: onshown }), noop);
  }

  function _drawFlag(flag) {
    const element = call(getElementById, doc, "_flag");
    if (!element) return;
    if (flag === 0) call(setAttribute, element, "hidden", "");
    else call(removeAttribute, element, "hidden");
    call(setStyleProperty, call(styleOf, element), "color", "var(--flag-" + flag + ")");
  }

  function _drawMark(mark) {
    const element = call(getElementById, doc, "_mark");
    if (!element) return;
    if (!mark) call(setAttribute, element, "hidden", "");
    else call(removeAttribute, element, "hidden");
  }

  function getTypedAnswer() {
    if (!typedElement) return null;
    const value = typedElement.value;
    return value === undefined || value === null ? null : value;
  }

  function _typeAnsPress() {
    const event = eventGetter ? call(eventGetter, win) : win.event;
    if (event.key !== "Enter") return;
    if (side !== "question" || enterBelongsToIME(event)) return;
    const signal = { signal: "command", reveal: true, side: side };
    if (latest && latest.identity) {
      signal.card = latest.identity.card;
      signal.generation = latest.identity.generation;
    }
    emit(signal);
  }

  // Mac only (C10.5).
  function pycmd(command, callback) {
    const signal = { signal: "command", command: StringCtor(command), side: side };
    if (latest && latest.identity) {
      signal.card = latest.identity.card;
      signal.generation = latest.identity.generation;
    }
    emit(signal);
    if (typeof callback === "function") {
      call(setTimeoutFn, win, function () {
        apply(callback, undefined, [null]);
      }, 0);
    }
    return false;
  }

  // Packages (C10.7).
  const packageNames = isMac
    ? ["anki/packages", "anki/bridgecommand", "anki/reviewer"]
    : ["anki/packages", "anki/reviewer"];
  const packages = createObject(null);

  function require(name) {
    for (let i = 0; i < packageNames.length; i++) {
      if (packageNames[i] === name) return packages[name];
    }
    throw new ErrorCtor("Cannot require \"" + name + "\" at runtime.");
  }

  function listPackages() {
    const names = [];
    for (let i = 0; i < packageNames.length; i++) names[i] = packageNames[i];
    return names;
  }

  function hasPackages(...names) {
    for (let i = 0; i < names.length; i++) {
      let found = false;
      for (let j = 0; j < packageNames.length; j++) if (packageNames[j] === names[i]) found = true;
      if (!found) return false;
    }
    return true;
  }

  packages["anki/packages"] = { require: require, listPackages: listPackages, hasPackages: hasPackages };
  packages["anki/reviewer"] = { onUpdateHook: updateHooks, onShownHook: shownHooks };
  if (isMac) {
    // The global is looked up at each call; its result is not passed on.
    packages["anki/bridgecommand"] = {
      bridgeCommand: function bridgeCommand(command, callback) {
        apply(win.bridgeCommand, win, [command, callback]);
      },
    };
  }

  // ---------------------------------------------------------------------------
  // Native's messages (C14): identity, presentation, cache warming.

  call(nativeAdd, win, NATIVE_EVENT, function (event) {
    call(stopImmediate, event);
    let message;
    try {
      message = apply(parseJSON, null, [StringCtor(call(customDetail, event))]);
    } catch (error) {
      return;
    }
    if (message === null || typeof message !== "object") return;
    switch (message.op) {
      case "focusOwnership":
        automaticTypeAnswerFocus = message.owned === true;
        break;
      case "restoreTypedText": {
        // Native admits the matching recovery binding before this operation.
        // Keep C10's retained answer even on a comparison-only answer side.
        const input = call(getElementById, doc, "typeans");
        if (input instanceof TextAreaCtor) {
          typedElement = input;
          call(textAreaValueSetter, input, StringCtor(message.text));
        } else {
          typedElement = input instanceof InputCtor ? input : call(createElement, doc, "input");
          call(inputValueSetter, typedElement, StringCtor(message.text));
        }
        break;
      }
      case "identity":
        announced = { card: message.card, generation: message.generation };
        break;
      case "present":
        present(message.token, message.target);
        break;
      case "resume":
        // Native lifecycle return: discard attempts held during suspension.
        // Ordinary page presentation still resumes pre-rendered media (C8.2).
        held = [];
        present(message.token);
        break;
      case "unpresent":
        unpresent();
        break;
      case "warm":
        try {
          startPreload(message.html);
        } catch (error) {
          // Warming is best effort.
        }
        break;
      default:
        break;
    }
  }, true);

  // ---------------------------------------------------------------------------
  // Install the C9.1 public surface and the two non-configurable bridge entries.

  win.onUpdateHook = updateHooks;
  win.onShownHook = shownHooks;
  win._showQuestion = _showQuestion;
  win._showAnswer = _showAnswer;
  win._updateQA = _updateQA;
  win._drawFlag = _drawFlag;
  win._drawMark = _drawMark;
  win.getTypedAnswer = getTypedAnswer;
  win._typeAnsPress = _typeAnsPress;
  win.require = require;
  win.anki = {
    mutateNextCardStates: function mutateNextCardStates() {
      return resolved(undefined);
    },
  };
  win.MathJax = mathFacade;
  if (isMac) {
    win.ankiPlatform = "desktop";
    defineProperty(win, "pycmd", { value: pycmd, writable: true, enumerable: true, configurable: false });
    defineProperty(win, "bridgeCommand", { value: pycmd, writable: true, enumerable: true, configurable: false });
  }
})();
