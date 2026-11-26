import CoreBridge
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#endif
@testable import AppModel
@testable import Editor

// N17.1. These join the serialized real-host suite: no second owner of the
// process-wide CoreBridge host.
extension FieldEditorTests {
    private static func form(_ names: [String], id: Int64 = 77) -> NotetypeRecord {
        let fields = names.enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: false,
                rtl: false, fontName: "", fontSize: 20, description: "", plainText: false,
                collapsed: false, excludeFromSearch: false)
        }
        return .init(id: id, name: "Form \(id)", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [], originalStockKind: 0)
    }
    private static let deck = DeckNameEntry(id: 9, fullName: "Languages::Daily", filtered: false)

    #if os(macOS)
    @Test(arguments: [false, true])
    func manyFieldMacFormOpensAtTheSizedViewport(_ editing: Bool) async throws {
        let type = Self.form((0..<24).map { "Field \($0)" })
        let values = (0..<24).map { "Synthetic \($0)" }
        let draft = try editing
            ? NoteEditorDraft(editing: .init(id: 5, notetypeId: type.id, fields: values, tags: [], guid: "g"),
                              notetype: type, deck: Self.deck, generation: 1)
            : NoteEditorDraft(generation: 1, notetype: type, deck: Self.deck, values: values)
        var host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        BrowseFieldEditingTests.installProbe(host)
        // Production starts the page before SwiftUI mounts and sizes the web view.
        try #require(host.webView.frame == .zero)
        try await host.start()
        try #require(try await host.webView.callAsyncJavaScript("return innerWidth === 0 && innerHeight === 0;",
            contentWorld: .page) as? Bool == true, "exercise eager focus at zero viewport")
        let window = EditorFormTestWindow(contentRect: CGRect(x: -30_000, y: -30_000, width: 500, height: 240),
                                          styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host.webView
        host.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 240)
        window.makeFirstResponder(host.webView)
        defer { window.contentView = nil; window.close() }

        func checkOpening() async throws {
            let checks = try #require(try await host.webView.callAsyncJavaScript("""
                await new Promise((resolve, reject) => {
                    const timeout = setTimeout(() => reject(new Error('No sized viewport frames within 2 s')), 2000);
                    requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => {
                        clearTimeout(timeout); resolve();
                    })));
                });
                const root = testFieldRoots.find(root => root.host.isConnected);
                const box = document.querySelector('.fieldBoundary'), rect = box.getBoundingClientRect();
                const style = getComputedStyle(box), ring = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
                return {
                    sizedScrollableViewport: innerWidth === 500 && innerHeight === 240 && document.scrollingElement.scrollHeight > innerHeight,
                    firstFieldFocused: root.activeElement === root.querySelector('meguri-field-content'),
                    boxVisible: rect.top >= 0 && rect.bottom <= innerHeight && rect.left >= 0 && rect.right <= innerWidth,
                    outlineVisible: ring > 0 && rect.top - ring >= 0 && rect.bottom + ring <= innerHeight &&
                        rect.left - ring >= 0 && rect.right + ring <= innerWidth
                };
                """, contentWorld: BrowseFieldEditingTests.world) as? [String: Bool])
            // Before the repair the opening selection reveal clips the first box
            // or its outline. No test-driven focus or scrolling repairs that state.
            for (name, passed) in checks { #expect(passed, "Opening form: \(name)") }
        }
        try await checkOpening()
        if !editing {
            // Type changes and acknowledged adds open a new draft in this page.
            let changed = try draft.replacingSelection(notetype: Self.form((0..<18).map { "Changed \($0)" }, id: 78), deck: Self.deck)
            for next in [changed, try changed.nextAfterAcknowledgedAdd()] {
                try await host.flush(hold: true)
                host = try #require(host.successor(for: next, isCurrent: { true }) as? FieldEditorHost)
                try await host.start()
                try await checkOpening()
            }
        }
    }
    #endif

    @Test func addWaitsForContentInTheFirstField() throws {
        let blank = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck)
        #expect(!blank.firstFieldHasContent)
        let onlyBack = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck, values: ["<br> ", "answer"])
        #expect(!onlyBack.firstFieldHasContent)
        // Typing counts at once, before its commit arrives.
        onlyBack.markDirty(1)
        #expect(!onlyBack.firstFieldHasContent)
        onlyBack.markDirty(0)
        #expect(onlyBack.firstFieldHasContent)
        let filled = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck, values: ["question", ""])
        #expect(filled.firstFieldHasContent)
    }

    @Test func deckChoicesFollowTheDeckTreeForTheDefaultDeck() {
        let names = [DeckNameEntry(id: 1, fullName: "Default", filtered: false),
                     DeckNameEntry(id: 5, fullName: "Cram", filtered: true),
                     DeckNameEntry(id: 9, fullName: "Spanish", filtered: false),
                     DeckNameEntry(id: 10, fullName: "Spanish::Mining", filtered: false)]
        #expect(AddNoteFlow.deckChoices(names: names, selected: 9, defaultShown: false).map(\.id) == [9, 10])
        #expect(AddNoteFlow.deckChoices(names: names, selected: 9, defaultShown: true).map(\.id) == [1, 9, 10])
        // The selected deck stays listed, and the list is never empty.
        #expect(AddNoteFlow.deckChoices(names: names, selected: 1, defaultShown: false).map(\.id) == [1, 9, 10])
        #expect(AddNoteFlow.deckChoices(names: Array(names.prefix(2)), selected: 9, defaultShown: false).map(\.id) == [1])
    }

    @Test func onlyAWideIPhoneCoversTheScreen() {
        #expect(AddNotePresentation.coversScreen(phone: true, regularWidth: true))
        #expect(!AddNotePresentation.coversScreen(phone: true, regularWidth: false))
        #expect(!AddNotePresentation.coversScreen(phone: false, regularWidth: true))
    }

    @Test func aNoteTypeChangeKeepsTheFocusedPosition() throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back", "Extra"]), deck: Self.deck, values: ["a", "b", "c"])
        draft.setFocus(2)
        let narrower = try draft.replacingSelection(notetype: Self.form(["Text", "Back"], id: 78), deck: Self.deck)
        #expect(narrower.focusedField == 1 && narrower.values == ["a", "b"])
        draft.setFocus(nil)
        let first = try draft.replacingSelection(notetype: Self.form(["Text", "Back"], id: 78), deck: Self.deck)
        #expect(first.focusedField == 0)
    }

    @Test func oneModeControlActsOnTheFocusedField() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back", "Extra"]), deck: Self.deck)
        let page = try FieldEditorHost(draft: draft, isCurrent: { true })
        page.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 700)
        let host = page
        defer { host.shutDown() }
        #expect(page.modeField == nil && page.modeControlIdentifier == "editor.add.htmlToggle")
        try await page.start()
        // The page has each name as a header and no mode button of its own.
        #expect(try await page.webView.callAsyncJavaScript("""
            return document.querySelectorAll('button.label').length === 3 &&
                document.querySelectorAll('[id$=".htmlToggle"]').length === 0 &&
                document.documentElement.className.length > 0;
            """, contentWorld: .page) as? Bool == true)
        draft.setFocus(1)
        #expect(page.modeField == 1 && page.focusedMode == .rich)
        #expect(page.modeControlIdentifier == "editor.field.1.htmlToggle")
        #expect(page.adjacentField(previous: true) == 0 && page.adjacentField(previous: false) == 2)
        page.toggleFocusedMode()
        for _ in 0..<200 where draft.fields[1].mode != .html { try await Task.sleep(for: .milliseconds(10)) }
        #expect(draft.fields[1].mode == .html && draft.fields[0].mode == .rich && page.focusedMode == .html)
        draft.setCollapsed(true, ord: 2)
        #expect(page.adjacentField(previous: false) == nil)
        draft.setFocus(0)
        #expect(page.adjacentField(previous: true) == nil && page.adjacentField(previous: false) == 1)
    }

    @Test func theNextDraftLoadsIntoTheSamePage() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck, values: ["question", "answer"])
        let first = try FieldEditorHost(draft: draft, isCurrent: { true })
        first.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 700)
        // A page that is not editable yet has nothing to hand on.
        #expect(first.successor(for: draft, isCurrent: { true }) == nil)
        try await first.start()
        try await first.flush(hold: true)
        let next = try draft.nextAfterAcknowledgedAdd()
        let second = try #require(first.successor(for: next, isCurrent: { true }) as? FieldEditorHost)
        #expect(second.webView === first.webView && !first.editable && second.draft === next)
        try await second.start()
        #expect(second.editable && !second.held)
        // The page shows the cleared draft and accepts input again.
        #expect(try await second.webView.callAsyncJavaScript("""
            return document.querySelectorAll('button.label').length === 2 && !document.getElementById('fields').inert;
            """, contentWorld: .page) as? Bool == true)
        try await second.flush(hold: false)
        #expect(next.values == ["", ""])
        // The first owner is over: it cannot start again or end the page it gave away.
        await #expect(throws: (any Error).self) { try await first.start() }
        first.shutDown()
        try await second.flush(hold: false)
        second.shutDown()
    }

    @Test func inputThatArrivesDuringAHoldIsCommittedAfterRelease() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck)
        let page = try FieldEditorHost(draft: draft, isCurrent: { true })
        page.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 700)
        try await page.start()
        try await page.setMode(.html, ord: 0)
        try await page.flush(hold: true)
        // Composition input is not cancelable: it reaches the field although held.
        #expect(try await page.webView.callAsyncJavaScript("""
            const source = document.querySelectorAll('textarea')[0];
            const before = new InputEvent('beforeinput', {bubbles: true, cancelable: false, inputType: 'insertCompositionText', data: 'ime'});
            source.dispatchEvent(before);
            source.value = 'ime';
            source.dispatchEvent(new InputEvent('input', {bubbles: true, inputType: 'insertCompositionText', data: 'ime'}));
            return true;
            """, contentWorld: .page) as? Bool == true)
        try await page.flush(hold: true)
        #expect(draft.values[0] == "", "a hold admits nothing into the draft")
        try await page.release()
        try await page.flush(hold: false)
        #expect(draft.values[0] == "ime" && draft.firstFieldHasContent)
        page.shutDown()
    }
}

#if os(macOS)
@MainActor private final class EditorFormTestWindow: NSWindow {
    override var isVisible: Bool { true }
    override var occlusionState: NSWindow.OcclusionState { .visible }
}
#endif
