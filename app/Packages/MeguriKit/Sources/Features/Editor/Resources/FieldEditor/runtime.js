// Meguri's field runtime, authored from editor/field-html. It runs only in
// the host's isolated world; field markup is never interpolated into a script.
(() => {
    "use strict";
    const protocol = 1;
    const container = document.getElementById("fields");
    const blocks = new Set(("address article aside blockquote details dialog dd div dl dt " +
        "fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 header hgroup hr li main " +
        "nav ol p pre section table ul").split(" "));
    let loadId = 0, seq = 0, held = true, fields = [], current = null, pageMode = "add";
    const bridge = window.webkit.messageHandlers.meguriEditor;
    const envelope = (type, payload = {}) => ({type, loadId, seq: ++seq, ...payload});
    const emit = (type, payload) => bridge.postMessage(envelope(type, payload));
    const parse = html => {
        // Template contents are inert; CSP additionally refuses page scripts,
        // handler attributes, frames, links and all resource requests.
        const template = document.createElement("template");
        template.innerHTML = html;
        return template.content;
    };
    const inline = node => !Array.from(node.querySelectorAll("*")).some(element =>
        blocks.has(element.localName) || (element.hasAttribute("block") && element.getAttribute("block") !== "false"));
    const loadParse = html => {
        const fragment = parse(html);
        if (inline(fragment)) fragment.append(document.createElement("br"));
        return fragment;
    };
    function serialized(node, rich) {
        const copy = node.cloneNode(true);
        if (rich && inline(copy) && copy.lastChild?.nodeType === Node.ELEMENT_NODE && copy.lastChild.localName === "br") {
            copy.lastChild.remove();
        }
        for (const element of copy.querySelectorAll("[data-meguri-editor]")) element.removeAttribute("data-meguri-editor");
        const template = document.createElement("template");
        template.content.append(...Array.from(copy.childNodes));
        return template.innerHTML;
    }
    const content = field => field.mode === "html" ? field.source.value : field.rich.innerHTML;
    const html12 = field => field.mode === "html" ? serialized(parse(field.source.value), false) : serialized(field.rich, true);
    const baseline12 = field => serialized(loadParse(field.baseline), true);
    function unchanged(field) {
        if (!field.changed) return true;
        return field.mode === "html" ? field.source.value === field.baseline : html12(field) === baseline12(field);
    }
    const active = field => field.mode === "html" ? field.source : field.rich;

    function selection(field) {
        if (field.root.getSelection) return field.root.getSelection();
        return document.getSelection();
    }
    function range(field) {
        const current = selection(field);
        if (current?.getComposedRanges) {
            const value = current.getComposedRanges({shadowRoots: [field.root]})[0];
            if (value && field.rich.contains(value.startContainer) && field.rich.contains(value.endContainer)) {
                const result = document.createRange();
                result.setStart(value.startContainer, value.startOffset);
                result.setEnd(value.endContainer, value.endOffset);
                return result;
            }
        }
        if (current?.rangeCount) {
            const result = current.getRangeAt(0);
            if (field.rich.contains(result.startContainer) && field.rich.contains(result.endContainer)) return result.cloneRange();
        }
        const result = document.createRange();
        result.selectNodeContents(field.rich); result.collapse(false);
        if (inline(field.rich) && field.rich.lastChild?.localName === "br") {
            result.setStartBefore(field.rich.lastChild); result.collapse(true);
        }
        return result;
    }
    function selectRange(field, value) {
        // Replace the selection in one step. Clearing it first leaves a
        // focused field with no caret when WebKit refuses a range inside the
        // field's shadow tree, and typing then goes nowhere.
        selection(field).setBaseAndExtent(value.startContainer, value.startOffset, value.endContainer, value.endOffset);
        field.caret = value.cloneRange();
    }
    function saveCaret(field) {
        if (field.mode === "html") field.sourceCaret = [field.source.selectionStart, field.source.selectionEnd];
        else field.caret = range(field);
    }
    function focus(field) {
        if (!field || field.collapsed || field.hidden || held) return;
        active(field).focus();
        if (field.mode === "html") {
            const [start, end] = field.sourceCaret ?? [field.source.value.length, field.source.value.length];
            field.source.setSelectionRange(start, end);
        } else {
            try { selectRange(field, field.caret ?? range(field)); }
            catch { field.caret = null; selectRange(field, range(field)); }
        }
    }
    function historyState(field) {
        void emit("historyState", {ord: field.ord, canUndo: field.undo.length > 0, canRedo: field.redo.length > 0});
    }
    function dirty(field) {
        field.changed = true;
        if (!field.pending) void emit("dirty", {ord: field.ord});
        field.pending = true;
        clearTimeout(field.timer);
        field.timer = setTimeout(() => commit(field, "idle"), 600);
        placeholder(field);
    }
    function commitValue(field) {
        if (!field.pending) return null;
        clearTimeout(field.timer); field.timer = null; field.pending = false;
        const same = unchanged(field), html = html12(field);
        field.changed = !same;
        return {ord: field.ord, html, unchanged: same};
    }
    function commit(field, reason) {
        const value = commitValue(field);
        if (value) void emit("commit", {...value, reason}).catch(() => {
            // Retain unacknowledged content for the next explicit flush.
            // A bridge failure never rebases or clears the live field.
            field.pending = true;
        });
    }
    function beforeEdit(field, kind) {
        const now = performance.now();
        if (kind !== "insertText" || field.lastKind !== kind || now - field.lastEdit >= 600) field.undo.push(content(field));
        field.redo = []; field.lastKind = kind; field.lastEdit = now;
    }
    function restore(field, value) {
        if (field.mode === "html") field.source.value = value;
        else field.rich.replaceChildren(parse(value));
        field.caret = null; field.sourceCaret = null;
        dirty(field); focus(field); historyState(field);
    }
    function history(field, redo) {
        const from = redo ? field.redo : field.undo, to = redo ? field.undo : field.redo;
        if (held || !from.length) return false;
        to.push(content(field)); field.lastKind = null;
        restore(field, from.pop()); return true;
    }
    function toggleMode(field) {
        if (held) return;
        commit(field, "modeSwitch");
        saveCaret(field);
        if (field.mode === "rich") {
            field.source.value = unchanged(field) ? field.baseline : html12(field);
            field.mode = "html";
        } else {
            field.rich.replaceChildren(loadParse(field.source.value));
            field.mode = "rich";
        }
        field.undo = []; field.redo = []; field.lastKind = null;
        field.caret = null; field.sourceCaret = null;
        show(field); focus(field);
        void emit("modeChanged", {ord: field.ord, mode: field.mode});
        historyState(field);
    }
    function collapse(field, value) {
        if (held) return;
        if (value) commit(field, "collapse");
        field.collapsed = value; show(field);
        void emit("collapsedChanged", {ord: field.ord, collapsed: value});
        if (!value) focus(field);
    }
    function show(field) {
        // Browse's inspector leaves empty fields out (V12): the whole section goes.
        field.section.hidden = field.hidden;
        field.host.hidden = field.collapsed || field.mode !== "rich";
        field.source.hidden = field.collapsed || field.mode !== "html";
        field.label.setAttribute("aria-expanded", String(!field.collapsed));
        placeholder(field);
    }
    function placeholder(field) {
        const children = field.rich.childNodes;
        const br = node => node?.nodeType === Node.ELEMENT_NODE && node.localName === "br";
        const singleBreak = children.length === 1 && br(children[0]);
        const emptyDiv = children.length === 1 && children[0].localName === "div" &&
            children[0].childNodes.length === 1 && br(children[0].firstChild);
        const empty = field.changed ? children.length === 0 || singleBreak || emptyDiv : field.baseline === "";
        field.placeholder.hidden = field.collapsed || field.mode !== "rich" || !empty;
    }
    function tab(field, backwards) {
        saveCaret(field);
        const available = fields.filter(value => !value.collapsed && !value.hidden);
        const next = available[available.indexOf(field) + (backwards ? -1 : 1)];
        if (next) focus(next);
        else { active(field).blur(); void emit("focusLeave", {direction: backwards ? "previous" : "next"}); }
    }
    function breakLine(field, lineBreak) {
        beforeEdit(field, lineBreak ? "insertLineBreak" : "insertParagraph");
        const caret = range(field);
        caret.deleteContents();
        if (lineBreak) {
            const br = document.createElement("br");
            caret.insertNode(br); caret.setStartAfter(br); caret.collapse(true);
            // At the end of inline content a second BR supplies the caret's
            // last line. H3 drops this final BR rather than the user's break.
            if (inline(field.rich) && field.rich.lastChild === br) field.rich.append(document.createElement("br"));
        } else {
            let block = caret.startContainer.nodeType === Node.ELEMENT_NODE ? caret.startContainer : caret.startContainer.parentElement;
            while (block && block !== field.rich && !blocks.has(block.localName)) block = block.parentElement;
            if (block?.localName === "li") {
                const list = block.parentElement;
                if (!block.textContent && !block.querySelector("img") && block === list.lastElementChild) {
                    const next = document.createElement("div"); next.append(document.createElement("br"));
                    list.after(next); block.remove(); if (!list.children.length) list.remove();
                    caret.selectNodeContents(next); caret.collapse(true);
                } else {
                    const tail = document.createRange(); tail.setStart(caret.startContainer, caret.startOffset); tail.setEnd(block, block.childNodes.length);
                    const next = document.createElement("li"); next.append(tail.extractContents());
                    if (!next.childNodes.length) next.append(document.createElement("br"));
                    block.after(next); caret.selectNodeContents(next); caret.collapse(true);
                }
            } else {
                const owner = block && block !== field.rich ? block : field.rich;
                const tail = document.createRange(); tail.setStart(caret.startContainer, caret.startOffset); tail.setEnd(owner, owner.childNodes.length);
                const next = document.createElement("div"); next.append(tail.extractContents());
                if (!next.childNodes.length) next.append(document.createElement("br"));
                if (owner === field.rich) owner.append(next); else owner.after(next);
                caret.selectNodeContents(next); caret.collapse(true);
            }
        }
        selectRange(field, caret); dirty(field); historyState(field);
    }
    function wire(field, input) {
        input.addEventListener("focus", () => { current = field; void emit("focus", {ord: field.ord, mode: field.mode}); });
        input.addEventListener("blur", () => {
            if (current === field) current = null;
            saveCaret(field); field.lastKind = null; commit(field, "blur"); void emit("blur", {ord: field.ord});
        });
        input.addEventListener("beforeinput", event => {
            // Composition input cannot be cancelled: remember the field, so
            // what it now shows is committed once the hold ends.
            if (held) { event.preventDefault(); if (!event.cancelable) field.heldInput = true; return; }
            if (["historyUndo", "historyRedo"].includes(event.inputType)) {
                event.preventDefault(); history(field, event.inputType === "historyRedo"); return;
            }
            if (field.mode === "rich" && ["insertParagraph", "insertLineBreak"].includes(event.inputType)) {
                event.preventDefault(); breakLine(field, event.inputType === "insertLineBreak"); return;
            }
            beforeEdit(field, event.inputType);
        });
        input.addEventListener("input", () => {
            if (held) { field.heldInput = true; return; }
            dirty(field); historyState(field);
        });
        input.addEventListener("keydown", event => {
            if (held) { event.preventDefault(); return; }
            if (event.key === "Tab") { event.preventDefault(); tab(field, event.shiftKey); }
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") event.preventDefault();
            if (event.metaKey && event.shiftKey && event.code === "KeyX") { event.preventDefault(); toggleMode(field); }
            // editor/formatting F14: in Browse the Format menu owns ⌘B, ⌘I and ⌘U;
            // WebKit's own handling of them never changes field HTML.
            if (pageMode === "browse" && event.metaKey && !event.altKey && !event.ctrlKey &&
                ["KeyB", "KeyI", "KeyU"].includes(event.code)) event.preventDefault();
            if (event.metaKey && event.code === "KeyZ" && history(field, event.shiftKey)) event.preventDefault();
        });
        // U197 has not supplied the paste/drop pipeline. Refuse these rather
        // than let WebKit import files, remote resources or strip user HTML.
        input.addEventListener("paste", event => event.preventDefault());
        input.addEventListener("drop", event => event.preventDefault());
        input.addEventListener("dragover", event => event.preventDefault());
    }
    // editor/formatting F3 and F4: inline formats as the runtime's own DOM
    // transforms. Applying wraps each run of selected inline content (never
    // across a block) after removing the format inside it, and merges with
    // a neighbour of the same format; removing takes the format off the
    // selection and keeps it on the content around it.
    const formats = {
        bold: {tag: "b", tags: ["b", "strong"], prop: "font-weight",
            styled: style => style.fontWeight === "bold" || Number(style.fontWeight) >= 700},
        italic: {tag: "i", tags: ["i", "em"], prop: "font-style",
            styled: style => style.fontStyle === "italic" || style.fontStyle === "oblique"},
        underline: {tag: "u", tags: ["u"], prop: null, styled: () => false},
        superscript: {tag: "sup", tags: ["sup"], prop: "vertical-align", styled: style => style.verticalAlign === "super", excludes: "subscript"},
        subscript: {tag: "sub", tags: ["sub"], prop: "vertical-align", styled: style => style.verticalAlign === "sub", excludes: "superscript"},
    };
    const voids = new Set("area base br col embed hr img input link meta source track wbr".split(" "));
    const counts = (node, kind) => node?.nodeType === Node.ELEMENT_NODE &&
        (kind.tags.includes(node.localName) || (node.hasAttribute("style") && kind.styled(node.style)));
    function withinFormat(field, node, kind) {
        for (let element = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
            element && element !== field.rich; element = element.parentElement) if (counts(element, kind)) return element;
        return null;
    }
    function unwrap(element) { element.replaceWith(...Array.from(element.childNodes)); }
    function stripStyle(element, kind) {
        if (!kind.prop || !element.hasAttribute("style")) return;
        element.style.removeProperty(kind.prop);
        if (!element.getAttribute("style").trim()) element.removeAttribute("style");
        if (element.localName === "span" && element.attributes.length === 0) unwrap(element);
    }
    // F3: an own-tag element is unwrapped, or with attributes becomes a span
    // that keeps them; otherwise the format's property goes from its style.
    function strip(element, kind) {
        if (kind.tags.includes(element.localName)) {
            if (element.attributes.length === 0) { unwrap(element); return; }
            const span = document.createElement("span");
            for (const attribute of Array.from(element.attributes)) span.setAttribute(attribute.name, attribute.value);
            span.append(...Array.from(element.childNodes)); element.replaceWith(span);
            stripStyle(span, kind); return;
        }
        stripStyle(element, kind);
    }
    function stripInside(root, kind) {
        for (const element of Array.from(root.querySelectorAll("*")).reverse()) if (counts(element, kind)) strip(element, kind);
    }
    const textNodes = root => {
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const nodes = []; while (walker.nextNode()) if (walker.currentNode.data.length) nodes.push(walker.currentNode);
        return nodes;
    };
    const isVoid = node => node.nodeType === Node.ELEMENT_NODE && voids.has(node.localName);
    // F4 step 3: one wrapper per run of adjacent inline content with text;
    // void elements at a run's edges stay outside it.
    function wrapRuns(parent, tag) {
        let run = [];
        const close = () => {
            while (run.length && isVoid(run[0])) run.shift();
            while (run.length && isVoid(run[run.length - 1])) run.pop();
            if (run.some(node => node.textContent.length > 0)) {
                const wrapper = document.createElement(tag);
                run[0].before(wrapper); wrapper.append(...run);
            }
            run = [];
        };
        for (const node of Array.from(parent.childNodes)) {
            if (node.nodeType === Node.ELEMENT_NODE && (blocks.has(node.localName) ||
                (node.hasAttribute("block") && node.getAttribute("block") !== "false"))) { close(); wrapRuns(node, tag); }
            else run.push(node);
        }
        close();
    }
    // F4 step 5: a wrapper merges with a neighbour of the same format,
    // past empty text nodes.
    function mergeNeighbours(root, tag) {
        for (const wrapper of Array.from(root.querySelectorAll(tag))) {
            if (!wrapper.isConnected || wrapper.attributes.length) continue;
            let previous = wrapper.previousSibling;
            while (previous?.nodeType === Node.TEXT_NODE && previous.data === "") previous = previous.previousSibling;
            if (previous?.nodeType === Node.ELEMENT_NODE && previous.localName === tag && previous.attributes.length === 0) {
                previous.append(...Array.from(wrapper.childNodes)); wrapper.remove(); continue;
            }
            let next = wrapper.nextSibling;
            while (next?.nodeType === Node.TEXT_NODE && next.data === "") next = next.nextSibling;
            if (next?.nodeType === Node.ELEMENT_NODE && next.localName === tag && next.attributes.length === 0) {
                wrapper.append(...Array.from(next.childNodes)); next.remove();
            }
        }
    }
    function selectTexts(field, texts) {
        if (!texts.length) return;
        const value = document.createRange();
        value.setStart(texts[0], 0); value.setEnd(texts[texts.length - 1], texts[texts.length - 1].length);
        selectRange(field, value);
    }
    function format(field, command) {
        const kind = formats[command];
        const selected = range(field);
        if (held || selected.collapsed) return false;
        beforeEdit(field, "format");
        const before = field.rich.innerHTML;
        const has = withinFormat(field, selected.startContainer, kind) || withinFormat(field, selected.endContainer, kind);
        const content = selected.extractContents();
        if (kind.excludes) stripInside(content, formats[kind.excludes]);
        stripInside(content, kind);
        const texts = textNodes(content);
        if (has) {
            // The outermost format element around the point is split there,
            // and the selection goes between its two halves (F4 Removing).
            let outer = null;
            for (let element = selected.startContainer.nodeType === Node.ELEMENT_NODE ? selected.startContainer : selected.startContainer.parentElement;
                element && element !== field.rich; element = element.parentElement) if (counts(element, kind)) outer = element;
            if (outer) {
                let middle = content;
                for (let element = selected.startContainer.nodeType === Node.ELEMENT_NODE ? selected.startContainer : selected.startContainer.parentElement;
                    element && element !== outer; element = element.parentElement) {
                    if (counts(element, kind)) continue;
                    const copy = element.cloneNode(false); copy.append(middle); middle = copy;
                }
                const kept = outer.cloneNode(false); kept.append(middle);
                const tail = document.createRange();
                tail.setStart(selected.startContainer, selected.startOffset); tail.setEndAfter(outer);
                const after = tail.extractContents();
                const halves = [outer, ...Array.from(after.childNodes)];
                outer.after(kept, after);
                strip(kept, kind);
                // A half left with nothing in it goes.
                for (const half of halves) {
                    if (half.nodeType === Node.ELEMENT_NODE && !half.textContent && !half.querySelector("img, br")) half.remove();
                }
            } else selected.insertNode(content);
        } else {
            wrapRuns(content, kind.tag);
            selected.insertNode(content);
            mergeNeighbours(field.rich, kind.tag);
        }
        field.rich.normalize();
        selectTexts(field, texts.filter(node => node.isConnected));
        if (field.rich.innerHTML === before) { field.undo.pop(); return false; }
        dirty(field); historyState(field);
        return true;
    }
    // editor/cloze-wrap W4: the selection's serialization, its white space
    // ends kept outside, wrapped in the prefix and suffix as one input.
    const edgeSpace = "(?:[\\t-\\r \\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000\\ufeff]|&nbsp;|&#160;)*";
    function wrap(field, prefix, suffix) {
        const selected = range(field);
        beforeEdit(field, "insertCloze");
        if (selected.collapsed) {
            const text = document.createTextNode(prefix + suffix);
            selected.insertNode(text);
            const caret = document.createRange(); caret.setStart(text, prefix.length); caret.collapse(true);
            selectRange(field, caret);
        } else {
            const template = document.createElement("template");
            template.content.append(selected.cloneContents());
            const serialized = template.innerHTML;
            const lead = serialized.match(new RegExp("^" + edgeSpace))[0];
            let middle = serialized.slice(lead.length), trail = "";
            if (middle.length) { trail = middle.match(new RegExp(edgeSpace + "$"))[0]; middle = middle.slice(0, middle.length - trail.length); }
            const replacement = lead + prefix + middle + suffix + trail;
            selected.deleteContents();
            const fragment = parse(replacement);
            const last = fragment.lastChild;
            selected.insertNode(fragment);
            if (last) { const caret = document.createRange(); caret.setStartAfter(last); caret.collapse(true); selectRange(field, caret); }
        }
        dirty(field); historyState(field);
    }
    function makeField(value) {
        const section = document.createElement("section"), header = document.createElement("header");
        // N17.1: the name is the section's header; it still collapses the field (F3).
        // The one mode control is native, in the keyboard bar or the Mac bottom bar.
        const label = document.createElement("button");
        label.type = "button"; label.className = "label";
        label.textContent = value.name; label.id = `editor.field.${value.ord}.label`;
        header.append(label);
        const boundary = document.createElement("div"), host = document.createElement("div");
        boundary.className = "fieldBoundary"; host.className = "richHost"; boundary.append(host);
        const root = host.attachShadow({mode: "closed"});
        const style = document.createElement("style");
        style.textContent = ":host { display:block; position:relative } meguri-field-content { display:block; min-height:1.3em; outline:none; overflow-wrap:anywhere } " +
            ".placeholder { position:absolute; inset-block-start:0; inset-inline-start:0; max-width:100%; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; pointer-events:none; color:GrayText } [hidden] { display:none !important }";
        const rich = document.createElement("meguri-field-content");
        rich.contentEditable = "true"; rich.setAttribute("role", "textbox"); rich.setAttribute("aria-multiline", "true");
        rich.setAttribute("aria-label", value.name); rich.id = `editor.field.${value.ord}`;
        const placeholderLabel = document.createElement("span");
        placeholderLabel.className = "placeholder"; placeholderLabel.textContent = value.description;
        placeholderLabel.setAttribute("aria-hidden", "true");
        // CSS properties are assigned as data. A field font cannot inject CSS.
        host.style.fontFamily = value.fontFamily; host.style.fontSize = `${value.fontSize}px`;
        host.style.direction = value.rtl ? "rtl" : "ltr";
        rich.replaceChildren(loadParse(value.html)); root.append(style, rich, placeholderLabel);
        const source = document.createElement("textarea"); source.id = `editor.field.${value.ord}`;
        source.setAttribute("aria-label", value.name); source.value = value.html;
        source.spellcheck = false; source.autocorrect = "off"; source.autocapitalize = "off";
        source.setAttribute("autocorrect", "off"); source.setAttribute("autocapitalize", "off");
        source.setAttribute("writingsuggestions", "false");
        section.append(header, boundary, source); container.append(section);
        const field = {ord: value.ord, baseline: value.baselineHtml ?? value.html,
            changed: Object.hasOwn(value, "baselineHtml"), pending: false, mode: value.mode,
            collapsed: value.collapsed, hidden: value.hidden === true, section,
            rich, root, host: boundary, source, label, placeholder: placeholderLabel,
            timer: null, caret: null, sourceCaret: null, undo: [], redo: [], lastKind: null, lastEdit: 0};
        label.addEventListener("click", () => collapse(field, !field.collapsed));
        // The box's own padding belongs to the field: a tap there gives it
        // focus. A tap in the text keeps the caret WebKit placed.
        boundary.addEventListener("click", event => {
            if (event.target === boundary && !held && !field.collapsed) active(field).focus();
        });
        wire(field, rich); wire(field, source); show(field);
        return field;
    }
    const keys = (object, allowed, required = allowed) => {
        if (!object || typeof object !== "object" || Array.isArray(object) ||
            Object.keys(object).some(key => !allowed.includes(key)) || required.some(key => !Object.hasOwn(object, key))) {
            throw new Error("Invalid editor payload");
        }
    };
    function validateField(field) {
        keys(field, ["ord", "name", "html", "baselineHtml", "description", "fontFamily", "fontSize", "rtl", "collapsed", "mode", "clozeField", "hidden"],
            ["ord", "name", "html", "description", "fontFamily", "fontSize", "rtl", "collapsed", "mode", "clozeField"]);
        if (!Number.isSafeInteger(field.ord) || field.ord < 0 ||
            ["name", "html", "description", "fontFamily"].some(key => typeof field[key] !== "string") ||
            (Object.hasOwn(field, "baselineHtml") && typeof field.baselineHtml !== "string") ||
            !Number.isFinite(field.fontSize) || field.fontSize <= 0 ||
            ["rtl", "collapsed", "clozeField"].some(key => typeof field[key] !== "boolean") ||
            (Object.hasOwn(field, "hidden") && typeof field.hidden !== "boolean") ||
            !["rich", "html"].includes(field.mode)) throw new Error("Invalid field description");
    }
    async function receive(message) {
        if (message.type === "setTheme") {
            keys(message, ["type", "theme"]); keys(message.theme, []); return {};
        }
        if (message.type === "openNote") {
            keys(message, ["type", "protocol", "loadId", "mode", "fields", "focus", "theme", "options"]);
            keys(message.options, ["closeHTMLTags", "shrinkImages"]);
            if (message.protocol !== protocol || !["add", "edit", "browse", "content"].includes(message.mode) ||
                !Number.isSafeInteger(message.loadId) || message.loadId <= loadId || !Array.isArray(message.fields) ||
                message.fields.length === 0 || ![null, ...message.fields.map(value => value.ord)].includes(message.focus) ||
                Object.keys(message.theme).length !== 0 ||
                typeof message.options.closeHTMLTags !== "boolean" || typeof message.options.shrinkImages !== "boolean") throw new Error("Invalid openNote");
            message.fields.forEach(validateField);
            if (message.fields.some((value, index) => index > 0 && value.ord <= message.fields[index - 1].ord)) throw new Error("Invalid field order");
            for (const field of fields) clearTimeout(field.timer);
            loadId = message.loadId; seq = 0; current = null; pageMode = message.mode;
            held = true; container.replaceChildren(); fields = message.fields.map(makeField);
            held = false; container.inert = false;
            focus(fields.find(field => field.ord === message.focus));
            await emit("loaded");
            return {loadId};
        }
        if (message.loadId !== loadId) throw new Error("Stale editor load");
        const field = fields.find(value => value.ord === message.ord);
        switch (message.type) {
        case "flush": {
            keys(message, ["type", "loadId", "hold"]);
            if (typeof message.hold !== "boolean") throw new Error("Invalid hold");
            if (message.hold) held = true;
            const commits = fields.map(commitValue).filter(Boolean);
            // A hold refuses input in each handler. The fields keep their focus,
            // so on touch the keyboard stays up across an add (N17.1).
            return envelope("flush", {commits});
        }
        case "release":
            keys(message, ["type", "loadId"]); held = false; container.inert = false;
            for (const changed of fields.filter(value => value.heldInput)) {
                changed.heldInput = false; dirty(changed); historyState(changed);
            }
            return {loadId};
        case "setFocus":
            keys(message, ["type", "loadId", "ord"]); if (!field) throw new Error("Unknown field"); focus(field); return {loadId};
        case "setFieldState":
            keys(message, ["type", "loadId", "ord", "mode", "collapsed"], ["type", "loadId", "ord"]);
            if (!field || held) throw new Error("Field unavailable");
            if (!Object.hasOwn(message, "mode") && !Object.hasOwn(message, "collapsed")) throw new Error("Missing field state");
            if (Object.hasOwn(message, "mode") && !["rich", "html"].includes(message.mode)) throw new Error("Invalid mode");
            if (Object.hasOwn(message, "collapsed") && typeof message.collapsed !== "boolean") throw new Error("Invalid collapsed state");
            if (Object.hasOwn(message, "mode")) {
                if (field.mode !== message.mode) toggleMode(field);
            }
            if (Object.hasOwn(message, "collapsed")) {
                if (field.collapsed !== message.collapsed) collapse(field, message.collapsed);
            }
            return {loadId};
        case "setField":
            keys(message, ["type", "loadId", "ord", "html"]);
            if (!field || typeof message.html !== "string") throw new Error("Invalid field");
            clearTimeout(field.timer); field.pending = false; field.changed = false; field.baseline = message.html;
            field.rich.replaceChildren(loadParse(message.html)); field.source.value = message.html;
            field.undo = []; field.redo = []; field.caret = null; field.sourceCaret = null; field.lastKind = null;
            placeholder(field); historyState(field); return {loadId};
        case "setMarks":
            keys(message, ["type", "loadId", "duplicate", "hint"]);
            if (typeof message.duplicate !== "boolean" || !(message.hint === null || typeof message.hint === "string")) throw new Error("Invalid marks");
            if (fields[0]) {
                fields[0].rich.setAttribute("aria-invalid", String(message.duplicate));
                fields[0].source.setAttribute("aria-invalid", String(message.duplicate));
            }
            document.getElementById("hint").textContent = message.hint ?? "";
            return {loadId};
        case "setHidden": {
            keys(message, ["type", "loadId", "ords"]);
            if (!Array.isArray(message.ords) || message.ords.some(ord => !Number.isSafeInteger(ord))) throw new Error("Invalid ords");
            for (const value of fields) {
                const hidden = message.ords.includes(value.ord);
                if (hidden && current === value) active(value).blur();
                value.hidden = hidden; show(value);
            }
            return {loadId};
        }
        case "format": {
            keys(message, ["type", "loadId", "command"]);
            if (!Object.hasOwn(formats, message.command)) throw new Error("Unknown format");
            if (held || !current || current.mode !== "rich") return {applied: false};
            return {applied: format(current, message.command)};
        }
        case "wrap": {
            keys(message, ["type", "loadId", "prefix", "suffix"]);
            if (typeof message.prefix !== "string" || typeof message.suffix !== "string") throw new Error("Invalid wrap");
            if (held || !current || current.mode !== "rich") return {applied: false};
            wrap(current, message.prefix, message.suffix);
            return {applied: true};
        }
        case "undo": case "redo":
            keys(message, ["type", "loadId", "ord"]); if (!field) throw new Error("Unknown field");
            return {applied: history(field, message.type === "redo")};
        default: throw new Error("Unsupported editor command");
        }
    }
    // This object exists only in the editor's WKContentWorld. No DOM event
    // or page-world function forwards commands into it.
    Object.defineProperty(globalThis, "meguriEditorRuntime", {value: Object.freeze({receive: async message => {
        try { return await receive(message); }
        catch (error) { console.warn("Rejected editor command"); throw error; }
    }}), configurable: false});
    document.addEventListener("click", event => {
        if (event.composedPath().some(node => node.localName === "a")) event.preventDefault();
    }, true);
    document.addEventListener("submit", event => event.preventDefault(), true);
    new ResizeObserver(() => {
        if (loadId) void emit("contentHeight", {height: document.documentElement.scrollHeight});
    }).observe(document.body);
    void bridge.postMessage({type: "ready", protocol});
})();
