// Meguri's pair-only isolated producer (render/host-reports H1–H10).
// Input, hit maps and idle locking belong to U180/U181. C66 owns rendering,
// resource budgets, paint evidence and scrolling; these are observations only.
(function () {
  "use strict";

  const call = Function.prototype.call.bind(Function.prototype.call);
  const ownKeys = Reflect.ownKeys;
  const descriptor = Object.getOwnPropertyDescriptor;
  const define = Object.defineProperty;
  const freeze = Object.freeze;
  const safeInteger = Number.isSafeInteger;
  const finite = Number.isFinite;
  const array = Array.isArray;
  const getPrototype = Object.getPrototypeOf;
  const objectPrototype = Object.prototype;
  const now = performance.now.bind(performance);
  const later = window.setTimeout.bind(window);
  const cancelTimer = window.clearTimeout.bind(window);
  const computed = window.getComputedStyle.bind(window);
  const supports = CSS.supports.bind(CSS);
  const add = EventTarget.prototype.addEventListener;
  const remove = EventTarget.prototype.removeEventListener;
  const query = Document.prototype.querySelectorAll;
  const attribute = Element.prototype.getAttribute;
  const create = Document.prototype.createElement;
  const then = Promise.prototype.then;
  const Observer = MutationObserver;
  const observe = Observer.prototype.observe;
  const disconnect = Observer.prototype.disconnect;
  const weakGet = WeakMap.prototype.get;
  const weakSet = WeakMap.prototype.set;
  const has = WeakSet.prototype.has;
  const insert = WeakSet.prototype.add;
  const WeakMapClass = WeakMap;
  const WeakSetClass = WeakSet;
  const LIMIT = 1000000;

  function getter(prototype, name) {
    const entry = descriptor(prototype, name);
    return entry && entry.get;
  }
  const bodyGetter = getter(Document.prototype, "body");
  const fontsGetter = getter(Document.prototype, "fonts");
  const complete = getter(HTMLImageElement.prototype, "complete");
  const currentSource = getter(HTMLImageElement.prototype, "currentSrc");
  const naturalWidth = getter(HTMLImageElement.prototype, "naturalWidth");
  const decode = HTMLImageElement.prototype.decode;
  const ImageElement = HTMLImageElement;
  const StyleElement = HTMLStyleElement;
  const LinkElement = HTMLLinkElement;
  const bodyHeight = getter(HTMLElement.prototype, "offsetHeight");
  const linkSheet = getter(HTMLLinkElement.prototype, "sheet");
  const styleSheet = getter(HTMLStyleElement.prototype, "sheet");
  const sheetRules = getter(CSSStyleSheet.prototype, "cssRules");
  const ruleType = getter(CSSRule.prototype, "type");
  const importSheet = getter(CSSImportRule.prototype, "styleSheet");
  const fontStatus = getter(FontFaceSet.prototype, "status");
  const faceStatus = getter(FontFace.prototype, "status");
  const eachFont = FontFaceSet.prototype.forEach;
  const styleProperty = CSSStyleDeclaration.prototype.getPropertyValue;
  const eventTrusted = getter(Event.prototype, "isTrusted");
  const eventTarget = getter(Event.prototype, "target");
  const eventType = getter(Event.prototype, "type");
  const canvasContext = HTMLCanvasElement.prototype.getContext;
  const canvasWidth = descriptor(HTMLCanvasElement.prototype, "width").set;
  const canvasHeight = descriptor(HTMLCanvasElement.prototype, "height").set;
  const contextAttributes = CanvasRenderingContext2D.prototype.getContextAttributes;
  const fillStyle = descriptor(CanvasRenderingContext2D.prototype, "fillStyle").set;
  const readFillStyle = getter(CanvasRenderingContext2D.prototype, "fillStyle");
  const fillRect = CanvasRenderingContext2D.prototype.fillRect;
  const pixels = CanvasRenderingContext2D.prototype.getImageData;
  const pixelData = getter(ImageData.prototype, "data");
  const pixelSpace = getter(ImageData.prototype, "colorSpace");
  let handler = null;
  let post = null;
  try {
    handler = window.webkit.messageHandlers.meguriReviewHost;
    post = handler.postMessage;
  } catch (_) {}

  let revision = 0;
  let context = null;
  let background = null;
  let resources = null;
  let lastBackgroundRequest = 0;
  let lastResourcesRequest = 0;
  let disposed = false;
  let gesture = null;
  let inputSerial = 0;

  function exact(value, names) {
    if (!value || typeof value !== "object" || array(value)) return false;
    const prototype = getPrototype(value);
    if (prototype !== objectPrototype && prototype !== null) return false;
    const keys = ownKeys(value);
    if (keys.length !== names.length) return false;
    for (const name of names) {
      const entry = descriptor(value, name);
      if (!entry || !("value" in entry)) return false;
    }
    return true;
  }
  function positive(value, maximum) {
    return typeof value === "number" && finite(value) && value > 0 && value <= maximum;
  }
  function positiveInteger(value) { return safeInteger(value) && value > 0; }
  function stop(request) {
    if (!request) return;
    request.active = false;
    cancelTimer(request.timer);
    if (request.cleanup) request.cleanup();
    request.cleanup = null;
    // Promise continuations retain this small token, never a card DOM/context.
    request.wake.notify = null;
    if (background === request) background = null;
    if (resources === request) resources = null;
  }
  function clear() {
    context = null;
    gesture = null; inputSerial = 0; lastTouchStart = null; composingNow = false;
    stop(background); stop(resources);
  }
  function current(request) {
    return !disposed && request.active && context === request.context && now() < request.deadline;
  }
  function report(request, payload) {
    if (!current(request)) { stop(request); return; }
    const start = request.context;
    stop(request);
    if (!post) return;
    try {
      const reply = call(post, handler, {
        signal: "hostProbe", version: 1, card: start.card, generation: start.generation,
        side: start.side, viewRevision: start.viewRevision, payload: payload
      });
      if (reply) call(then, reply, undefined, function () {});
    } catch (_) {}
  }
  function backgroundProbe(request) {
    try {
      const body = call(bodyGetter, document);
      if (!body) return;
      const color = call(styleProperty, computed(body), "background-color");
      if (!supports("color", color)) return;
      // Browser color conversion, including modern CSS colors, into explicit
      // sRGB unorm8. A detached pixel never alters the visible card or styles.
      const canvas = call(create, document, "canvas");
      call(canvasWidth, canvas, 1); call(canvasHeight, canvas, 1);
      const drawing = call(canvasContext, canvas, "2d", { colorSpace: "srgb", willReadFrequently: true });
      if (!drawing || !contextAttributes || call(contextAttributes, drawing).colorSpace !== "srgb") return;
      call(fillStyle, drawing, "#000000");
      call(fillStyle, drawing, color);
      const first = call(readFillStyle, drawing);
      call(fillStyle, drawing, "#ffffff");
      call(fillStyle, drawing, color);
      if (first !== call(readFillStyle, drawing)) return;
      call(fillRect, drawing, 0, 0, 1, 1);
      const sample = call(pixels, drawing, 0, 0, 1, 1, { colorSpace: "srgb" });
      if (!pixelSpace || call(pixelSpace, sample) !== "srgb") return;
      const data = call(pixelData, sample);
      if (data.length !== 4) return;
      report(request, { kind: "background", request: request.id,
        rgba: [data[0] / 255, data[1] / 255, data[2] / 255, data[3] / 255] });
    } catch (_) {
      // Missing/unsupported color observations remain unavailable. Native
      // expires the request; no fabricated black/white or budgetExpired reply.
    }
  }

  // This separate activation prevents a never-settling decode promise from
  // capturing the observer's scope (document nodes, fonts or native context).
  function watchDecode(promise, result, wake) {
    call(then, promise, function () {
      result.state = "ready";
      if (wake.notify) wake.notify();
    }, function () {
      result.state = "failed";
      if (wake.notify) wake.notify();
    });
  }

  function resourceProbe(request, purpose) {
    const images = new WeakMapClass();
    const failures = new WeakSetClass();
    const wake = request.wake;
    let observer = null;
    let fonts = null;
    function cleanup() {
      if (observer) call(disconnect, observer);
      call(remove, document, "load", changed, true);
      call(remove, document, "error", changed, true);
      if (fonts) {
        call(remove, fonts, "loadingdone", changed);
        call(remove, fonts, "loadingerror", changed);
      }
    }
    request.cleanup = cleanup;
    function changed(event) {
      // Only engine-trusted resource events can contribute failure evidence
      // or wake a native-property scan. Page-authored events establish nothing.
      const trusted = eventTrusted ? call(eventTrusted, event) : event.isTrusted;
      if (!trusted) return;
      const target = call(eventTarget, event);
      const type = call(eventType, event);
      if (type === "error" && target) call(insert, failures, target);
      scan();
    }
    let rulesVisited = 0;
    function rulesReady(sheet, depth) {
      if (!sheet) return false;
      if (depth > 32) throw new Error("unsupported stylesheet depth");
      let rules;
      try { rules = call(sheetRules, sheet); }
      catch (_) { return false; } // Unreadable CSSOM supplies no load-failure evidence.
      rulesVisited += rules.length;
      if (rulesVisited > LIMIT) throw new Error("stylesheet limit");
      let ready = true;
      for (let i = 0; i < rules.length; i++) {
        const rule = rules[i];
        if (call(ruleType, rule) === 3 && !rulesReady(call(importSheet, rule), depth + 1)) ready = false;
      }
      return ready;
    }
    function scan() {
      if (!current(request)) { stop(request); return; }
      let imagesPending = 0, fontsPending = 0, stylesPending = 0;
      let failed = false;
      rulesVisited = 0;
      try {
        // Resolve current style/layout so font loading is actually requested
        // by WebKit before inspecting its font set; no rendering gate is added.
        const body = call(bodyGetter, document);
        if (!body) throw new Error("missing body");
        call(bodyHeight, body);
        const nodes = call(query, document, "img,link,style");
        if (nodes.length > LIMIT) throw new Error("resource limit");
        for (let i = 0; i < nodes.length; i++) {
          const node = nodes[i];
          if (node instanceof ImageElement) {
            const src = call(attribute, node, "src");
            const srcset = call(attribute, node, "srcset");
            const selected = call(currentSource, node);
            if (!src && !srcset && !selected && call(complete, node)) continue;
            if (!call(complete, node)) { imagesPending++; continue; }
            if (!call(naturalWidth, node)) { failed = true; continue; }
            let record = call(weakGet, images, node);
            const sizes = call(attribute, node, "sizes");
            if (!record || record.src !== src || record.srcset !== srcset || record.sizes !== sizes || record.selected !== selected) {
              const result = { state: "pending" };
              record = { src: src, srcset: srcset, sizes: sizes, selected: selected, result: result };
              call(weakSet, images, node, record);
              // Only this bounded token and per-image result enter a promise.
              // Cancellation disconnects its callback, even if decode hangs.
              watchDecode(call(decode, node), result, wake);
            }
            if (record.result.state === "pending") imagesPending++;
            if (record.result.state === "failed") failed = true;
          } else if (node instanceof StyleElement) {
            if (call(has, failures, node)) failed = true;
            if (!rulesReady(call(styleSheet, node), 0)) stylesPending++;
          } else if (node instanceof LinkElement) {
            // The browser's token list decides membership, not a CSS/URL parser.
            const tokens = call(relList, node);
            if (call(relContains, tokens, "stylesheet")) {
              if (call(has, failures, node)) failed = true;
              if (!rulesReady(call(linkSheet, node), 0)) stylesPending++;
            } else if (call(relContains, tokens, "preload")) {
              const kind = call(attribute, node, "as");
              if (kind !== "font" && kind !== "image" && kind !== "style") continue;
              if (call(has, failures, node)) failed = true;
              // Preload links have no native completion property. A queued
              // load event could describe a replaced href, even when trusted.
              // Keep these unavailable until removed or the native deadline;
              // C66's own preload tracking/removal remains authoritative.
              if (kind === "font") fontsPending++;
              else if (kind === "image") imagesPending++;
              else stylesPending++;
            }
          }
        }
        if (!fonts || !eachFont || !fontStatus || !faceStatus) throw new Error("font observation unavailable");
        let faces = 0;
        call(eachFont, fonts, function (face) {
          if (++faces > LIMIT) throw new Error("font limit");
          const state = call(faceStatus, face);
          if (state === "loading") fontsPending++;
          else if (state === "error") failed = true;
          else if (state !== "loaded" && state !== "unloaded") failed = true;
        });
        // Unused unloaded faces are not required loads. A loading font set
        // with no individually exposed face still cannot be reported ready.
        const status = call(fontStatus, fonts);
        if (status === "loading" && fontsPending === 0) fontsPending = 1;
        else if (status !== "loading" && status !== "loaded") failed = true;
      } catch (_) { return; } // Unknown inspection stays pending until native expiry.
      if (imagesPending > LIMIT || fontsPending > LIMIT || stylesPending > LIMIT) { stop(request); return; }
      if (failed || imagesPending + fontsPending + stylesPending === 0) {
        report(request, { kind: "resources", request: request.id, purpose: purpose,
          state: failed ? "failed" : "ready", imagesPending: imagesPending,
          fontsPending: fontsPending, stylesPending: stylesPending });
      }
    }
    wake.notify = scan;
    try {
      fonts = call(fontsGetter, document);
      call(add, document, "load", changed, true);
      call(add, document, "error", changed, true);
      if (fonts) {
        call(add, fonts, "loadingdone", changed);
        call(add, fonts, "loadingerror", changed);
      }
      observer = new Observer(scan);
      call(observe, observer, document, { childList: true, subtree: true, attributes: true,
        characterData: true, attributeFilter: ["src", "srcset", "sizes", "type", "href", "rel", "as", "media", "disabled", "style", "class", "loading"] });
      scan();
    } catch (_) {
      // Observation setup failure is unavailable, never an observed load error.
    }
  }
  const relList = getter(HTMLLinkElement.prototype, "relList");
  const relContains = DOMTokenList.prototype.contains;

  // Review taps (U180; R404–R408, R412, R415). One observation per finished
  // single-pointer gesture on the presented side, sent after the page's own
  // handlers for that gesture have run. WebKit on iOS need not dispatch a
  // click for a tap on plain content, so the observation closes the pointer
  // sequence (with a click, when one follows within it). Native owns movement, duration,
  // finger count, the zones and every decision; this only observes.
  const EXEMPT = "a,button,input,select,textarea,label,summary,details,audio,video," +
    "[onclick],[contenteditable],[role=button],.tappable,.replay-button,#typeans";
  const QUERY = "meguri:query";
  // How long a gesture waits for the click a card handler could see (ms).
  const CLICK_WAIT = 350;
  const matches = Element.prototype.matches;
  const parentNodeOf = getter(Node.prototype, "parentNode");
  const shadowHostOf = getter(ShadowRoot.prototype, "host");
  const shadowRootOf = getter(Element.prototype, "shadowRoot");
  const shadowActiveOf = getter(ShadowRoot.prototype, "activeElement");
  const connectedOf = getter(Node.prototype, "isConnected");
  const composedPath = Event.prototype.composedPath;
  const prevented = getter(Event.prototype, "defaultPrevented");
  const stamp = getter(Event.prototype, "timeStamp");
  const pointerIdOf = getter(PointerEvent.prototype, "pointerId");
  const pointerTypeOf = getter(PointerEvent.prototype, "pointerType");
  const primaryOf = getter(PointerEvent.prototype, "isPrimary");
  const clientXOf = getter(MouseEvent.prototype, "clientX");
  const clientYOf = getter(MouseEvent.prototype, "clientY");
  const buttonOf = getter(MouseEvent.prototype, "button");
  const TouchClass = window.TouchEvent;
  const touchesOf = TouchClass ? getter(TouchClass.prototype, "touches") : null;
  const touchCount = window.TouchList ? getter(TouchList.prototype, "length") : null;
  const activeOf = getter(Document.prototype, "activeElement");
  const rootOf = getter(Document.prototype, "documentElement");
  const editableOf = getter(HTMLElement.prototype, "isContentEditable");
  const idOf = getter(Element.prototype, "id");
  const localNameOf = getter(Element.prototype, "localName");
  const selectionOf = Document.prototype.getSelection;
  const collapsedOf = getter(Selection.prototype, "isCollapsed");
  const rangesOf = getter(Selection.prototype, "rangeCount");
  const ViewportClass = window.VisualViewport;
  const viewportOf = getter(Window.prototype, "visualViewport") || getter(window, "visualViewport");
  const viewportWidth = ViewportClass && getter(ViewportClass.prototype, "width");
  const viewportHeight = ViewportClass && getter(ViewportClass.prototype, "height");
  const viewportLeft = ViewportClass && getter(ViewportClass.prototype, "offsetLeft");
  const viewportTop = ViewportClass && getter(ViewportClass.prototype, "offsetTop");
  const viewportScale = ViewportClass && getter(ViewportClass.prototype, "scale");
  const dispatch = EventTarget.prototype.dispatchEvent;
  const CustomEventClass = CustomEvent;
  const ElementClass = Element;
  const NodeClass = Node;
  const ShadowRootClass = ShadowRoot;
  const push = Array.prototype.push;
  let composingNow = false;
  let lastTouchStart = null;
  let layoutRevision = 0;
  let lastGeometry = null;

  function trusted(event) {
    return eventTrusted ? call(eventTrusted, event) === true : event.isTrusted === true;
  }
  // Fails closed: an unreadable state never lets a tap act.
  function hasSelection() {
    try {
      const selection = call(selectionOf, document);
      return !!selection && call(rangesOf, selection) > 0 && !call(collapsedOf, selection);
    } catch (_) { return true; }
  }
  function focusState() {
    try {
      let active = call(activeOf, document);
      // Focus inside an open shadow root: the focused element itself.
      for (let depth = 0; active && depth < 32; depth++) {
        const root = active instanceof ElementClass ? call(shadowRootOf, active) : null;
        const inner = root ? call(shadowActiveOf, root) : null;
        if (!inner) break;
        active = inner;
      }
      if (!active || active === call(bodyGetter, document) || active === call(rootOf, document)) {
        return { editable: false, typeans: false };
      }
      const name = call(localNameOf, active);
      const editable = name === "input" || name === "textarea" || name === "select" ||
        (active instanceof HTMLElement && call(editableOf, active) === true);
      return { editable: editable, typeans: editable && call(idOf, active) === "typeans" };
    } catch (_) { return { editable: true, typeans: false }; }
  }
  function parentOf(node) {
    const parent = call(parentNodeOf, node);
    if (parent) return parent;
    return node instanceof ShadowRootClass ? call(shadowHostOf, node) : null;
  }
  // R405's targets and their descendants; below <body>, an element styled as
  // clickable (`cursor: pointer`) or one with its own tap listener (C13's
  // registry, asked through the runtime) is the card's control too.
  function exempt(target) {
    try {
      if (!(target instanceof NodeClass)) return true;
      const body = call(bodyGetter, document);
      let belowBody = true;
      for (let node = target; node; node = parentOf(node)) {
        if (!(node instanceof ElementClass)) continue;
        if (node === body) belowBody = false;
        if (call(matches, node, EXEMPT)) return true;
        if (belowBody && call(styleProperty, computed(node), "cursor") === "pointer") return true;
      }
      const query = new CustomEventClass(QUERY, { detail: "tapTarget", bubbles: true, cancelable: true, composed: true });
      // Not cancelled: a card listener could see the tap, or no one answered.
      return call(dispatch, target, query) !== false;
    } catch (_) { return true; }
  }
  function geometry() {
    try {
      const viewport = viewportOf ? call(viewportOf, window) : null;
      if (!viewport || !viewportWidth) return null;
      const next = { width: call(viewportWidth, viewport), height: call(viewportHeight, viewport),
        offsetX: call(viewportLeft, viewport), offsetY: call(viewportTop, viewport), scale: call(viewportScale, viewport) };
      if (!positive(next.width, LIMIT) || !positive(next.height, LIMIT) || !positive(next.scale, 1000) ||
          !finite(next.offsetX) || !finite(next.offsetY) ||
          next.offsetX > 1e9 || next.offsetX < -1e9 || next.offsetY > 1e9 || next.offsetY < -1e9) return null;
      const last = lastGeometry;
      if (!last || last.width !== next.width || last.height !== next.height || last.offsetX !== next.offsetX ||
          last.offsetY !== next.offsetY || last.scale !== next.scale || layoutRevision === 0) {
        if (layoutRevision >= 9007199254740991) return null;
        layoutRevision += 1;
        lastGeometry = next;
      }
      return { width: next.width, height: next.height, offsetX: next.offsetX, offsetY: next.offsetY,
        scale: next.scale, layoutRevision: layoutRevision };
    } catch (_) { return null; }
  }
  function remember(event) {
    if (gesture && !gesture.done) call(push, gesture.events, event);
  }
  function pointerDown(event) {
    if (disposed || !trusted(event)) return;
    const time = call(stamp, event);
    // A second pointer joins the gesture; a primary one starts a new gesture
    // (no other pointer is down then), so a lost end never blocks the next tap.
    if (!call(primaryOf, event)) { if (gesture && !gesture.done) gesture.multi = true; return; }
    gesture = null;
    if (!context || context.visibility !== "presented") return;
    const focus = focusState();
    gesture = { context: context, id: call(pointerIdOf, event), start: time, events: [event],
      selection: hasSelection(), editable: focus.editable, typeans: focus.typeans, composing: composingNow,
      multi: false, done: false, end: null, target: null, exempt: true, x: 0, y: 0, awaitClick: false, timer: 0,
      clickSeen: false, clickBubbled: false };
    if (lastTouchStart && time - call(stamp, lastTouchStart) < 100) call(push, gesture.events, lastTouchStart);
  }
  function touchStart(event) {
    if (disposed || !trusted(event)) return;
    lastTouchStart = event;
    if (!gesture || gesture.done) return;
    remember(event);
    if (touchesOf && touchCount && call(touchCount, call(touchesOf, event)) > 1) gesture.multi = true;
  }
  // Scrolling, pinching or a long-press selection: the gesture is WebKit's.
  function cancelled(event) {
    if (trusted(event) && gesture && !gesture.done) gesture = null;
  }
  // Whether a card listener anywhere on the target's path could still handle
  // the click that may follow (C13's question, window and document included).
  function clickListened(target) {
    const kinds = ["click", "mousedown", "mouseup"];
    for (let i = 0; i < kinds.length; i++) {
      const query = new CustomEventClass(QUERY, { detail: kinds[i], bubbles: true, cancelable: true, composed: true });
      if (call(dispatch, target, query) !== false) return true;
    }
    return false;
  }
  function pointerUp(event) {
    if (disposed || !trusted(event) || !gesture || gesture.done) return;
    if (call(pointerIdOf, event) !== gesture.id) { gesture.multi = true; return; }
    const g = gesture;
    g.done = true;
    call(push, g.events, event);
    g.end = event;
    try {
      const path = call(composedPath, event);
      g.target = path.length > 0 ? path[0] : call(eventTarget, event);
      g.x = call(clientXOf, event); g.y = call(clientYOf, event);
      g.exempt = exempt(g.target);
      // On iOS a click comes later, from a separate commit of the tap. When a
      // card handler could see it, wait for it (bounded), so its
      // preventDefault counts; otherwise close after this dispatch.
      g.awaitClick = !g.exempt && clickListened(g.target);
    } catch (_) { g.exempt = true; g.awaitClick = false; }
    g.timer = later(function () { finish(g); }, g.awaitClick ? CLICK_WAIT : 0);
  }
  function lateEvent(event) {
    if (!trusted(event)) return;
    // touchend and a click after pointerup belong to the closing gesture.
    if (gesture && gesture.done && gesture.end) call(push, gesture.events, event);
    else remember(event);
  }
  // Clicks are taken first (capture, before any card handler) and seen again
  // on the way back to the window: one that never returns was stopped by a
  // card handler, which then owns the tap.
  function clickCaptured(event) {
    if (!trusted(event)) return;
    const g = gesture;
    if (!g || !g.done || !g.end) { remember(event); return; }
    call(push, g.events, event);
    g.clickSeen = true;
    if (g.awaitClick) {
      g.awaitClick = false;
      cancelTimer(g.timer);
      g.timer = later(function () { finish(g); }, 0);
    }
  }
  function clickBubbled(event) {
    if (trusted(event) && gesture && gesture.clickSeen) gesture.clickBubbled = true;
  }
  function finish(g) {
    if (gesture !== g) return;
    gesture = null;
    const start = g.context;
    // R415: a gesture belongs to the side presented when it began.
    if (disposed || !post || g.multi || context !== start) return;
    let defaultPrevented = false;
    for (let i = 0; i < g.events.length; i++) {
      if (call(prevented, g.events[i])) defaultPrevented = true;
    }
    let isExempt = g.exempt;
    try { if (!call(connectedOf, g.target)) isExempt = true; } catch (_) { isExempt = true; }
    if (g.clickSeen && !g.clickBubbled) isExempt = true;
    const view = geometry();
    if (!view) return;
    const x = g.x - view.offsetX, y = g.y - view.offsetY;
    if (!finite(x) || !finite(y)) return;
    const type = call(pointerTypeOf, g.end);
    let button = call(buttonOf, g.end);
    if (!safeInteger(button) || button < 0 || button > 4) button = 0;
    const time = call(stamp, g.end);
    if (inputSerial >= 9007199254740991) return;
    inputSerial += 1;
    try {
      const reply = call(post, handler, {
        signal: "hostInput", version: 1, card: start.card, generation: start.generation,
        side: start.side, viewRevision: start.viewRevision, serial: inputSerial,
        payload: { kind: "click", timeMs: finite(time) && time > 0 ? time : 0, defaultPrevented: defaultPrevented,
          exempt: isExempt, selection: g.selection || hasSelection(), editable: g.editable,
          typeans: g.editable && g.typeans, composing: g.composing, geometry: view,
          x: x < 0 ? 0 : x > view.width ? view.width : x, y: y < 0 ? 0 : y > view.height ? view.height : y,
          button: button, pointer: type === "touch" || type === "mouse" || type === "pen" ? type : "unknown" }
      });
      if (reply) call(then, reply, undefined, function () {});
    } catch (_) {}
  }
  function noop() {}
  // R408: CSS :active styles need a touch listener on the document. Every
  // listener here is passive: it never cancels, so it never holds up scrolling.
  call(add, document, "touchstart", noop, { passive: true });
  call(add, window, "pointerdown", pointerDown, { capture: true, passive: true });
  call(add, window, "touchstart", touchStart, { capture: true, passive: true });
  call(add, window, "pointercancel", cancelled, { capture: true, passive: true });
  call(add, window, "touchcancel", cancelled, { capture: true, passive: true });
  call(add, window, "pointerup", pointerUp, { passive: true });
  call(add, window, "touchend", lateEvent, { passive: true });
  call(add, window, "click", clickCaptured, { capture: true, passive: true });
  call(add, window, "click", clickBubbled, { passive: true });
  call(add, window, "compositionstart", function (event) { if (trusted(event)) composingNow = true; }, true);
  call(add, window, "compositionend", function (event) { if (trusted(event)) composingNow = false; }, true);

  function receive(message) {
    try { return receiveMessage(message); } catch (_) { return false; }
  }
  function receiveMessage(message) {
    if (disposed || !message || typeof message !== "object") return false;
    if (message.op === "clear") {
      if (!exact(message, ["op", "viewRevision"]) || !positiveInteger(message.viewRevision) || message.viewRevision <= revision) return false;
      clear(); revision = message.viewRevision;
      return true;
    }
    if (message.op === "context") {
      if (!exact(message, ["op", "card", "generation", "side", "viewRevision", "visibility", "bounds", "pageZoom"]) ||
          !safeInteger(message.card) || !safeInteger(message.generation) || message.generation < 0 ||
          (message.side !== "question" && message.side !== "answer") ||
          !positiveInteger(message.viewRevision) || message.viewRevision !== revision || context !== null ||
          (message.visibility !== "preparing" && message.visibility !== "presented") ||
          !exact(message.bounds, ["width", "height"]) ||
          !positive(message.bounds.width, LIMIT) || !positive(message.bounds.height, LIMIT) ||
          !positive(message.pageZoom, 1000)) return false;
      // Preserve safe native identity values exactly, without coercion.
      context = freeze({ card: message.card, generation: message.generation, side: message.side,
        viewRevision: revision, visibility: message.visibility,
        bounds: freeze({ width: message.bounds.width, height: message.bounds.height }), pageZoom: message.pageZoom });
      return true;
    }
    if (message.op !== "probe") return false;
    const resource = message.kind === "resources";
    if (!exact(message, resource ? ["op", "kind", "request", "viewRevision", "timeoutMs", "purpose"] :
        ["op", "kind", "request", "viewRevision", "timeoutMs"]) ||
        (!resource && message.kind !== "background") || !context || message.viewRevision !== revision ||
        !positiveInteger(message.request) || message.request <= (resource ? lastResourcesRequest : lastBackgroundRequest) ||
        message.request === (resource ? lastBackgroundRequest : lastResourcesRequest) ||
        !positiveInteger(message.timeoutMs) || message.timeoutMs > 2000 ||
        (resource && message.purpose !== "warmup" && message.purpose !== "preload")) return false;
    if (resource) lastResourcesRequest = message.request; else lastBackgroundRequest = message.request;
    stop(resource ? resources : background);
    const request = { id: message.request, context: context, active: true, deadline: now() + message.timeoutMs,
      timer: null, cleanup: null, wake: { notify: null } };
    if (resource) resources = request; else background = request;
    // Expiry only disconnects. The native monotonic deadline owns timeout.
    request.timer = later(function () { stop(request); }, message.timeoutMs);
    if (resource) resourceProbe(request, message.purpose); else backgroundProbe(request);
    return true;
  }

  function retireProducer() {
    if (disposed) return;
    disposed = true;
    call(remove, window, "pagehide", endDocumentObservation);
    clear();
  }
  function endDocumentObservation(event) {
    const fromEngine = eventTrusted ? call(eventTrusted, event) : event.isTrusted;
    if (!fromEngine) return;
    retireProducer();
  }
  const receiver = freeze({ receive: receive });
  // Replacement invokes the prior installation's setter, disconnecting its
  // observers/promises without inventing a native revision or an ABI operation.
  window.meguriReviewHost = receiver;
  define(window, "meguriReviewHost", { configurable: true, enumerable: true,
    get: function () { return receiver; }, set: retireProducer });
  call(add, window, "pagehide", endDocumentObservation);
})();
