import CoreBridge
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
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
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.shutDown() }
        BrowseFieldEditingTests.installProbe(host)
        // Production focuses before SwiftUI mounts the retained, zero-sized view.
        try #require(host.webView.frame == .zero)
        try await host.start()
        try #require(try await host.webView.callAsyncJavaScript("return innerWidth === 0 && innerHeight === 0;",
            contentWorld: .page) as? Bool == true)

        func settleLayout() async throws {
            _ = try await host.webView.callAsyncJavaScript("""
                await new Promise((resolve, reject) => {
                    const timeout = setTimeout(() => reject(new Error('Layout did not settle')), 2000);
                    requestAnimationFrame(() => requestAnimationFrame(() => {
                        clearTimeout(timeout); resolve();
                    }));
                });
                """, contentWorld: .page)
        }
        let window = EditorFormTestWindow(contentRect: CGRect(x: -30_000, y: -30_000, width: 1, height: 1),
                                          styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host.webView
        window.makeFirstResponder(host.webView)
        defer { window.contentView = nil; window.close() }
        // The first nonzero layout need not be the final usable viewport.
        host.webView.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
        try await settleLayout()
        window.setContentSize(NSSize(width: 500, height: 240))
        host.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 240)
        try await settleLayout()

        let checks = try #require(try await host.webView.callAsyncJavaScript("""
            const root = testFieldRoots.find(root => root.host.isConnected);
            const box = document.querySelector('.fieldBoundary');
            const style = getComputedStyle(box);
            return {
                scrollable: innerHeight === 240 && document.scrollingElement.scrollHeight > innerHeight,
                focused: root.activeElement === root.querySelector('meguri-field-content'),
                topVisible: parseFloat(style.outlineWidth) > 0 &&
                    box.getBoundingClientRect().top - parseFloat(style.outlineWidth) >= 0
            };
            """, contentWorld: BrowseFieldEditingTests.world) as? [String: Bool])
        // No test-driven focus or scrolling may repair the clipped opening box.
        #expect(checks["scrollable"] == true)
        #expect(checks["focused"] == true)
        #expect(checks["topVisible"] == true, "The first field and its focus ring must start within the viewport")
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

    /// N17.1 (2026-11-27): field text takes the system font in every form; the
    /// note type's editor font and size stay with the card.
    @Test func fieldTextTakesTheSystemFontNotTheNoteTypes() async throws {
        let fields = ["Front", "Back"].enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: false,
                rtl: false, fontName: "Example Serif", fontSize: 40, description: "", plainText: false,
                collapsed: false, excludeFromSearch: false)
        }
        let type = NotetypeRecord(id: 79, name: "Large", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [])
        let draft = try NoteEditorDraft(generation: 1, notetype: type, deck: Self.deck, values: ["question", "answer"])
        let page = try FieldEditorHost(draft: draft, isCurrent: { true })
        page.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 700)
        defer { page.shutDown() }
        try await page.start()
        #expect(try await page.webView.callAsyncJavaScript("""
            const hosts = [...document.querySelectorAll('.richHost')];
            const body = getComputedStyle(document.body);
            return hosts.length === 2 && hosts.every(host => host.style.fontFamily === '' && host.style.fontSize === '' &&
                getComputedStyle(host).fontSize === body.fontSize && getComputedStyle(host).fontFamily === body.fontFamily);
            """, contentWorld: .page) as? Bool == true)
    }

    /// `editor/cloze-wrap` W2: the cloze group follows the note type's shown cloze fields.
    @Test func theClozeGroupShowsOnlyForAShownClozeField() throws {
        func type(kind: Int32, cloze: [UInt32]) -> NotetypeRecord {
            let base = Self.form(["Text", "Back Extra"], id: 80)
            return .init(id: base.id, name: base.name, kind: kind, sortField: 0, css: "", base: [], fields: base.fields,
                         templates: [], clozeFields: cloze)
        }
        let basic = try FieldEditorHost(draft: NoteEditorDraft(generation: 1, notetype: type(kind: 0, cloze: []), deck: Self.deck),
                                        isCurrent: { true })
        defer { basic.shutDown() }
        #expect(!basic.showsCloze && !basic.canCloze)
        let none = try FieldEditorHost(draft: NoteEditorDraft(generation: 1, notetype: type(kind: 1, cloze: []), deck: Self.deck),
                                       isCurrent: { true })
        defer { none.shutDown() }
        #expect(!none.showsCloze)
        let cloze = try FieldEditorHost(draft: NoteEditorDraft(generation: 1, notetype: type(kind: 1, cloze: [0]), deck: Self.deck),
                                        isCurrent: { true })
        defer { cloze.shutDown() }
        #expect(cloze.showsCloze)
        let hiddenDraft = try NoteEditorDraft(generation: 1, notetype: type(kind: 1, cloze: [0]), deck: Self.deck)
        hiddenDraft.hiddenFields = [0]
        let hidden = try FieldEditorHost(draft: hiddenDraft, isCurrent: { true })
        defer { hidden.shutDown() }
        #expect(!hidden.showsCloze)
    }

    #if os(iOS)
    /// N17.1 (2026-11-27): the keyboard bar is Bold, Italic, Underline, Cloze,
    /// then previous and next field, then the HTML control. It has no Done.
    @Test func theKeyboardBarFormatsAndHasNoDone() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.form(["Front", "Back"]), deck: Self.deck)
        let page = try FieldEditorHost(draft: draft, isCurrent: { true })
        page.webView.frame = CGRect(x: 0, y: 0, width: 390, height: 700)
        defer { page.shutDown() }
        let bar = try #require(page.webView.inputAccessoryView as? UIToolbar)
        let items = (bar.items ?? []).filter { $0.accessibilityIdentifier != nil }
        #expect(items.map(\.accessibilityIdentifier) == ["editor.toolbar.bold", "editor.toolbar.italic", "editor.toolbar.underline",
            "editor.toolbar.clozeNew", "editor.add.previousField", "editor.add.nextField", "editor.add.htmlToggle"])
        #expect(!(bar.items ?? []).contains { $0.accessibilityIdentifier == "editor.add.keyboardDone" })
        // A basic note type hides the cloze control; with no focus nothing formats.
        #expect(items[3].isHidden)
        #expect(items[0...2].allSatisfy { !$0.isEnabled })
        #expect(page.webView.scrollView.keyboardDismissMode == .interactive)
    }
    #endif

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
