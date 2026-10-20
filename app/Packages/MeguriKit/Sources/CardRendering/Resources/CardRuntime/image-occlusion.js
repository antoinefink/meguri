// Meguri's review-only image occlusion runtime, from render/image-occlusion-review.
// No editor assets or dependencies (R1960). State belongs to this page only.
(function () {
  "use strict";

  const win = window;
  const doc = document;
  const apply = Reflect.apply;
  const descriptor = Object.getOwnPropertyDescriptor;
  const NumberFn = Number;
  const StringFn = String;
  const float = parseFloat;
  const integer = parseInt;
  const round = Math.round;
  const floor = Math.floor;
  const min = Math.min;
  const max = Math.max;
  const pi = Math.PI;
  const now = Date.now;
  const split = String.prototype.split;
  const PromiseCtor = Promise;
  const then = Promise.prototype.then;
  const resolve = Promise.resolve;
  const frame = win.requestAnimationFrame;
  // runtime.js has already installed its registry. Capture that counted add,
  // not a pristine native add: IO buttons and the key belong to the card.
  const add = EventTarget.prototype.addEventListener;
  const remove = EventTarget.prototype.removeEventListener;
  const byId = Document.prototype.getElementById;
  const query = Document.prototype.querySelector;
  const queryAll = Document.prototype.querySelectorAll;
  const attribute = Element.prototype.getAttribute;
  const isDiv = HTMLDivElement;
  const nodeListLength = descriptor(NodeList.prototype, "length").get;
  const metricAscent = descriptor(TextMetrics.prototype, "actualBoundingBoxAscent").get;
  const metricDescent = descriptor(TextMetrics.prototype, "actualBoundingBoxDescent").get;
  const metricWidth = descriptor(TextMetrics.prototype, "width").get;
  const connected = descriptor(Node.prototype, "isConnected").get;
  const style = descriptor(HTMLElement.prototype, "style").get;
  const innerText = descriptor(HTMLElement.prototype, "innerText").set;
  const clientWidth = descriptor(Element.prototype, "clientWidth").get;
  const clientHeight = descriptor(Element.prototype, "clientHeight").get;
  const naturalWidth = descriptor(HTMLImageElement.prototype, "naturalWidth").get;
  const naturalHeight = descriptor(HTMLImageElement.prototype, "naturalHeight").get;
  const complete = descriptor(HTMLImageElement.prototype, "complete").get;
  const canvasWidth = descriptor(HTMLCanvasElement.prototype, "width");
  const canvasHeight = descriptor(HTMLCanvasElement.prototype, "height");
  const context2D = HTMLCanvasElement.prototype.getContext;
  const computedStyle = win.getComputedStyle;
  const cssGet = CSSStyleDeclaration.prototype.getPropertyValue;
  const cssSet = CSSStyleDeclaration.prototype.setProperty;
  const dpr = descriptor(win, "devicePixelRatio") || descriptor(Window.prototype, "devicePixelRatio");
  const key = descriptor(KeyboardEvent.prototype, "key").get;
  const ResizeCtor = ResizeObserver;
  const observeSize = ResizeObserver.prototype.observe;
  const disconnectSize = ResizeObserver.prototype.disconnect;
  const MutationCtor = MutationObserver;
  const observeDOM = MutationObserver.prototype.observe;
  const disconnectDOM = MutationObserver.prototype.disconnect;
  const mediaQuery = win.matchMedia;
  const ctxProto = CanvasRenderingContext2D.prototype;
  const ctxMethods = {};
  const methodNames = ["save", "restore", "translate", "rotate", "scale", "fillRect", "strokeRect",
    "beginPath", "ellipse", "closePath", "fill", "stroke", "moveTo", "lineTo", "measureText", "fillText", "clearRect"];
  for (let i = 0; i < methodNames.length; i += 1) ctxMethods[methodNames[i]] = ctxProto[methodNames[i]];
  const ctxSetters = {};
  const propertyNames = ["fillStyle", "strokeStyle", "lineWidth", "font", "textBaseline"];
  for (let i = 0; i < propertyNames.length; i += 1) ctxSetters[propertyNames[i]] = descriptor(ctxProto, propertyNames[i]).set;

  function call(fn, self, ...args) { return apply(fn, self, args); }
  function ctx(c, method, ...args) { return apply(ctxMethods[method], c, args); }
  function set(c, property, value) { call(ctxSetters[property], c, value); }
  function id(name) { return call(byId, doc, name); }
  function image() { return call(query, doc, "#image-occlusion-container img"); }
  function attr(element, name) { return call(attribute, element, "data-" + name); }
  function ratio() { return dpr && dpr.get ? call(dpr.get, win) : win.devicePixelRatio; }
  function option(options, name, fallback) {
    const value = options[name];
    return value === undefined ? fallback : value;
  }

  class Shape {
    constructor(options = {}) {
      this.left = option(options, "left", 0);
      this.top = option(options, "top", 0);
      this.angle = option(options, "angle", 0);
      this.fill = option(options, "fill", this instanceof Text ? "#000000" : "#ffeba2");
      this.occludeInactive = options.occludeInactive;
      this.ordinal = options.ordinal;
      this.id = undefined;
    }
    absolutePosition(size) { return { left: round(this.left * size.width), top: round(this.top * size.height) }; }
    normalPosition(size) { return { left: this.left / size.width, top: this.top / size.height }; }
    toAbsolute(size) { return convert(this, size, true); }
    toNormal(size) { return convert(this, size, false); }
  }
  class Rectangle extends Shape {
    constructor(options = {}) {
      super(options);
      this.id = "rect-" + now();
      this.width = option(options, "width", 0);
      this.height = option(options, "height", 0);
    }
  }
  class Ellipse extends Shape {
    constructor(options = {}) {
      super(options);
      this.id = "ellipse-" + now();
      this.rx = option(options, "rx", 0);
      this.ry = option(options, "ry", 0);
    }
  }
  class Polygon extends Shape {
    constructor(options = {}) {
      super(options);
      this.id = "polygon-" + now();
      this.points = option(options, "points", []);
    }
  }
  class Text extends Shape {
    constructor(options = {}) {
      super(options);
      this.id = "text-" + now();
      this.text = option(options, "text", "");
      this.scaleX = option(options, "scaleX", 1);
      this.scaleY = option(options, "scaleY", 1);
      this.fontSize = options.fontSize;
    }
  }
  function convert(shape, size, absolute) {
    // Read constructor inputs once, in constructor order. Construction still
    // supplies defaults; conversion restores the unchanged inputs verbatim.
    const options = {
      left: shape.left, top: shape.top, angle: shape.angle, fill: shape.fill,
      occludeInactive: shape.occludeInactive, ordinal: shape.ordinal,
    };
    let result;
    if (shape instanceof Rectangle) {
      options.width = shape.width; options.height = shape.height;
      result = new Rectangle(options);
    } else if (shape instanceof Ellipse) {
      options.rx = shape.rx; options.ry = shape.ry;
      result = new Ellipse(options);
    } else if (shape instanceof Polygon) {
      options.points = shape.points;
      result = new Polygon(options);
    } else if (shape instanceof Text) {
      options.text = shape.text; options.scaleX = shape.scaleX; options.scaleY = shape.scaleY; options.fontSize = shape.fontSize;
      result = new Text(options);
      result.text = options.text; result.scaleX = options.scaleX; result.scaleY = options.scaleY;
    } else result = new Shape(options);
    result.angle = options.angle;
    result.fill = options.fill;
    result.occludeInactive = options.occludeInactive;
    result.ordinal = options.ordinal;
    function dimension(value, extent) { return absolute ? round(value * extent) : value / extent; }
    result.left = dimension(shape.left, size.width);
    result.top = dimension(shape.top, size.height);
    if (shape instanceof Rectangle) {
      result.width = dimension(shape.width, size.width);
      result.height = dimension(shape.height, size.height);
    } else if (shape instanceof Ellipse) {
      result.rx = dimension(shape.rx, size.width);
      result.ry = dimension(shape.ry, size.height);
    } else if (shape instanceof Polygon) {
      result.points = [];
      for (let i = 0; i < shape.points.length; i += 1) {
        const p = shape.points[i];
        result.points[i] = { x: dimension(p.x, size.width), y: dimension(p.y, size.height) };
      }
    } else if (shape instanceof Text) {
      result.fontSize = absolute ? (shape.fontSize ? shape.fontSize * size.height : 40) : (shape.fontSize || 40) / size.height;
    }
    return result;
  }

  function drawShape({ context: c, size, shape, fill, stroke, strokeWidth }) {
    const s = shape.toAbsolute(size);
    set(c, "fillStyle", fill);
    set(c, "strokeStyle", stroke);
    set(c, "lineWidth", strokeWidth);
    // Styles remain assigned; transforms/font/baseline are restored even on error.
    ctx(c, "save");
    try {
      if (s instanceof Polygon) {
        let mx = s.points[0].x;
        let my = s.points[0].y;
        for (let i = 1; i < s.points.length; i += 1) {
          if (s.points[i].x < mx) mx = s.points[i].x;
          if (s.points[i].y < my) my = s.points[i].y;
        }
        ctx(c, "beginPath");
        for (let i = 0; i < s.points.length; i += 1) {
          ctx(c, i === 0 ? "moveTo" : "lineTo", s.points[i].x + (s.left - mx), s.points[i].y + (s.top - my));
        }
        ctx(c, "closePath");
        ctx(c, "fill");
        if (strokeWidth) ctx(c, "stroke");
      } else if (s instanceof Text) {
        set(c, "font", StringFn(s.fontSize) + "px Arial");
        set(c, "textBaseline", "top");
        const lines = call(split, s.text, "\n");
        const metrics = ctx(c, "measureText", "M");
        const lineHeight = 1.5 * (call(metricAscent, metrics) + call(metricDescent, metrics));
        let width = 0;
        for (let i = 0; i < lines.length; i += 1) width = max(width, call(metricWidth, ctx(c, "measureText", lines[i])));
        ctx(c, "scale", s.scaleX, s.scaleY);
        const x = s.left / s.scaleX;
        const y = s.top / s.scaleY;
        rotate(c, s.angle, x, y);
        set(c, "fillStyle", "#ffffff");
        ctx(c, "fillRect", x, y, width + 5, lines.length * lineHeight + 5);
        set(c, "fillStyle", s.fill == null ? "#000" : s.fill);
        for (let i = 0; i < lines.length; i += 1) ctx(c, "fillText", lines[i], x, y + i * lineHeight);
      } else if (s instanceof Rectangle || s instanceof Ellipse) {
        rotate(c, s.angle, s.left, s.top);
        if (s instanceof Rectangle) {
          ctx(c, "fillRect", s.left, s.top, s.width, s.height);
          if (strokeWidth) ctx(c, "strokeRect", s.left, s.top, s.width, s.height);
        } else {
          ctx(c, "beginPath");
          ctx(c, "ellipse", s.left + s.rx, s.top + s.ry, s.rx, s.ry, 0, 0, 2 * pi);
          ctx(c, "closePath");
          ctx(c, "fill");
          if (strokeWidth) ctx(c, "stroke");
        }
      }
    } finally {
      ctx(c, "restore");
    }
  }
  function rotate(c, angle, x, y) {
    const radians = (angle == null ? 0 : angle) * pi / 180;
    if (radians && radians === radians) {
      ctx(c, "translate", x, y);
      ctx(c, "rotate", radians);
      ctx(c, "translate", -x, -y);
    }
  }

  let labelsOnly = false;
  let latestOptions;
  let keyInstalled = false;
  let watched = null;
  let watchedImage = null;
  let imageSize = null;
  let imageChanged = false;
  let waitingImages = [];
  let lastSize;
  let pending = false;
  let dprQuery = null;
  let dprListener = null;

  function readShapes(className) {
    const elements = call(queryAll, doc, "div." + className);
    const shapes = [];
    for (let i = 0; i < call(nodeListLength, elements); i += 1) {
      const element = elements[i];
      if (!(element instanceof isDiv)) continue;
      const kind = attr(element, "shape");
      if (kind !== "rect" && kind !== "ellipse" && kind !== "polygon" && kind !== "text") continue;
      function position(name) {
        const value = attr(element, name);
        return NumberFn(value) !== NumberFn(value) ? 0 : value === null ? 0 : float(value);
      }
      const angle = ((NumberFn(attr(element, "angle")) % 10000) / 10000) * 360;
      const fill = attr(element, "fill");
      const options = {
        left: position("left"), top: position("top"), angle: angle === angle ? angle : 0,
        fill: fill === null ? undefined : fill,
        occludeInactive: attr(element, "occludeinactive") === "1", ordinal: integer(attr(element, "ordinal")),
      };
      let shape;
      if (kind === "rect") {
        options.width = float(attr(element, "width"));
        options.height = float(attr(element, "height"));
        shape = new Rectangle(options);
      } else if (kind === "ellipse") {
        options.rx = float(attr(element, "rx"));
        options.ry = float(attr(element, "ry"));
        shape = new Ellipse(options);
      } else if (kind === "polygon") {
        const value = attr(element, "points");
        // Deliberately let absent points fail with TypeError before any hook.
        const items = value === "" ? null : call(split, value, " ");
        options.points = [];
        if (items === null) options.points[0] = { x: 0, y: 0 };
        else for (let j = 0; j < items.length; j += 1) {
          const parts = call(split, items[j], ",");
          options.points[j] = { x: parts[0], y: parts.length > 1 ? parts[1] : 0 };
        }
        shape = new Polygon(options);
      } else {
        options.text = attr(element, "text") || "";
        options.scaleX = options.scaleY = float(attr(element, "scale"));
        const font = attr(element, "font-size");
        options.fontSize = font ? float(font) : undefined;
        shape = new Text(options);
      }
      if (!labelsOnly || shape instanceof Text) shapes[shapes.length] = shape;
    }
    return shapes;
  }
  function properties(canvas) {
    const css = call(computedStyle, win, canvas);
    function value(name, fallback) { return call(cssGet, css, "--" + name) || fallback; }
    function border(name, color) {
      const pieces = call(split, value(name, ""), " ");
      const nonempty = [];
      for (let i = 0; i < pieces.length; i += 1) if (pieces[i] !== "") nonempty[nonempty.length] = pieces[i];
      const width = float(nonempty[0]);
      return { width: width === width ? width : 1, color: nonempty.length > 1 ? nonempty[1] : color };
    }
    return {
      activeShapeColor: value("active-shape-color", "#ff8e8e"),
      inActiveShapeColor: value("inactive-shape-color", "#ffeba2"),
      highlightShapeColor: value("highlight-shape-color", "#ff8e8e00"),
      activeBorder: border("active-shape-border", "#212121"),
      inActiveBorder: border("inactive-shape-border", "#212121"),
      highlightShapeBorder: border("highlight-shape-border", "#ff8e8e"),
    };
  }
  function draw(options) {
    const canvas = id("image-occlusion-canvas");
    const c = call(context2D, canvas, "2d");
    let data = {
      activeShapes: readShapes("cloze"), inactiveShapes: readShapes("cloze-inactive"),
      highlightShapes: readShapes("cloze-highlight"), properties: properties(canvas),
    };
    const will = options == null ? undefined : options.onWillDrawShapes;
    if (will != null) data = call(will, undefined, data, c) || data;
    const activeShapes = data.activeShapes;
    const inactiveShapes = data.inactiveShapes;
    const highlightShapes = data.highlightShapes;
    const p = data.properties;
    const size = { width: call(canvasWidth.get, canvas), height: call(canvasHeight.get, canvas) };
    function group(shapes, fill, border, inactive) {
      for (let i = 0; i < shapes.length; i += 1) {
        const s = shapes[i];
        if (inactive && !s.occludeInactive) continue;
        drawShape({ context: c, size, shape: s, fill: inactive && s.fill !== "#ffeba2" ? s.fill : fill,
          stroke: border.color, strokeWidth: border.width });
      }
    }
    group(activeShapes, p.activeShapeColor, p.activeBorder, false);
    group(inactiveShapes, p.inActiveShapeColor, p.inActiveBorder, true);
    group(highlightShapes, p.highlightShapeColor, p.highlightShapeBorder, false);
    const did = options == null ? undefined : options.onDidDrawShapes;
    if (did != null) call(did, undefined, {
      activeShapes, inactiveShapes, highlightShapes, properties: p,
    }, c);
  }
  function toggle(options) {
    labelsOnly = !labelsOnly;
    const canvas = id("image-occlusion-canvas");
    const c = call(context2D, canvas, "2d");
    ctx(c, "clearRect", 0, 0, call(canvasWidth.get, canvas), call(canvasHeight.get, canvas));
    draw(options);
  }
  function keydown(event) {
    const button = id("toggle");
    if (call(key, event) === "M" && button && call(cssGet, call(style, button), "display") !== "none") toggle(latestOptions);
  }
  function dimensions(canvas) {
    return [call(clientWidth, canvas), call(clientHeight, canvas), ratio()];
  }
  function sizeCanvas(canvas, img) {
    const extent = dimensions(canvas);
    const nw = call(naturalWidth, img);
    const nh = call(naturalHeight, img);
    const scale = min(extent[0] * extent[2] / nw, extent[1] * extent[2] / nh);
    let w = nw * scale;
    let h = nh * scale;
    if (max(w, h) > 4096) {
      const reduction = 4096 / max(w, h);
      w *= reduction;
      h *= reduction;
    }
    call(canvasWidth.set, canvas, floor(w));
    call(canvasHeight.set, canvas, floor(h));
    lastSize = extent;
  }
  function dropWatch() {
    call(disconnectSize, resizeObserver);
    call(disconnectDOM, removalObserver);
    if (watchedImage) call(remove, watchedImage, "load", imageLoaded);
    if (dprQuery) call(remove, dprQuery, "change", dprListener);
    dprQuery = dprListener = null;
    watchedImage = imageSize = null;
    imageChanged = false;
    watched = lastSize = null;
    if (waitingImages.length) call(observeDOM, removalObserver, doc, { childList: true, subtree: true });
  }
  function imageLoaded() {
    if (!watchedImage || !call(connected, watchedImage) || watchedImage !== image()) return;
    const nw = call(naturalWidth, watchedImage);
    const nh = call(naturalHeight, watchedImage);
    if (!nw || !nh || (imageSize && nw === imageSize[0] && nh === imageSize[1])) return;
    imageSize = [nw, nh];
    // A failed image has no height: restore the aspect before measuring the
    // backing, even when ResizeObserver has seen no client-size change yet.
    call(cssSet, call(style, watched), "aspect-ratio", StringFn(nw / nh));
    imageChanged = true;
    requestRedraw();
  }
  function armDPR() {
    if (dprQuery) call(remove, dprQuery, "change", dprListener);
    dprQuery = call(mediaQuery, win, "(resolution: " + StringFn(ratio()) + "dppx)");
    dprListener = function () { armDPR(); requestRedraw(); };
    call(add, dprQuery, "change", dprListener);
  }
  function requestRedraw() {
    if (!watched || pending) return;
    pending = true;
    call(frame, win, function () {
      pending = false;
      if (!watched) return;
      if (!call(connected, watched)) { dropWatch(); return; }
      const canvas = id("image-occlusion-canvas");
      const img = image();
      if (!canvas || !img) return;
      const extent = dimensions(canvas);
      if (!imageChanged && lastSize && extent[0] === lastSize[0] && extent[1] === lastSize[1] && extent[2] === lastSize[2]) return;
      imageChanged = false;
      // The detached promise intentionally lets C12 report redraw failures.
      call(then, call(resolve, PromiseCtor), function () { sizeCanvas(canvas, img); draw(latestOptions); });
    });
  }
  const resizeObserver = new ResizeCtor(requestRedraw);
  function forgetWaiting(entry) {
    call(remove, entry.img, "load", entry.ready);
    call(remove, entry.img, "error", entry.ready);
    const remaining = [];
    for (let i = 0; i < waitingImages.length; i += 1) {
      if (waitingImages[i] !== entry) remaining[remaining.length] = waitingImages[i];
    }
    waitingImages = remaining;
  }
  function pruneWaiting() {
    // Repeated calls for the same current image remain independent. Only
    // obsolete sides lose their initial load/error listeners and pending work.
    const entries = waitingImages;
    for (let i = 0; i < entries.length; i += 1) {
      const entry = entries[i];
      if (!call(connected, entry.img) || entry.img !== image()) {
        forgetWaiting(entry);
        entry.finish(undefined);
      }
    }
  }
  const removalObserver = new MutationCtor(function () {
    pruneWaiting();
    if (watched && (!call(connected, watched) || watchedImage !== image())) dropWatch();
    if (!watched && !waitingImages.length) call(disconnectDOM, removalObserver);
  });
  function watch(container, img) {
    // Keep this setup's measurement across cleanup: the observer's initial
    // notification must not add another draw for an already-loaded image.
    const extent = lastSize;
    dropWatch();
    lastSize = extent;
    watched = container;
    watchedImage = img;
    imageSize = [call(naturalWidth, img), call(naturalHeight, img)];
    call(add, img, "load", imageLoaded);
    call(observeSize, resizeObserver, container);
    call(observeDOM, removalObserver, doc, { childList: true, subtree: true });
    armDPR();
  }
  function setupStep(options) {
    const canvas = id("image-occlusion-canvas");
    if (!canvas) return;
    const img = image();
    const container = id("image-occlusion-container");
    if (!img) { call(innerText, container, "No image to show."); return; }
    call(cssSet, call(style, container), "aspect-ratio", StringFn(call(naturalWidth, img) / call(naturalHeight, img)));
    sizeCanvas(canvas, img);
    if (!keyInstalled) {
      call(add, win, "keydown", keydown);
      call(add, win, "resize", requestRedraw);
      keyInstalled = true;
    }
    const button = id("toggle");
    if (button) {
      if (call(query, doc, '[data-occludeinactive="1"]')) call(add, button, "click", function () { toggle(options); });
      else call(cssSet, call(style, button), "display", "none");
    }
    labelsOnly = false;
    draw(options);
    watch(container, img);
  }
  function setup(options) {
    latestOptions = options;
    // This outer promise only schedules work. Its completion is not a draw
    // barrier; setup/draw failures belong to the detached promise in the rAF.
    return new PromiseCtor(function (finish) {
      pruneWaiting();
      const img = image();
      function schedule() {
        call(frame, win, function () {
          call(then, call(resolve, PromiseCtor), function () { setupStep(options); });
        });
        finish(undefined);
      }
      if (img && !call(complete, img)) {
        const entry = { img, finish, ready: null };
        const ready = function () {
          forgetWaiting(entry);
          schedule();
        };
        entry.ready = ready;
        waitingImages[waitingImages.length] = entry;
        call(add, img, "load", ready);
        call(add, img, "error", ready);
        call(observeDOM, removalObserver, doc, { childList: true, subtree: true });
      } else call(then, call(resolve, PromiseCtor), schedule);
    });
  }

  win.anki.imageOcclusion = { setup, drawShape, Ellipse, Polygon, Rectangle, Shape, Text };
  win.anki.setupImageCloze = setup;
})();
