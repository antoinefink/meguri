import CoreBridge
import Darwin
import Foundation
import Testing
import WebKit
@testable import AppModel
@testable import Editor

@MainActor
@Suite(.serialized) struct FieldEditorTests {
    @Test func untrustedPayloadsRefuseBeforeDraftMutation() throws {
        let invalid: [[String: Any]] = [
            ["type": "focusLeave", "loadId": 1, "seq": 1, "direction": 0],
            ["type": "dirty", "loadId": true, "seq": 1, "ord": 0],
            ["type": "dirty", "loadId": 1, "seq": 1.5, "ord": 0],
            ["type": "dirty", "loadId": 1, "seq": 1, "ord": 8],
            ["type": "commit", "loadId": 1, "seq": 1, "ord": 0, "html": "x", "unchanged": 0, "reason": "idle"],
            ["type": "flush", "loadId": 1, "seq": 1, "commits": [["ord": 0, "html": "x", "unchanged": false, "extra": "y"]]],
            ["type": "dirty", "loadId": 1, "seq": 1, "ord": 0, "extra": "x"],
            ["type": "addNote", "loadId": 1, "seq": 1]
        ]
        for value in invalid {
            #expect(throws: (any Error).self) { try EditorBridgeMessage(value, ordinals: [0, 1, 2]) }
        }
    }

    @Test func realRuntimePreservesUntouchedBytesScopesStylesAndHoldsExactInput() async throws {
        let original = "<span  class='original'>untouched</span><br/>"
        let typed = "<style>header,section{display:none}</style><b>Daily</b>" +
            "<script>window.editorFieldRan=true</script>" +
            "<img src=\"q%22r.png\" onerror=\"window.editorHandlerRan=true\">"
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.type(),
            deck: .init(id: 9, fullName: "Daily", filtered: false), values: [original, "", "e\u{301}\u{1f}"])
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        host.webView.frame = CGRect(x: 0, y: 0, width: 500, height: 700)
        defer { host.dispose() }
        try await host.start()
        #expect(try await Self.replaceSource(host, ord: 1, value: typed))
        try await host.setMode(.rich, ord: 1)
        try await host.flush(hold: true)
        #expect(draft.values == [original, typed.replacingOccurrences(of: "q%22r", with: "q&quot;r"), "e\u{301}\u{1f}"])
        #expect(host.held)
        #expect(try await host.webView.callAsyncJavaScript("""
            return window.editorFieldRan !== true && window.editorHandlerRan !== true &&
                typeof window.webkit?.messageHandlers?.meguriEditor === 'undefined' &&
                [...document.querySelectorAll('header')].every(header => getComputedStyle(header).display !== 'none');
            """, contentWorld: .page) as? Bool == true)
        // Even a synthetic beforeinput is cancelled while held. No later
        // source change can overtake the snapshot used for an add or close.
        let admittedWhileHeld = try await Self.replaceSource(host, ord: 0, value: "must not replace")
        #expect(!admittedWhileHeld)
        try await host.flush(hold: true)
        #expect(draft.values[0] == original)
        try await host.release()
        try await host.setMode(.html, ord: 1)
        #expect(try await Self.replaceSource(host, ord: 1, value: "<b>unclosed"))
        try await host.flush(hold: true)
        #expect(draft.values[1] == "<b>unclosed</b>")
        let next = try draft.nextAfterAcknowledgedAdd()
        #expect(next.values == ["", "", "e\u{301}\u{1f}"])
        #expect(next.deck == draft.deck && next.notetype == draft.notetype)
        #expect(next.unchangedSinceLastAdd)
        #expect(!draft.unchangedSinceLastAdd)
    }

    @Test func unsupportedOcclusionAndObsoleteHostDoNotLoadOrReinterpretFields() async throws {
        let deck = DeckNameEntry(id: 9, fullName: "Daily", filtered: false)
        #expect(throws: (any Error).self) {
            try NoteEditorDraft(generation: 1, notetype: Self.type(stockKind: 6), deck: deck)
        }
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.type(), deck: deck, values: ["kept", "", ""])
        let host = try FieldEditorHost(draft: draft, isCurrent: { false })
        defer { host.dispose() }
        await #expect(throws: (any Error).self) { try await host.start() }
        #expect(draft.values == ["kept", "", ""] && host.webView.url == nil)
    }

    @Test(arguments: ["owner", "dispose", "failed", "process"])
    func heldActualConversionCannotOutliveItsOwnerOrLoad(_ invalidation: String) async throws {
        let original = "<span  class='original'>kept</span><br/>"
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.type(),
            deck: .init(id: 9, fullName: "Daily", filtered: false), values: ["", original, "sticky"])
        var current = true
        let host = try FieldEditorHost(draft: draft, isCurrent: { current })
        let hold = HeldResult()
        var replacement: FieldEditorHost?
        defer { hold.release(); replacement?.dispose(); host.dispose() }
        try await host.start()
        try #require(try await Self.replaceSource(host, ord: 0, value: "<b>accepted</b>"))
        try await host.flush(hold: false)
        let accepted = draft.fields
        draft.afterConvertingFields = { await hold.suspend() }
        try #require(try await Self.replaceSource(host, ord: 1, value: "must not commit"))
        let oldFlush = Task { () -> Bool in
            do { try await host.flush(hold: true); return true } catch { return false }
        }
        try await hold.wait()
        let actualDrain = try #require(host.processingCompletion)
        #expect(draft.fields == accepted && draft.pendingFields == [1])
        switch invalidation {
        case "owner": current = false
        case "dispose": host.dispose()
        case "failed": try await Self.failPayload(host)
        default: host.webViewWebContentProcessDidTerminate(host.webView)
        }
        draft.afterConvertingFields = nil
        if invalidation == "process" {
            try await host.recoverCommittedDraft()
            #expect(!hold.expired && host.editable && host.error == nil)
            #expect(draft.fields == accepted && draft.pendingFields.isEmpty)
            try #require(try await Self.replaceSource(host, ord: 1, value: "new load"))
            try await host.flush(hold: true)
        } else if invalidation == "owner" {
            // The old owner has not been disposed. Its late guard must not
            // touch a new owner's already accepted state in the same draft.
            let next = try FieldEditorHost(draft: draft, isCurrent: { true })
            replacement = next
            try await next.start()
            try #require(try await Self.replaceSource(next, ord: 1, value: "new owner"))
            try await next.flush(hold: true)
        }
        let kept = draft.fields, dirty = draft.pendingFields
        hold.release()
        await actualDrain.value
        #expect(await oldFlush.value == false)
        #expect(!hold.expired && draft.fields == kept && draft.pendingFields == dirty)
        #expect(draft.values[0] == "<b>accepted</b>" && draft.values[2] == "sticky")
        if invalidation == "process" {
            #expect(host.editable && host.held && host.error == nil && draft.values[1] == "new load")
        } else if let replacement {
            #expect(replacement.editable && replacement.held && replacement.error == nil && draft.values[1] == "new owner")
        } else {
            #expect(draft.values[1] == original && draft.pendingFields == [1] && !host.editable)
        }
    }

    @Test func failedLoadRefusesSameLoadMessagesAndImplicitRestart() async throws {
        let draft = try NoteEditorDraft(generation: 1, notetype: Self.type(),
            deck: .init(id: 9, fullName: "Daily", filtered: false), values: ["kept", "", ""])
        let host = try FieldEditorHost(draft: draft, isCurrent: { true })
        defer { host.dispose() }
        try await host.start()
        try await Self.failPayload(host)
        let accepted = try await host.webView.callAsyncJavaScript("""
            const message = await meguriEditorRuntime.receive({type:'flush', loadId:1, hold:false});
            try { await window.webkit.messageHandlers.meguriEditor.postMessage(message); return true; }
            catch { return false; }
            """, contentWorld: .world(name: "Meguri.FieldEditor.v1"))
        #expect(accepted as? Bool == false)
        await #expect(throws: (any Error).self) { try await host.start() }
        await #expect(throws: (any Error).self) { try await host.recoverCommittedDraft() }
        #expect(draft.values == ["kept", "", ""] && !host.editable)
    }

    private static func failPayload(_ host: FieldEditorHost) async throws {
        _ = try await host.webView.callAsyncJavaScript("""
            try { await window.webkit.messageHandlers.meguriEditor.postMessage({type:'invalid'}); }
            catch { }
            """, contentWorld: .world(name: "Meguri.FieldEditor.v1"))
    }

    // A bounded suspension of an actual result, shared only by tests in this
    // file. Expiry releases the result so a regression reports instead of hangs.
    @MainActor final class HeldResult {
        private var held: CheckedContinuation<Void, Never>?
        private var admission: CheckedContinuation<Void, any Error>?
        private var arrived = false
        private var released = false
        private var timeout: Task<Void, Never>?
        private(set) var expired = false
        func suspend() async {
            guard !released else { return }
            await withCheckedContinuation {
                held = $0; arrived = true; admission?.resume(); admission = nil
            }
        }
        func wait() async throws {
            timeout = Task { [weak self] in
                do { try await Task.sleep(for: .seconds(20)) } catch { return }
                guard let self else { return }
                expired = true
                let waiter = admission; admission = nil
                waiter?.resume(throwing: EditorRuntimeError.unavailable)
                release()
            }
            if arrived { return }
            try await withCheckedThrowingContinuation { admission = $0 }
        }
        func release() {
            released = true; timeout?.cancel(); timeout = nil
            let result = held; held = nil; result?.resume()
        }
    }

    static func replaceSource(_ host: FieldEditorHost, ord: Int, value: String) async throws -> Bool {
        let result = try await host.webView.callAsyncJavaScript("""
            const source = document.querySelectorAll('textarea')[ord];
            const before = new InputEvent('beforeinput', {bubbles:true, cancelable:true, inputType:'insertText', data:value});
            if (!source.dispatchEvent(before)) return false;
            source.value = value;
            source.dispatchEvent(new InputEvent('input', {bubbles:true, inputType:'insertText', data:value}));
            return true;
            """, arguments: ["ord": ord, "value": value], in: nil, contentWorld: .page)
        return result as? Bool == true
    }
    private static func type(stockKind: Int32 = 0) -> NotetypeRecord {
        let fields = ["Prompt", "Meaning", "Source"].enumerated().map { index, name in
            NotetypeField(ord: UInt32(index), id: Int64(index), name: name, sticky: index == 2,
                rtl: false, fontName: "", fontSize: 20, description: "", plainText: true,
                collapsed: false, excludeFromSearch: false)
        }
        return .init(id: 42, name: "Three fields", kind: 0, sortField: 0, css: "", base: [], fields: fields, templates: [], originalStockKind: stockKind)
    }
}

// Reuse the existing serialized real-host suite; do not introduce another
// parallel owner of CoreBridge's process-wide host or alter its fixtures.
extension ReviewCollectionStartupTests {
    @Test @MainActor func preparedAddDraftFlushFeedsOneDurableU27WriteAndExistingStudy() async throws {
        let temp = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
        let root = URL(filePath: String(cString: temp)).appending(path: UUID().uuidString)
        free(temp)
        let directory = root.appending(path: "collection"), backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let core = CoreCollectionService()
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget"), manualTransferDirectory: root.appending(path: "transfers"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: backups)
        let sync = SyncCoordinator(directory: directory, configuration: nil, defaults: nil)
        let navigation = AppNavigation()
        let owner = ReviewCollection(configuration: configuration, navigation: navigation, sync: sync,
            providerDirectory: root.appending(path: "transfers"))
        var editor: FieldEditorHost?
        do {
            await owner.start()
            try #require(owner.state == .noFile)
            await owner.startEmpty()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            let deck = try await CoreDeckService().addDeck(name: "Daily Add")
            await owner.loadCounts()
            try #require(await owner.openDeckPage(deckID: deck.id).value)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == 0)
            let preparation = owner.addNoteDraft
            await preparation.prepare(presetDeck: deck.id)
            let draft = try #require(preparation.draft)
            #expect(draft.deck.id == deck.id && draft.generation == owner.generation)
            #expect(draft.notetype == (try await CoreNotetypeService().readNotetype(id: draft.notetype.id)))
            // Hold the actual final lifecycle result in the existing real-Core
            // scope. Defaults must not replace input arriving at that boundary,
            // even input undone to blank, or replace an obsolete owner's draft.
            for change in ["edited", "undone", "owner"] {
                let hold = FieldEditorTests.HeldResult()
                defer { hold.release(); preparation.afterReadingLifecycle = nil }
                preparation.afterReadingLifecycle = { await hold.suspend() }
                let request = Task { await preparation.prepare(presetDeck: deck.id) }
                try await hold.wait()
                if change == "owner" {
                    owner.setForeground(false)
                } else {
                    draft.markDirty(0)
                    try await draft.accept([.init(ord: 0, html: "input during defaults", unchanged: false)],
                        ifCurrent: { owner.foreground && owner.generation == draft.generation })
                    if change == "undone" {
                        try await draft.accept([.init(ord: 0, html: "", unchanged: true)], ifCurrent: { true })
                    } else { draft.markDirty(0) }
                }
                hold.release()
                await request.value
                #expect(!hold.expired && preparation.draft === draft)
                if change == "edited" {
                    #expect(draft.values[0] == "input during defaults" && draft.pendingFields == [0])
                } else { #expect(draft.isBlank) }
                preparation.afterReadingLifecycle = nil
                if change == "owner" {
                    owner.setForeground(true)
                    await owner.waitForLifecycle()
                    try #require(owner.foreground && !owner.resuming && owner.generation == draft.generation)
                } else {
                    try await draft.accept([.init(ord: 0, html: "", unchanged: true)], ifCurrent: { true })
                }
            }
            let host = try FieldEditorHost(draft: draft, isCurrent: { owner.state == .ready && owner.generation == draft.generation })
            editor = host
            try await host.start()
            try await host.setMode(.html, ord: 0)
            try #require(try await FieldEditorTests.replaceSource(host, ord: 0, value: "<b>Daily note</b>"))
            try await host.flush(hold: true)
            let check = try await CoreNoteService().checkNoteFields(notetypeId: draft.notetype.id, fields: draft.values, ownNote: nil)
            try #require(check.state == .normal)
            let added = try await CoreNoteService().addNote(.init(notetypeId: draft.notetype.id, fields: draft.values,
                tags: draft.tags, deckId: draft.deck.id, guid: nil, updateAddDefaults: true))
            #expect(added.written && !added.cards.isEmpty && added.summary.generation == draft.generation)
            let record = try await CoreNoteService().readNote(id: added.id)
            #expect(record.fields == draft.values && record.notetypeId == draft.notetype.id)
            let defaults = try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id)
            #expect(defaults.deckId == deck.id && defaults.notetypeId == draft.notetype.id)
            let next = try draft.nextAfterAcknowledgedAdd()
            #expect(next.unchangedSinceLastAdd && next.deck.id == deck.id)
            host.dispose(); editor = nil
            await owner.loadCounts()
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == UInt32(added.cards.count))
            #expect(navigation.selectedDeck?.rawValue == deck.id)
            await owner.startStudy(deckID: deck.id)
            let item = try #require(owner.review?.item)
            #expect(added.cards.contains(item.cardId))
            await owner.finishReview()
            try #require(await owner.close())
            // Cold reopen proves U27 durability rather than a draft echo.
            try await core.open(location, randomSeed: 190196)
            #expect(try await CoreNoteService().readNote(id: added.id).fields == record.fields)
            try await core.close()
        } catch {
            editor?.dispose(); _ = await owner.close(); try? await core.close(); throw error
        }
    }
}
