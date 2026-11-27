// Meguri's field runtime, authored from editor/field-html. It runs only in
// the host's isolated world; field markup is never interpolated into a script.
(() => {
    "use strict";
    const protocol = 1;
    const container = document.getElementById("fields");
    const blocks = new Set(("address article aside blockquote details dialog dd div dl dt " +
        "fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 header hgroup hr li main " +
        "nav ol p pre section table ul").split(" "));
    let loadId = 0, seq = 0, held = true, fields = [], current = null, browsing = false, media = false, pictureID = 0;
    let occlusionForm = null, formFrame = 0, lastFormLayout = "";
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
    const isBlock = element => blocks.has(element.localName) ||
        (element.hasAttribute("block") && element.getAttribute("block") !== "false");
    const inline = node => !Array.from(node.querySelectorAll("*")).some(isBlock);
    // Browse's inspector and Edit Note show a sound reference as a chip that
    // plays on a click; the chip stands for its reference's exact text, which is what
    // serialization writes back, so the field's bytes never change by it.
    const soundReference = /\[sound:([^\]]+)\]/g;
    const middle = name => name.length <= 28 ? name : `${name.slice(0, 13)}…${name.slice(-12)}`;
    function chips(root) {
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const texts = []; while (walker.nextNode()) texts.push(walker.currentNode);
        for (const text of texts) {
            if (text.parentElement?.closest?.("[data-meguri-chip]")) continue;
            const value = text.data;
            soundReference.lastIndex = 0;
            if (!soundReference.test(value)) continue;
            soundReference.lastIndex = 0;
            const fragment = document.createDocumentFragment();
            let at = 0, match;
            while ((match = soundReference.exec(value))) {
                if (match.index > at) fragment.append(document.createTextNode(value.slice(at, match.index)));
                const chip = document.createElement("span");
                chip.setAttribute("data-meguri-chip", "sound");
                chip.setAttribute("data-meguri-text", match[0]);
                chip.setAttribute("data-meguri-name", match[1]);
                chip.setAttribute("contenteditable", "false");
                chip.setAttribute("role", "button");
                chip.setAttribute("aria-label", match[1]);
                chip.title = match[1];
                chip.textContent = middle(match[1]);
                fragment.append(chip);
                at = match.index + match[0].length;
            }
            if (at < value.length) fragment.append(document.createTextNode(value.slice(at)));
            text.replaceWith(fragment);
        }
        return root;
    }
    const loadParse = html => {
        const fragment = parse(html);
        if (media) chips(fragment);
        if (inline(fragment)) fragment.append(document.createElement("br"));
        return fragment;
    };
    function serialized(node, rich) {
        const copy = node.cloneNode(true);
        if (rich && inline(copy) && copy.lastChild?.nodeType === Node.ELEMENT_NODE && copy.lastChild.localName === "br") {
            copy.lastChild.remove();
        }
        for (const element of copy.querySelectorAll("[data-meguri-editor]")) element.removeAttribute("data-meguri-editor");
        // A chip is its reference's text again (and adjacent text joins it).
        for (const chip of copy.querySelectorAll("[data-meguri-chip]")) {
            chip.replaceWith(document.createTextNode(chip.getAttribute("data-meguri-text") ?? ""));
        }
        if (copy.normalize) copy.normalize();
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
    function validRange(field, value) {
        const validPoint = (node, offset) => node && field.rich.contains(node) &&
            Number.isInteger(offset) && offset >= 0 &&
            offset <= (node instanceof CharacterData ? node.length : node.childNodes.length);
        return value && validPoint(value.startContainer, value.startOffset) && validPoint(value.endContainer, value.endOffset);
    }
    function range(field) {
        const current = selection(field);
        if (current?.getComposedRanges) {
            const value = current.getComposedRanges({shadowRoots: [field.root]})[0];
            // After DOM replacement WebKit can retain composed offsets from
            // the old children. Check both points before making a live Range.
            if (validRange(field, value)) {
                const result = document.createRange();
                result.setStart(value.startContainer, value.startOffset);
                result.setEnd(value.endContainer, value.endOffset);
                return result;
            }
        }
        if (current?.rangeCount) {
            const result = current.getRangeAt(0);
            if (validRange(field, result)) return result.cloneRange();
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
            selectRange(field, validRange(field, field.caret) ? field.caret : range(field));
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
        if (field.mode === "html") { field.source.value = value; if (browsing) grow(field.source); }
        else field.rich.replaceChildren(parse(value));
        field.caret = null; field.sourceCaret = null;
        dirty(field); focus(field); historyState(field);
    }
    function history(field, redo) {
        const from = redo ? field.redo : field.undo, to = redo ? field.undo : field.redo;
        if (!held) cancelPicture(field);
        if (held || !from.length) return false;
        to.push(content(field)); field.lastKind = null;
        restore(field, from.pop()); return true;
    }
    function toggleMode(field) {
        if (held) return;
        cancelPicture(field);
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
        if (held || browsing) return;
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
        if (!browsing) field.label.setAttribute("aria-expanded", String(!field.collapsed));
        if (browsing && field.mode === "html" && !field.collapsed) requestAnimationFrame(() => grow(field.source));
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
        const available = (occlusionForm?.ordered ?? fields).filter(value => !value.collapsed && !value.hidden);
        const next = available[available.indexOf(field) + (backwards ? -1 : 1)];
        if (next) focus(next);
        else { active(field).blur(); void emit("focusLeave", {direction: backwards ? "previous" : "next"}); }
    }
    const hasContent = node => {
        if (node.querySelector("img,hr,audio,video,iframe,object,input,svg,math,[data-meguri-chip]")) return true;
        const texts = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
        while (texts.nextNode()) {
            if (texts.currentNode.data && !texts.currentNode.parentElement?.closest("style,script,template")) return true;
        }
        return false;
    };
    function fillLine(node) {
        if (!hasContent(node) && !node.querySelector("br")) node.append(document.createElement("br"));
    }
    function lineOwner(field, caret) {
        let node = caret.startContainer.nodeType === Node.ELEMENT_NODE ? caret.startContainer : caret.startContainer.parentElement;
        // The custom editable element is a boundary even though it is not an
        // HTML block tag. Never split the shadow/UI containers outside it.
        while (node !== field.rich && !isBlock(node)) node = node.parentElement;
        return node;
    }
    function fillRootLine(root, point) {
        let top = point.startContainer;
        while (top !== root && top.parentNode !== root) top = top.parentNode;
        const nodes = Array.from(root.childNodes);
        const at = top === root ? point.startOffset : nodes.indexOf(top);
        const block = node => node.nodeType === Node.ELEMENT_NODE && isBlock(node);
        let start = at, end = at;
        while (start > 0 && !block(nodes[start - 1])) start--;
        while (end < nodes.length && !block(nodes[end])) end++;
        const run = document.createRange(); run.setStart(root, start); run.setEnd(root, end);
        const content = run.cloneContents();
        if (!hasContent(content) && !content.querySelector("br")) root.insertBefore(document.createElement("br"), nodes[end] ?? null);
    }
    function breakLine(field, lineBreak) {
        beforeEdit(field, lineBreak ? "insertLineBreak" : "insertParagraph");
        const caret = range(field);
        // Deleting across sibling lines already leaves the boundary Return
        // needs, including a root inline run beside a block. Remember the
        // suffix's live point before deletion collapses the main range.
        let paragraphs = null;
        if (!lineBreak && !caret.collapsed) {
            const end = caret.cloneRange(); end.collapse(false);
            const first = lineOwner(field, caret), last = lineOwner(field, end);
            const adjacent = first === field.rich ? last.parentNode === first :
                last === field.rich ? first.parentNode === last : first.parentNode === last.parentNode;
            if (first !== last && adjacent &&
                (first === field.rich || inline(first)) && (last === field.rich || inline(last)) &&
                !first.closest("li,ol,ul") && !last.closest("li,ol,ul")) paragraphs = {first, last, end};
        }
        // H1/H3's final inline BR is a load/caret sentinel. Once Return
        // creates a block, H3 no longer drops it: remove it before extracting
        // the tail, retaining every preceding (authored) break.
        const loadBreak = !lineBreak && inline(field.rich) && field.rich.lastChild?.localName === "br" ? field.rich.lastChild : null;
        caret.deleteContents(); loadBreak?.remove();
        if (paragraphs) {
            const {first, last, end} = paragraphs;
            if (first === field.rich) fillRootLine(first, caret); else fillLine(first);
            if (last === field.rich) fillRootLine(last, end); else fillLine(last);
            caret.setStart(end.startContainer, end.startOffset); caret.collapse(true);
        } else if (lineBreak) {
            const br = document.createElement("br");
            caret.insertNode(br); caret.setStartAfter(br); caret.collapse(true);
            // At the end of inline content a second BR supplies the caret's
            // last line. H3 drops this final BR rather than the user's break.
            const owner = lineOwner(field, caret), tail = document.createRange();
            tail.setStartAfter(br); tail.setEnd(owner, owner.childNodes.length);
            const rest = tail.cloneContents();
            if (!hasContent(rest) && !rest.querySelector("br")) br.after(document.createElement("br"));
        } else {
            const owner = lineOwner(field, caret);
            let block = caret.startContainer.nodeType === Node.ELEMENT_NODE ? caret.startContainer : caret.startContainer.parentElement;
            // A list item's paragraph can itself be a div or another block.
            while (block !== field.rich && block.localName !== "li") block = block.parentElement;
            if (block.localName === "li") {
                const list = block.parentElement;
                if (!hasContent(block) && block === list.lastElementChild) {
                    const next = document.createElement("div"); next.append(document.createElement("br"));
                    list.after(next); block.remove(); if (!list.children.length) list.remove();
                    caret.selectNodeContents(next); caret.collapse(true);
                } else {
                    const tail = document.createRange(); tail.setStart(caret.startContainer, caret.startOffset); tail.setEnd(block, block.childNodes.length);
                    const next = document.createElement("li"); next.append(tail.extractContents());
                    fillLine(block); fillLine(next);
                    block.after(next); caret.selectNodeContents(next); caret.collapse(true);
                }
            } else {
                // At the root, split only this run of inline nodes. A later
                // authored block is a boundary, not part of the new line.
                let boundary = null;
                if (owner === field.rich) {
                    let top = caret.startContainer;
                    while (top !== owner && top.parentNode !== owner) top = top.parentNode;
                    const start = top === owner ? caret.startOffset : Array.from(owner.childNodes).indexOf(top) + 1;
                    boundary = Array.from(owner.childNodes).slice(start).find(node => node.nodeType === Node.ELEMENT_NODE && isBlock(node)) ?? null;
                }
                const tail = document.createRange(); tail.setStart(caret.startContainer, caret.startOffset); tail.setEnd(owner, owner.childNodes.length);
                if (boundary) tail.setEndBefore(boundary);
                const next = document.createElement("div"); next.append(tail.extractContents());
                fillLine(next);
                if (owner === field.rich) {
                    owner.insertBefore(next, boundary);
                    const prefix = document.createRange(); prefix.selectNodeContents(owner); prefix.setEndBefore(next);
                    // Only this inline run can need a blank source line;
                    // earlier authored blocks belong to preceding lines.
                    let previousBlock = next.previousSibling;
                    while (previousBlock && !(previousBlock.nodeType === Node.ELEMENT_NODE && isBlock(previousBlock))) previousBlock = previousBlock.previousSibling;
                    if (previousBlock) prefix.setStartAfter(previousBlock);
                    const previous = prefix.cloneContents();
                    if (inline(previous) && !hasContent(previous) && !previous.querySelector("br")) owner.insertBefore(document.createElement("br"), next);
                } else { fillLine(owner); owner.after(next); }
                caret.selectNodeContents(next); caret.collapse(true);
            }
        }
        selectRange(field, caret); dirty(field); historyState(field);
    }
    function cancelPicture(field) {
        field.picture = null; field.rich.removeAttribute("aria-busy");
    }
    function insert(field, html, caret) {
        if (!html) return false;
        const fragment = parse(html), last = fragment.lastChild;
        if (!last) return false;
        beforeEdit(field, "insertFromPaste");
        caret.deleteContents(); caret.insertNode(fragment); caret.setStartAfter(last); caret.collapse(true);
        selectRange(field, caret); dirty(field); historyState(field); reportCaret(field);
        return true;
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
            // A later edit wins over an unfinished paste. Never swallow keys
            // or insert the delayed picture ahead of already accepted input.
            if (field.picture) cancelPicture(field);
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
            if (field.picture) cancelPicture(field);
            dirty(field); historyState(field);
        });
        input.addEventListener("keydown", event => {
            if (held) { event.preventDefault(); return; }
            if (event.key === "Tab") { event.preventDefault(); tab(field, event.shiftKey); return; }
            if (field.picture && (event.key === "Backspace" || event.key === "Delete" ||
                (event.key.length === 1 && !event.metaKey && !event.ctrlKey))) cancelPicture(field);
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") event.preventDefault();
            if (event.metaKey && event.shiftKey && event.code === "KeyX") { event.preventDefault(); toggleMode(field); }
            if (event.metaKey && event.code === "KeyZ" && history(field, event.shiftKey)) event.preventDefault();
        });
        // The system's paste event reaches the real editable, including its
        // context menu. Native code snapshots local picture representations;
        // unsupported content remains refused until U197 supplies that path.
        input.addEventListener("paste", event => {
            event.preventDefault();
            if (held || field.mode !== "rich" ||
                !document.documentElement.classList.contains("mac")) return;
            fields.forEach(cancelPicture);
            const request = ++pictureID;
            field.picture = {request, caret: range(field)};
            field.rich.setAttribute("aria-busy", "true");
            void emit("pastePicture", {ord: field.ord, request}).catch(() => {
                if (field.picture?.request === request) cancelPicture(field);
            });
        });
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
    const textNodes = root => {
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const nodes = []; while (walker.nextNode()) if (walker.currentNode.data.length) nodes.push(walker.currentNode);
        return nodes;
    };
    function selectTexts(field, texts) {
        if (!texts.length) return;
        const value = document.createRange();
        value.setStart(texts[0], 0); value.setEnd(texts[texts.length - 1], texts[texts.length - 1].length);
        selectRange(field, value);
    }
    // The selected text nodes, split at the selection's edges, in order.
    function selectedTexts(field, selected) {
        // A live range: splitting a text node moves its boundary points with it.
        const scope = selected.cloneRange();
        if (scope.endContainer.nodeType === Node.TEXT_NODE && scope.endOffset < scope.endContainer.data.length) {
            scope.endContainer.splitText(scope.endOffset);
        }
        if (scope.startContainer.nodeType === Node.TEXT_NODE && scope.startOffset > 0) {
            const rest = scope.startContainer.splitText(scope.startOffset);
            const end = scope.endContainer === rest.previousSibling ? null : [scope.endContainer, scope.endOffset];
            scope.setStart(rest, 0);
            if (end) scope.setEnd(end[0], end[1]); else scope.setEnd(rest, rest.data.length);
        }
        return textNodes(field.rich).filter(node => scope.intersectsNode(node) &&
            !(node === scope.endContainer && scope.endOffset === 0) &&
            !(node === scope.startContainer && scope.startOffset === node.data.length));
    }
    // F4 Removing, for one text node: its outermost format element is split
    // into the content before it, the node itself, and the content after it;
    // the middle loses the format (F3).
    function removeAround(field, text, kind) {
        let outer = null;
        for (let element = text.parentElement; element && element !== field.rich; element = element.parentElement) {
            if (counts(element, kind)) outer = element;
        }
        if (!outer) return;
        const before = document.createRange(); before.setStartBefore(outer); before.setEndBefore(text);
        const after = document.createRange(); after.setStartAfter(text); after.setEndAfter(outer);
        const head = before.extractContents(), tail = after.extractContents();
        // Only an empty shell of the split goes; anything with unselected content stays.
        const empty = node => node.nodeType === Node.ELEMENT_NODE && !node.textContent && node.querySelector("*") === null;
        for (const fragment of [head, tail]) for (const node of Array.from(fragment.childNodes)) if (empty(node)) node.remove();
        outer.before(head); outer.after(tail);
        for (const element of Array.from(outer.querySelectorAll("*")).reverse()) if (counts(element, kind)) strip(element, kind);
        strip(outer, kind);
    }
    function format(field, command) {
        const kind = formats[command];
        const selected = range(field);
        if (held || selected.collapsed) return false;
        const has = withinFormat(field, selected.startContainer, kind) || withinFormat(field, selected.endContainer, kind);
        beforeEdit(field, "format");
        const before = field.rich.innerHTML;
        const texts = selectedTexts(field, selected);
        if (has) {
            for (const text of texts) removeAround(field, text, kind);
        } else {
            if (kind.excludes) for (const text of texts) removeAround(field, text, formats[kind.excludes]);
            // F4 step 3: each selected text gets the wrapper (blocks keep their
            // structure); then adjacent wrappers and neighbours of the format merge.
            const made = new Set();
            for (const text of texts) {
                if (withinFormat(field, text, kind)) continue;
                const wrapper = document.createElement(kind.tag);
                text.before(wrapper); wrapper.append(text); made.add(wrapper);
            }
            const mergeable = node => node?.nodeType === Node.ELEMENT_NODE && node.localName === kind.tag && node.attributes.length === 0;
            for (const wrapper of made) {
                if (!wrapper.isConnected) continue;
                let previous = wrapper.previousSibling;
                while (previous?.nodeType === Node.TEXT_NODE && previous.data === "") previous = previous.previousSibling;
                let target = wrapper;
                if (mergeable(previous)) { previous.append(...Array.from(wrapper.childNodes)); wrapper.remove(); target = previous; }
                let next = target.nextSibling;
                while (next?.nodeType === Node.TEXT_NODE && next.data === "") next = next.nextSibling;
                if (mergeable(next) && true) { target.append(...Array.from(next.childNodes)); next.remove(); }
            }
        }
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
    // The inspector's page never scrolls: the host's scroll view does, so it
    // is told where the caret is, and the source box grows with its text.
    let caretFrame = 0;
    function reportCaret(field) {
        if (caretFrame) return;
        caretFrame = requestAnimationFrame(() => {
            caretFrame = 0;
            const box = field.mode === "html" ? sourceCaret(field.source) : (() => {
                const value = range(field); const rects = value.getClientRects();
                return rects.length ? rects[rects.length - 1] : field.host.getBoundingClientRect();
            })();
            void emit("caret", {top: Math.max(0, box.top), bottom: Math.max(0, box.bottom)});
        });
    }
    // The caret's line in a source box: a hidden copy of the text before the
    // caret, laid out like the box, ends where the caret is.
    function sourceCaret(source) {
        const style = getComputedStyle(source), box = source.getBoundingClientRect();
        const mirror = document.createElement("div");
        for (const name of ["boxSizing", "width", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "borderTopWidth",
            "borderRightWidth", "borderBottomWidth", "borderLeftWidth", "borderStyle", "fontFamily", "fontSize", "fontWeight",
            "lineHeight", "letterSpacing", "tabSize"]) mirror.style[name] = style[name];
        Object.assign(mirror.style, {position: "absolute", visibility: "hidden", top: "0", left: "0", whiteSpace: "pre-wrap",
            overflowWrap: "break-word"});
        mirror.textContent = source.value.slice(0, source.selectionEnd);
        const marker = document.createElement("span"); marker.textContent = "\u200b"; mirror.append(marker);
        document.body.append(mirror);
        const top = box.top + marker.offsetTop - source.scrollTop, bottom = top + marker.offsetHeight;
        mirror.remove();
        return {top, bottom};
    }
    function grow(source) { source.style.height = "auto"; source.style.height = `${source.scrollHeight}px`; }
    function makeField(value, mode) {
        const section = document.createElement("section"), header = document.createElement("header");
        // N21.4: Browse names are labels, never individual disclosures.
        // The separate Add/Edit form keeps N17.1's collapse button (F3).
        // The one mode control is native, in the keyboard bar or the Mac bottom bar.
        const label = document.createElement(browsing ? "span" : "button");
        if (!browsing) label.type = "button";
        label.className = "label";
        label.textContent = value.name; label.id = `editor.field.${value.ord}.label`;
        header.append(label);
        const boundary = document.createElement("div"), host = document.createElement("div");
        boundary.className = "fieldBoundary"; host.className = "richHost"; boundary.append(host);
        const root = host.attachShadow({mode: "closed"});
        const style = document.createElement("style");
        style.textContent = ":host { display:block; position:relative } meguri-field-content { display:block; min-height:1.3em; outline:none; overflow-wrap:anywhere } " +
            ".placeholder { position:absolute; inset-block-start:0; inset-inline-start:0; max-width:100%; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; pointer-events:none; color:GrayText } [hidden] { display:none !important }" +
            (browsing ? " meguri-field-content { min-height:1.2em } img { max-height:64px }" : "") +
            (media ? " img { max-width:100%; vertical-align:middle; border-radius:4px; cursor:zoom-in }" +
                (browsing ? "" : " img { max-height:120px }") +
                " [data-meguri-chip] { display:inline-flex; align-items:center; gap:4px; max-width:100%; padding:1px 8px 1px 6px; margin:1px 0;" +
                " border-radius:999px; font-size:12px; line-height:18px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; vertical-align:middle;" +
                " cursor:default; user-select:none; color:-apple-system-label; background:-apple-system-quaternary-label }" +
                " [data-meguri-chip]::before { content:'\\266A'; color:-apple-system-control-accent }" : "");
        const rich = document.createElement("meguri-field-content");
        rich.contentEditable = "true"; rich.setAttribute("role", "textbox"); rich.setAttribute("aria-multiline", "true");
        rich.setAttribute("aria-label", value.name); rich.id = `editor.field.${value.ord}`;
        // Native inline prediction can duplicate a typed suffix in the Mac Add form.
        if (mode === "add" && document.documentElement.classList.contains("mac")) rich.setAttribute("writingsuggestions", "false");
        const placeholderLabel = document.createElement("span");
        placeholderLabel.className = "placeholder"; placeholderLabel.textContent = value.description;
        placeholderLabel.setAttribute("aria-hidden", "true");
        // Every form shows field text in the system font: the body text style
        // on touch (Dynamic Type), 13 pt on the Mac beside its 13 pt labels.
        // The note type's editor font and size belong to the card (N17.1,
        // 2026-11-27); the page still receives them. In the inspector an
        // unmarked field reads in its own content's direction.
        host.style.direction = value.rtl ? "rtl" : browsing ? "" : "ltr";
        if (browsing && !value.rtl) rich.dir = "auto";
        rich.replaceChildren(loadParse(value.html)); root.append(style, rich, placeholderLabel);
        const source = document.createElement("textarea"); source.id = `editor.field.${value.ord}`;
        source.setAttribute("aria-label", value.name); source.value = value.html;
        source.spellcheck = false; source.autocorrect = "off"; source.autocapitalize = "off";
        source.setAttribute("autocorrect", "off"); source.setAttribute("autocapitalize", "off");
        source.setAttribute("writingsuggestions", "false");
        section.append(header, boundary, source); container.append(section);
        const field = {ord: value.ord, baseline: value.baselineHtml ?? value.html,
            changed: Object.hasOwn(value, "baselineHtml"), pending: false, mode: value.mode,
            collapsed: browsing ? false : value.collapsed, hidden: value.hidden === true, section,
            rich, root, host: boundary, source, label, placeholder: placeholderLabel,
            timer: null, caret: null, sourceCaret: null, undo: [], redo: [], lastKind: null, lastEdit: 0};
        if (!browsing) label.addEventListener("click", () => collapse(field, !field.collapsed));
        // The box's own padding belongs to the field: a tap there gives it
        // focus. A tap in the text keeps the caret WebKit placed.
        boundary.addEventListener("click", event => {
            if (event.target === boundary && !held && !field.collapsed) active(field).focus();
        });
        wire(field, rich); wire(field, source); show(field);
        if (media) {
            // A chip plays its sound; a thumbnail opens the larger view (the host does both).
            rich.addEventListener("click", event => {
                const chip = event.target.closest?.("[data-meguri-chip]");
                // A chip the note's own HTML imitates may carry no name: it plays nothing.
                if (chip) {
                    event.preventDefault();
                    const name = chip.getAttribute("data-meguri-name");
                    if (name) void emit("playMedia", {name});
                    return;
                }
                if (event.target.localName === "img" && event.target.getAttribute("src")) {
                    void emit("openMedia", {name: event.target.getAttribute("src")});
                }
            });
        }
        if (browsing || document.documentElement.classList.contains("mac")) {
            const follow = () => { if (browsing || occlusionForm) reportCaret(field); };
            for (const type of ["input", "keyup", "mouseup", "focus"]) rich.addEventListener(type, follow);
            source.addEventListener("input", () => { if (browsing) grow(source); follow(); });
            for (const type of ["keyup", "mouseup", "focus"]) source.addEventListener(type, follow);
        }
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
    function configureOcclusionForm(message) {
        keys(message, ["header", "backExtra", "viewport", "tagsHeight"]);
        if (browsing || !document.documentElement.classList.contains("mac") ||
            !fields.some(value => value.ord === message.header) || !fields.some(value => value.ord === message.backExtra) ||
            !Number.isFinite(message.viewport) || message.viewport < 0 ||
            !Number.isFinite(message.tagsHeight) || message.tagsHeight < 0) throw new Error("Invalid native form");
        if (!occlusionForm) {
            const slot = () => {
                const row = document.createElement("div");
                row.className = "nativeRow"; row.setAttribute("aria-hidden", "true");
                return row;
            };
            occlusionForm = {image: slot(), tags: slot()};
            const header = fields.find(value => value.ord === message.header);
            const back = fields.find(value => value.ord === message.backExtra);
            // Reorder only the display. Commits retain sorted stored ordinals.
            const remaining = fields.filter(value => value !== header && value !== back);
            occlusionForm.ordered = [header, back, ...remaining];
            container.replaceChildren(header.section, occlusionForm.image, back.section,
                ...remaining.map(value => value.section), occlusionForm.tags);
            document.documentElement.classList.add("occlusionForm");
        }
        occlusionForm.viewport = message.viewport;
        occlusionForm.tags.style.height = `${Math.max(22, message.tagsHeight)}px`;
        reportFormLayout();
    }
    async function receive(message) {
        if (message.type === "setTheme") {
            keys(message, ["type", "theme"]); keys(message.theme, []); return {};
        }
        if (message.type === "openNote") {
            keys(message, ["type", "protocol", "loadId", "mode", "fields", "focus", "theme", "options"]);
            keys(message.options, ["closeHTMLTags", "shrinkImages", "occlusionForm"], ["closeHTMLTags", "shrinkImages"]);
            if (message.protocol !== protocol || !["add", "edit", "browse", "content"].includes(message.mode) ||
                !Number.isSafeInteger(message.loadId) || message.loadId <= loadId || !Array.isArray(message.fields) ||
                message.fields.length === 0 || ![null, ...message.fields.map(value => value.ord)].includes(message.focus) ||
                Object.keys(message.theme).length !== 0 ||
                typeof message.options.closeHTMLTags !== "boolean" || typeof message.options.shrinkImages !== "boolean") throw new Error("Invalid openNote");
            message.fields.forEach(validateField);
            if (message.fields.some((value, index) => index > 0 && value.ord <= message.fields[index - 1].ord)) throw new Error("Invalid field order");
            for (const field of fields) clearTimeout(field.timer);
            loadId = message.loadId; seq = 0; current = null;
            browsing = message.mode === "browse";
            // Edit Note shows the same media as the inspector (2026-11-24, option A).
            media = browsing || message.mode === "edit";
            document.documentElement.classList.toggle("inspector", browsing);
            cancelAnimationFrame(formFrame); formFrame = 0;
            occlusionForm = null; lastFormLayout = "";
            document.documentElement.classList.remove("occlusionForm");
            held = true; container.replaceChildren(); fields = message.fields.map(value => makeField(value, message.mode));
            if (Object.hasOwn(message.options, "occlusionForm")) configureOcclusionForm(message.options.occlusionForm);
            held = false; container.inert = false;
            focus(fields.find(field => field.ord === message.focus));
            await emit("loaded");
            return {loadId};
        }
        if (message.loadId !== loadId) throw new Error("Stale editor load");
        const field = fields.find(value => value.ord === message.ord);
        switch (message.type) {
        case "setOcclusionForm": {
            keys(message, ["type", "loadId", "header", "backExtra", "viewport", "tagsHeight"]);
            configureOcclusionForm({header: message.header, backExtra: message.backExtra,
                viewport: message.viewport, tagsHeight: message.tagsHeight});
            return {loadId};
        }
        case "flush": {
            keys(message, ["type", "loadId", "hold"]);
            if (typeof message.hold !== "boolean") throw new Error("Invalid hold");
            if (message.hold) { held = true; fields.forEach(cancelPicture); }
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
            cancelPicture(field);
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
                if (hidden) cancelPicture(value);
                value.hidden = hidden; show(value);
            }
            return {loadId};
        }
        case "format": {
            keys(message, ["type", "loadId", "command"]);
            if (!Object.hasOwn(formats, message.command)) throw new Error("Unknown format");
            if (held || !current || current.mode !== "rich") return {applied: false};
            cancelPicture(current);
            return {applied: format(current, message.command)};
        }
        case "wrap": {
            keys(message, ["type", "loadId", "prefix", "suffix"]);
            if (typeof message.prefix !== "string" || typeof message.suffix !== "string") throw new Error("Invalid wrap");
            if (held || !current || current.mode !== "rich") return {applied: false};
            cancelPicture(current);
            wrap(current, message.prefix, message.suffix);
            return {applied: true};
        }
        case "classifyPicturePaste": {
            keys(message, ["type", "loadId", "html", "urls"]);
            if (typeof message.html !== "string" || message.html.length > 1048576 ||
                !Array.isArray(message.urls) || message.urls.some(url => typeof url !== "string")) throw new Error("Invalid clipboard HTML");
            const fragment = parse(message.html);
            // Never attach clipboard nodes to the document. Attribute text,
            // including > in alt, is not selection text or another element.
            for (const node of fragment.querySelectorAll("head,script,style,meta,link,base,title")) node.remove();
            const elements = [...fragment.querySelectorAll("*")];
            const wrappers = new Set(["a", "div", "span", "p", "figure"]);
            const text = fragment.textContent.trim();
            return {copyImage: elements.filter(node => node.localName === "img").length === 1 &&
                elements.every(node => node.localName === "img" || wrappers.has(node.localName)) &&
                (!text || message.urls.includes(text))};
        }
        case "finishPicturePaste": {
            keys(message, ["type", "loadId", "ord", "request", "html"]);
            if (!field || !Number.isSafeInteger(message.request) || typeof message.html !== "string") throw new Error("Invalid picture result");
            const picture = field.picture;
            if (picture?.request !== message.request) return {changed: false};
            cancelPicture(field);
            if (held || field.hidden || field.collapsed || field.mode !== "rich" || !message.html) return {changed: false};
            // P25: restore the recorded field and selection, even after the
            // user focused another field while its file was being imported.
            field.caret = picture.caret; focus(field);
            return {changed: insert(field, message.html, picture.caret)};
        }
        case "picturePasteCurrent":
            keys(message, ["type", "loadId", "ord", "request"]);
            return {current: !!field && !held && !field.hidden && !field.collapsed && field.mode === "rich" &&
                field.picture?.request === message.request};
        case "undo": case "redo":
            keys(message, ["type", "loadId", "ord"]); if (!field) throw new Error("Unknown field");
            cancelPicture(field);
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
    function reportFormLayout() {
        if (!occlusionForm || formFrame) return;
        formFrame = requestAnimationFrame(() => {
            formFrame = 0;
            if (!occlusionForm) return;
            const {image, tags, viewport} = occlusionForm;
            const other = document.body.getBoundingClientRect().height - image.getBoundingClientRect().height;
            // 280 pt canvas plus room for native toolbar, footer and messages.
            image.style.height = `${Math.max(400, viewport - other)}px`;
            const imageBox = image.getBoundingClientRect(), tagsBox = tags.getBoundingClientRect();
            const layout = {imageTop: imageBox.top + window.scrollY, imageHeight: imageBox.height,
                tagsTop: tagsBox.top + window.scrollY, height: Math.ceil(document.body.getBoundingClientRect().height)};
            const signature = JSON.stringify(layout);
            if (signature !== lastFormLayout) { lastFormLayout = signature; void emit("occlusionFormLayout", layout); }
        });
    }
    new ResizeObserver(() => {
        if (occlusionForm) { reportFormLayout(); return; }
        if (loadId) void emit("contentHeight", {height: browsing ? Math.ceil(document.body.getBoundingClientRect().height)
            : document.documentElement.scrollHeight});
    }).observe(document.body);
    void bridge.postMessage({type: "ready", protocol});
})();
