@testable import AppModel
import CoreBridge
import Foundation
import Testing
import WebKit
@testable import Editor

/// The inspector's field page (Option A, the owner's choice of 2026-11-23):
/// a sound reference shows as a chip that stands for its exact text, so a
/// field's bytes survive the chips, a switch to the source and back, and an
/// edit beside them; media names reach the host as plain file names only;
/// the formatting bar acts only while a field has the focus.
@MainActor
@Suite(.serialized) struct BrowseInspectorPageTests {
    private static let stored = "say [sound:kip wurl.mp3] then <b>tem</b> [sound:a&amp;b.ogg]<img src=\"zant.png\">"

    @Test(arguments: ["add", "edit", "browse"])
    func sparseDefinitionsUsePositionsForThePageFocusModesClozeAndClosedReload(_ flow: String) async throws {
        let type = Self.type(ords: [7, 19], cloze: true)
        let deck = DeckNameEntry(id: 1, fullName: "D", filtered: false)
        let note = NoteRecord(id: 5, notetypeId: 42, fields: [Self.stored, "second"], tags: [], guid: "g")
        let draft: NoteEditorDraft
        if flow == "add" {
            draft = try NoteEditorDraft(generation: 1, notetype: type, deck: deck, values: note.fields)
        } else {
            draft = try NoteEditorDraft(editing: note, notetype: type, deck: deck, generation: 1)
        }
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.browsing = flow == "browse"
        defer { host.shutDown() }
        try await host.start()
        #expect(draft.fields.map { $0.definition.ord } == [7, 19])
        try await host.focusField(1)
        _ = try await host.webView.callAsyncJavaScript("""
            const {loadId, seq} = await meguriEditorRuntime.receive({type:'flush', loadId:1, hold:false});
            return await window.webkit.messageHandlers.meguriEditor.postMessage({type:'focus', loadId, seq, ord:1, mode:'rich'});
            """, contentWorld: .world(name: "Meguri.FieldEditor.v1"))
        try await pollUntil { draft.focusedField == 1 && host.fieldFocused }
        #expect(host.modeField == 1 && host.canFormat && host.canCloze)
        #expect(host.modeControlIdentifier == "editor.field.1.htmlToggle")
        try await host.setMode(.html, ord: 1)
        #expect(try await FieldEditorTests.replaceSource(host, ord: 1, value: "<b>later</b>"))
        try await host.setMode(.rich, ord: 1)
        try await host.flush(hold: false)
        #expect(draft.values == [Self.stored, "<b>later</b>"])
        if flow != "add" {
            let patch = try #require(draft.patch())
            #expect(patch.fields == [1: "<b>later</b>"] && patch.notetype == type)
            draft.acknowledgeSaved(patch)
            #expect(!draft.hasChanges && draft.fields[1].changed, "the page baseline survives a save until it is reloaded")
            let positions = draft.reloadUnchanged(stored: [Self.stored + " external", "<b>later</b>"], tags: [])
            #expect(positions == [0])
            try await host.reload(fields: positions)
        }
        host.shutDown()
        // A new page over the retained draft must not reinterpret stored ords.
        let reopened = try FieldEditorHost(draft: draft, isCurrent: { true })
        reopened.browsing = flow == "browse"
        defer { reopened.shutDown() }
        try await reopened.start()
        try await reopened.setMode(.html, ord: 1)
        let source = try await reopened.webView.callAsyncJavaScript(
            "return document.querySelectorAll('textarea')[1].value;", contentWorld: .page) as? String
        #expect(source == "<b>later</b>")
        try await reopened.flush(hold: false)
        #expect(draft.values[0] == Self.stored + (flow == "add" ? "" : " external"))
        if flow != "add" { #expect(draft.patch() == nil) }
    }

    /// `editor/formatting` F13: a cloze wrap names its field; the page refuses
    /// it once that field has lost the focus (Next Field during Cloze's flush).
    @Test func theClozeWrapActsOnlyOnTheFieldItNames() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.type(cloze: true), deck: DeckNameEntry(id: 1, fullName: "D", filtered: false),
                                        values: ["first", "second"])
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        BrowseFieldEditingTests.installProbe(host)
        defer { host.shutDown() }
        try await host.start()
        // The page's own focus (a detached web view emits no focus event by itself).
        try await BrowseFieldEditingTests.edit(host, "", ord: 1)
        try await pollUntil { draft.focusedField == 1 && host.fieldFocused }
        func wrap(_ ord: Int) async throws -> Bool? {
            let reply = try await host.webView.callAsyncJavaScript(
                "return (await meguriEditorRuntime.receive({type:'wrap', loadId:1, ord, prefix:'{{c1::', suffix:'}}'})).applied;",
                arguments: ["ord": ord], contentWorld: .world(name: "Meguri.FieldEditor.v1"))
            return reply as? Bool
        }
        #expect(try await wrap(0) == false)
        #expect(try await wrap(1) == true)
        try await host.flush(hold: false)
        #expect(draft.values[0] == "first")
        #expect(draft.values[1].contains("{{c1::"))
    }

    /// Edit Note's page (option A, 2026-11-24) shows the same chips: `browsing` false.
    @Test(arguments: [true, false])
    func soundChipsKeepTheFieldsBytesThroughTheSourceAndAnEdit(browsing: Bool) async throws {
        let note = NoteRecord(id: 5, notetypeId: 42, fields: [Self.stored, "plain"], tags: [], guid: "g")
        let draft = try NoteEditorDraft(editing: note, notetype: Self.type(), deck: DeckNameEntry(id: 1, fullName: "D", filtered: false),
                                        generation: 1)
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.browsing = browsing
        host.webView.frame = CGRect(x: 0, y: 0, width: 320, height: 400)
        defer { host.shutDown() }
        try await host.start()
        // Opening writes nothing: the chips are presentation only.
        try await host.flush(hold: false)
        #expect(draft.values == [Self.stored, "plain"] && !draft.hasChanges && draft.committedPatch() == nil)
        // The source shows the stored text, not the chips.
        try await host.setMode(.html, ord: 0)
        let source = try await host.webView.callAsyncJavaScript("return document.querySelectorAll('textarea')[0].value;",
                                                                contentWorld: .page) as? String
        #expect(source == Self.stored)
        // An edit in the source, back to the chips, and a flush: the references come back byte for byte.
        let edited = Self.stored + " more"
        #expect(try await FieldEditorTests.replaceSource(host, ord: 0, value: edited))
        try await host.setMode(.rich, ord: 0)
        try await host.flush(hold: false)
        #expect(draft.values[0] == edited && draft.values[1] == "plain")
        // The rich page (with its chips) serialized again: the same text.
        try await host.setMode(.html, ord: 0)
        let again = try await host.webView.callAsyncJavaScript("return document.querySelectorAll('textarea')[0].value;",
                                                               contentWorld: .page) as? String
        #expect(again == edited)
        guard browsing else { return }
        // The page reports its height for the pane's scroll view.
        for _ in 0..<200 where host.contentHeight == 0 { try await Task.sleep(for: .milliseconds(10)) }
        #expect(host.contentHeight > 0 && host.contentHeight < 400, "the page reports its fields' height, not its frame's")
    }

    @Test(arguments: [true, false])
    func browseFieldsCannotCollapseAndTheSeparateEditorStillCan(_ browsing: Bool) async throws {
        let note = NoteRecord(id: 5, notetypeId: 42, fields: [Self.stored, ""], tags: [], guid: "g")
        let draft = try NoteEditorDraft(editing: note, notetype: Self.type(collapsed: true),
            deck: DeckNameEntry(id: 1, fullName: "D", filtered: false), generation: 1)
        draft.hiddenFields = [1]
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.browsing = browsing
        host.webView.frame = CGRect(x: 0, y: 0, width: 280, height: 500)
        defer { host.shutDown() }
        try await host.start()
        let shown = try await host.webView.callAsyncJavaScript("""
            const section = document.querySelector('section');
            const label = section.querySelector('.label');
            const before = !section.querySelector('.fieldBoundary').hidden;
            label.click(); section.click(); section.querySelector('.fieldBoundary').click();
            return {before, after: !section.querySelector('.fieldBoundary').hidden,
                disclosure: label.hasAttribute('aria-expanded'), button: label.tagName === 'BUTTON'};
            """, contentWorld: .page) as? [String: Bool]
        #expect(shown?["before"] == browsing)
        #expect(shown?["after"] == true)
        #expect(shown?["disclosure"] == !browsing && shown?["button"] == !browsing)
        if browsing {
            // The aggregate disclosure alone controls empty-field visibility.
            try await host.setHiddenFields([])
            #expect(try await host.webView.callAsyncJavaScript("""
                const section = document.querySelectorAll('section')[1];
                section.querySelector('.label').click(); section.click();
                return !section.hidden && !section.querySelector('.fieldBoundary').hidden;
                """, contentWorld: .page) as? Bool == true)
            try await host.focusField(1)
            // A detached WKWebView may not emit DOM focus. Deliver it through
            // the real bridge, using the runtime's next sequence (as in FieldEditorTests).
            // Awaiting setFocus alone does not join the host's message queue.
            _ = try await host.webView.callAsyncJavaScript("""
                const {loadId, seq} = await meguriEditorRuntime.receive({type:'flush', loadId:1, hold:false});
                return await window.webkit.messageHandlers.meguriEditor.postMessage({type:'focus', loadId, seq, ord:1, mode:'rich'});
                """, contentWorld: .world(name: "Meguri.FieldEditor.v1"))
            for _ in 0..<200 where draft.focusedField != 1 || !host.fieldFocused {
                try await Task.sleep(for: .milliseconds(10))
            }
            #expect(host.modeField == 1, "type-collapsed fields still support the formatting/source controls")
            try await host.setMode(.html, ord: 1)
            #expect(try await FieldEditorTests.replaceSource(host, ord: 1, value: "new empty-field text"))
            try await host.flush(hold: false)
            #expect(draft.values == [Self.stored, "new empty-field text"])
            try await host.setHiddenFields([1])
            #expect(try await host.webView.callAsyncJavaScript(
                "return document.querySelectorAll('section')[1].hidden;", contentWorld: .page) as? Bool == true)
        }
        let allFieldsCollapsed: Bool = draft.notetype.fields.allSatisfy(\.collapsed)
        #expect(allFieldsCollapsed, "the note type's setting is never rewritten")
    }

    @Test func mediaNamesArePlainFileNamesOnly() throws {
        #expect(MediaName.plain("kip wurl.mp3") == "kip wurl.mp3")
        #expect(MediaName.plain("kip%20wurl.mp3") == "kip wurl.mp3")
        for refused in ["", ".", "..", "../secret", "a/b.png", "a\\b.png", ".hidden", "line\nbreak.mp3", "%2E%2E%2Fsecret"] {
            #expect(MediaName.plain(refused) == nil, "\(refused.debugDescription)")
        }
        // A reference the host cannot serve is ignored, never a protocol failure that stops the page.
        #expect(try EditorBridgeMessage(["type": "playMedia", "loadId": 1, "seq": 1, "name": "../x.mp3"], ordinals: [0]).type == "playMedia")
        #expect(try EditorBridgeMessage(["type": "openMedia", "loadId": 1, "seq": 1, "name": "https://example.org/a.png"],
                                        ordinals: [0]).type == "openMedia")
        // A chip the note's HTML imitates without a name is ignored too, never a failed load.
        #expect(try EditorBridgeMessage(["type": "playMedia", "loadId": 1, "seq": 1, "name": NSNull()], ordinals: [0]).type == "playMedia")
        #expect(throws: (any Error).self) {
            try EditorBridgeMessage(["type": "playMedia", "loadId": 1, "seq": 1, "name": 3], ordinals: [0])
        }
        #expect(throws: (any Error).self) {
            try EditorBridgeMessage(["type": "caret", "loadId": 1, "seq": 1, "top": 20, "bottom": 10], ordinals: [0])
        }
        #expect(try EditorBridgeMessage(["type": "openMedia", "loadId": 1, "seq": 1, "name": "zant.png"], ordinals: [0]).type == "openMedia")
    }

    @Test func theFormattingBarActsOnlyWhileARichFieldHasTheFocus() {
        #expect(FieldEditorHost.formats(editable: true, held: false, fieldFocused: true, mode: .rich))
        #expect(!FieldEditorHost.formats(editable: true, held: false, fieldFocused: false, mode: .rich), "no focus: dimmed")
        #expect(!FieldEditorHost.formats(editable: true, held: false, fieldFocused: true, mode: .html), "the source has no formats")
        #expect(!FieldEditorHost.formats(editable: true, held: true, fieldFocused: true, mode: .rich), "a held page takes nothing")
        #expect(!FieldEditorHost.formats(editable: false, held: false, fieldFocused: true, mode: .rich))
    }

    private static func type(collapsed: Bool = false, ords: [UInt32] = [0, 1], cloze: Bool = false) -> NotetypeRecord {
        let fields = ["Front", "Back"].enumerated().map { index, name in
            NotetypeField(ord: ords[index], id: Int64(index), name: name, sticky: false, rtl: false, fontName: "Hiragino Mincho",
                          fontSize: 40, description: "", plainText: false, collapsed: collapsed, excludeFromSearch: false)
        }
        return .init(id: 42, name: "Two fields", kind: cloze ? 1 : 0, sortField: 0, css: "", base: [], fields: fields, templates: [],
                     clozeFields: cloze ? [1] : [])
    }
}
