// Meguri's field runtime, authored from editor/field-html. It runs only in
// the host's isolated world; field markup is never interpolated into a script.
(() => {
    "use strict";
    const protocol = 1;
    const container = document.getElementById("fields");
    const blocks = new Set(("address article aside blockquote details dialog dd div dl dt " +
        "fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 header hgroup hr li main " +
        "nav ol p pre section table ul").split(" "));
    let loadId = 0, seq = 0, held = true, fields = [];
    const labels = Object.fromEntries(["html", "rich", "mode"].map(key =>
        [key, document.querySelector(`meta[name="editor-${key}"]`).content]));
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
        const current = selection(field);
        current.removeAllRanges(); current.addRange(value);
        field.caret = value.cloneRange();
    }
    function saveCaret(field) {
        if (field.mode === "html") field.sourceCaret = [field.source.selectionStart, field.source.selectionEnd];
        else field.caret = range(field);
    }
    function focus(field) {
        if (!field || field.collapsed || held) return;
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
        field.host.hidden = field.collapsed || field.mode !== "rich";
        field.source.hidden = field.collapsed || field.mode !== "html";
        field.label.setAttribute("aria-expanded", String(!field.collapsed));
        field.toggle.textContent = field.mode === "rich" ? labels.html : labels.rich;
        field.toggle.setAttribute("aria-pressed", String(field.mode === "html"));
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
        const available = fields.filter(value => !value.collapsed);
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
        input.addEventListener("focus", () => void emit("focus", {ord: field.ord, mode: field.mode}));
        input.addEventListener("blur", () => {
            saveCaret(field); field.lastKind = null; commit(field, "blur"); void emit("blur", {ord: field.ord});
        });
        input.addEventListener("beforeinput", event => {
            if (held) { event.preventDefault(); return; }
            if (["historyUndo", "historyRedo"].includes(event.inputType)) {
                event.preventDefault(); history(field, event.inputType === "historyRedo"); return;
            }
            if (field.mode === "rich" && ["insertParagraph", "insertLineBreak"].includes(event.inputType)) {
                event.preventDefault(); breakLine(field, event.inputType === "insertLineBreak"); return;
            }
            beforeEdit(field, event.inputType);
        });
        input.addEventListener("input", () => { if (!held) { dirty(field); historyState(field); } });
        input.addEventListener("keydown", event => {
            if (held) { event.preventDefault(); return; }
            if (event.key === "Tab") { event.preventDefault(); tab(field, event.shiftKey); }
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") event.preventDefault();
            if (event.metaKey && event.shiftKey && event.code === "KeyX") { event.preventDefault(); toggleMode(field); }
            if (event.metaKey && event.code === "KeyZ" && history(field, event.shiftKey)) event.preventDefault();
        });
        // U197 has not supplied the paste/drop pipeline. Refuse these rather
        // than let WebKit import files, remote resources or strip user HTML.
        input.addEventListener("paste", event => event.preventDefault());
        input.addEventListener("drop", event => event.preventDefault());
        input.addEventListener("dragover", event => event.preventDefault());
    }
    function makeField(value) {
        const section = document.createElement("section"), header = document.createElement("header");
        const label = document.createElement("button"), toggle = document.createElement("button");
        label.type = toggle.type = "button";
        label.textContent = value.name; label.id = `editor.field.${value.ord}.label`;
        toggle.id = `editor.field.${value.ord}.htmlToggle`;
        toggle.setAttribute("aria-label", `${labels.mode}: ${value.name}`);
        header.append(label, toggle);
        const boundary = document.createElement("div"), host = document.createElement("div");
        boundary.className = "fieldBoundary"; host.className = "richHost"; boundary.append(host);
        const root = host.attachShadow({mode: "closed"});
        const style = document.createElement("style");
        style.textContent = ":host { display:block; position:relative } meguri-field-content { display:block; min-height:2em; overflow-wrap:anywhere } " +
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
            collapsed: value.collapsed, rich, root, host: boundary, source, label, toggle, placeholder: placeholderLabel,
            timer: null, caret: null, sourceCaret: null, undo: [], redo: [], lastKind: null, lastEdit: 0};
        label.addEventListener("click", () => collapse(field, !field.collapsed));
        toggle.addEventListener("click", () => toggleMode(field));
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
        keys(field, ["ord", "name", "html", "baselineHtml", "description", "fontFamily", "fontSize", "rtl", "collapsed", "mode", "clozeField"],
            ["ord", "name", "html", "description", "fontFamily", "fontSize", "rtl", "collapsed", "mode", "clozeField"]);
        if (!Number.isSafeInteger(field.ord) || field.ord < 0 ||
            ["name", "html", "description", "fontFamily"].some(key => typeof field[key] !== "string") ||
            (Object.hasOwn(field, "baselineHtml") && typeof field.baselineHtml !== "string") ||
            !Number.isFinite(field.fontSize) || field.fontSize <= 0 ||
            ["rtl", "collapsed", "clozeField"].some(key => typeof field[key] !== "boolean") ||
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
            loadId = message.loadId; seq = 0;
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
            if (held) container.inert = true;
            return envelope("flush", {commits});
        }
        case "release":
            keys(message, ["type", "loadId"]); held = false; container.inert = false; return {loadId};
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
