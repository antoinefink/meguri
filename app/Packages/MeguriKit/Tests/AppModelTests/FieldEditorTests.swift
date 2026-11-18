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
        let owner = CurrentOwner()
        let host = try FieldEditorHost(draft: draft, isCurrent: { owner.isCurrent })
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
        case "owner": owner.isCurrent = false
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

    @MainActor private final class CurrentOwner {
        var isCurrent = true
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
@MainActor private final class FailedAddStartup: AddNoteFieldRuntime {
    static let failure = BridgeError.unavailable(reason: "Synthetic field startup failure.")
    let draft: NoteEditorDraft
    let isCurrent: @MainActor () -> Bool
    var beforeFailure: (@MainActor () async throws -> Void)?
    private(set) var starts = 0
    private(set) var flushes = 0
    private(set) var disposals = 0
    init(_ draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) {
        self.draft = draft; self.isCurrent = isCurrent
    }
    func start() async throws { starts += 1; try await beforeFailure?(); throw Self.failure }
    func flush(hold: Bool) throws { flushes += 1; throw Self.failure }
    func release() {}
    func setFocus(_ ord: Int) { draft.setFocus(ord) }
    func dispose() { disposals += 1 }
}

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
            await owner.refreshDeckPage(try #require(owner.deckPageRefreshRequest))
            #expect(owner.emptyDeckPageID == deck.id)
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
            // An invalid explicit deck must not become a write to Default.
            #expect(await owner.prepareAddNoteEditor(presetDeck: -195) == nil)
            #expect(preparation.draft === draft)
            let flow = owner.addNoteFlow
            let undoBeforeStartup = try await CoreUndoService().peekUndo(scope: .global)
            var rejectedStartup: FailedAddStartup?
            #expect(await flow.open(presetDeck: deck.id) { draft, current in
                draft.setTags(["startup-retained"])
                draft.setMode(.html, ord: 0)
                let failed = FailedAddStartup(draft, isCurrent: current)
                rejectedStartup = failed
                return failed
            } == false)
            let failed = try #require(rejectedStartup)
            let retainedDraft = failed.draft, retainedFields = failed.draft.fields, retainedTags = failed.draft.tags
            #expect(retainedDraft.isBlank && flow.draft === retainedDraft)
            #expect(!flow.isPresented && flow.runtime == nil && !flow.canSave && !failed.isCurrent())
            #expect(failed.starts == 1 && failed.disposals == 1)
            #expect(flow.failure == FailedAddStartup.failure && flow.error == String(describing: FailedAddStartup.failure))
            #expect(await flow.save() == nil)
            // An unpreset retry must mount a fresh real host above this exact
            // blank draft, rather than accept the failed presentation or defaults.
            let host = try #require(await owner.prepareAddNoteEditor())
            editor = host
            await flow.selectionRefreshCompletion?.value
            #expect(host.draft === retainedDraft && flow.runtime === host && host.editable && flow.isPresented)
            #expect(retainedDraft.fields == retainedFields && retainedDraft.tags == retainedTags)
            #expect(failed.starts == 1 && failed.disposals == 1 && !failed.isCurrent())
            #expect(flow.lastAdded == nil && flow.history.isEmpty)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).empty)
            #expect(try await CoreUndoService().peekUndo(scope: .global) == undoBeforeStartup)
            let savingDraft = try #require(flow.draft)
            #expect(flow.runtime === host && savingDraft === host.draft)
            // Reentering the action model preserves the actual editor owner.
            #expect(await owner.prepareAddNoteEditor(presetDeck: deck.id) === host)
            #expect(await flow.save() == nil)
            #expect(flow.refusal == .empty && flow.draft === savingDraft && !host.held)
            try await host.setMode(.html, ord: 0)
            try #require(try await FieldEditorTests.replaceSource(host, ord: 0, value: "<b>Daily note</b>"))
            savingDraft.setTags(["daily", "source::native"])
            // Hold an actual fresh field check. A changed deck identity must
            // invalidate this attempt, preserve the exact draft and release
            // input; a second activation while held must submit no write.
            let other = try await CoreDeckService().addDeck(name: "Other Add")
            let hold = FieldEditorTests.HeldResult()
            defer { hold.release(); flow.afterCheckingFields = nil }
            flow.afterCheckingFields = { await hold.suspend() }
            let obsolete = Task { await flow.save() }
            try await hold.wait()
            #expect(host.held && flow.saving)
            #expect(await flow.save() == nil)
            try savingDraft.selectDeck(.init(id: other.id, fullName: other.name, filtered: false))
            let keptValues = savingDraft.values, keptTags = savingDraft.tags
            hold.release()
            #expect(await obsolete.value == nil)
            #expect(!hold.expired && flow.draft === savingDraft && flow.runtime === host)
            #expect(flow.error != nil && flow.failure != nil && !host.held && !flow.saving)
            #expect(savingDraft.values == keptValues && savingDraft.tags == keptTags && savingDraft.deck.id == other.id)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == 0)
            #expect(try await CoreDeckService().studyState(deckID: other.id).listCounts.new == 0)
            flow.afterCheckingFields = nil
            try savingDraft.selectDeck(.init(id: deck.id, fullName: deck.name, filtered: false))
            let added = try #require(await flow.save())
            #expect(added.written && !added.cards.isEmpty && added.summary.generation == savingDraft.generation)
            #expect(flow.lastAdded == added && flow.history == [added.id])
            let record = try await CoreNoteService().readNote(id: added.id)
            #expect(record.fields == keptValues && record.tags == keptTags && record.notetypeId == savingDraft.notetype.id)
            let defaults = try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id)
            #expect(defaults.deckId == deck.id && defaults.notetypeId == savingDraft.notetype.id)
            let next = try #require(flow.draft)
            #expect(next !== savingDraft && next.unchangedSinceLastAdd && next.deck.id == deck.id && next.tags == keptTags)
            #expect(next.values == savingDraft.fields.map { $0.definition.sticky ? $0.target : "" })
            #expect(flow.runtime?.draft === next)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == UInt32(added.cards.count))
            #expect(owner.deckPage(deck.id)?.counts.new == UInt32(added.cards.count))
            #expect(owner.emptyDeckPageID == nil)
            #expect(navigation.selectedDeck?.rawValue == deck.id)
            try #require(await flow.requestClose())
            #expect(!flow.isPresented && flow.runtime == nil && flow.draft === next)
            editor = nil
            await owner.startStudy(deckID: deck.id)
            let item = try #require(owner.review?.item)
            #expect(added.cards.contains(item.cardId))
            await owner.finishReview()
            // The real durable write succeeds even when its new next-draft
            // host never starts. Retire only that host, retaining newer input
            // on the exact successor for Close and an explicit fresh-host retry.
            let remountInput = try NoteEditorDraft(generation: owner.generation, notetype: savingDraft.notetype,
                deck: savingDraft.deck, values: savingDraft.fields.indices.map { $0 == 0 ? "Remount durable input" : "" })
            preparation.replace(with: remountInput)
            var rejectedRemount: FailedAddStartup?
            #expect(await flow.open { draft, current in
                if draft === remountInput { return try FieldEditorHost(draft: draft, isCurrent: current) }
                let failed = FailedAddStartup(draft, isCurrent: current)
                failed.beforeFailure = {
                    try await draft.accept([.init(ord: 0, html: "New next-draft input", unchanged: false)], ifCurrent: current)
                    draft.setTags(["next-startup-retained"])
                    draft.setMode(.html, ord: 0)
                }
                rejectedRemount = failed
                return failed
            })
            editor = try #require(flow.runtime as? FieldEditorHost)
            let beforeRemount = try await CoreDeckService().studyState(deckID: deck.id).listCounts.new
            let undoBeforeRemount = try await CoreUndoService().peekUndo(scope: .global)
            let remountAdded = try #require(await flow.save())
            let remountFailed = try #require(rejectedRemount)
            let remountDraft = remountFailed.draft
            let remountFields = remountDraft.fields, remountTags = remountDraft.tags
            #expect(remountAdded.written && remountAdded.summary.generation == remountInput.generation)
            #expect(remountInput.submissionAcknowledged && remountDraft !== remountInput && flow.draft === remountDraft)
            #expect(remountDraft.values[0] == "New next-draft input" && remountTags == ["next-startup-retained"])
            #expect(remountDraft.notetype == remountInput.notetype && remountDraft.deck == remountInput.deck)
            #expect(!flow.isPresented && flow.runtime == nil && !flow.canSave && !remountFailed.isCurrent())
            #expect(remountFailed.starts == 1 && remountFailed.disposals == 1 && remountFailed.flushes == 0)
            #expect(flow.lastAdded == remountAdded && flow.history == [remountAdded.id, added.id])
            #expect(flow.failure == FailedAddStartup.failure && flow.error == String(describing: FailedAddStartup.failure))
            let remountRecord = try await CoreNoteService().readNote(id: remountAdded.id)
            #expect(remountRecord.fields == remountInput.values && remountRecord.tags == remountInput.tags)
            let undoAfterRemount = try await CoreUndoService().peekUndo(scope: .global)
            #expect(undoAfterRemount != undoBeforeRemount)
            #expect(await flow.save() == nil)
            #expect(await flow.requestClose())
            #expect(flow.draft === remountDraft && remountDraft.fields == remountFields && remountDraft.tags == remountTags)
            let remountRetry = try #require(await owner.prepareAddNoteEditor())
            editor = remountRetry
            await flow.selectionRefreshCompletion?.value
            #expect(flow.draft === remountDraft && remountRetry.draft === remountDraft && flow.runtime === remountRetry)
            #expect(flow.isPresented && remountRetry.editable && flow.canSave)
            #expect(remountDraft.fields == remountFields && remountDraft.tags == remountTags)
            #expect(remountFailed.starts == 1 && remountFailed.disposals == 1 && remountFailed.flushes == 0 && !remountFailed.isCurrent())
            #expect(flow.lastAdded == remountAdded && flow.history == [remountAdded.id, added.id])
            #expect(try await CoreNoteService().readNote(id: remountAdded.id) == remountRecord)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == beforeRemount + UInt32(remountAdded.cards.count))
            #expect(try await CoreUndoService().peekUndo(scope: .global) == undoAfterRemount)
            flow.generationEnded(); preparation.replace(with: nil); editor = nil
            // Hold a real committed reply at exactly the ownership boundary.
            // Each case writes once; revocation cannot turn that write into nil,
            // revive a disposed host or erase newer/different retained input.
            for change in ["revoked", "newer", "replacement", "currentNewer"] {
                let submitted = try NoteEditorDraft(generation: owner.generation, notetype: savingDraft.notetype,
                    deck: savingDraft.deck, values: savingDraft.fields.indices.map { $0 == 0 ? "Late ack \(change)" : "" })
                preparation.replace(with: submitted)
                let lateHost = try #require(await owner.prepareAddNoteEditor(presetDeck: deck.id))
                editor = lateHost
                let committed = FieldEditorTests.HeldResult()
                var reply: NoteMutation?
                defer { committed.release(); flow.afterAddingNote = nil }
                flow.afterAddingNote = { actual in reply = actual; await committed.suspend() }
                let before = try await CoreDeckService().studyState(deckID: deck.id).listCounts.new
                let save = Task { await flow.save() }
                try await committed.wait()
                let actual = try #require(reply)
                let durable = try await CoreNoteService().readNote(id: actual.id)
                #expect(actual.written && actual.summary.generation == submitted.generation)
                #expect(durable.fields == submitted.values && durable.notetypeId == submitted.notetype.id)
                #expect(await flow.save() == nil)
                if change != "currentNewer" { flow.generationEnded() }
                var retained: NoteEditorDraft?
                if change == "newer" || change == "currentNewer" {
                    try await submitted.accept([.init(ord: 0, html: "New input \(change)", unchanged: false)], ifCurrent: { true })
                    submitted.setTags(["newer-input"])
                    submitted.markDirty(1)
                    retained = submitted
                } else if change == "replacement" {
                    let successorDraft = try NoteEditorDraft(generation: owner.generation, notetype: submitted.notetype,
                        deck: submitted.deck, values: submitted.values, tags: ["successor"])
                    preparation.replace(with: successorDraft); retained = successorDraft
                }
                let retainedValues = retained?.values, retainedTags = retained?.tags, retainedPending = retained?.pendingFields
                committed.release()
                #expect(await save.value == actual)
                #expect(!committed.expired && !flow.saving && flow.draft === retained)
                #expect(retained?.values == retainedValues && retained?.tags == retainedTags && retained?.pendingFields == retainedPending)
                if change == "currentNewer" {
                    #expect(flow.runtime === lateHost && flow.isPresented && lateHost.editable && !lateHost.held)
                    #expect(flow.lastAdded == actual && flow.history == [actual.id])
                } else {
                    #expect(flow.runtime == nil && !flow.isPresented && !lateHost.editable && !flow.canSave)
                    #expect(flow.lastAdded == nil && flow.history.isEmpty)
                    #expect(await flow.save() == nil)
                }
                if change == "revoked" {
                    #expect(submitted.submissionAcknowledged)
                    // Even explicit stale presentation of this exact object
                    // cannot send its already committed revision again.
                    preparation.replace(with: submitted)
                    #expect(await owner.prepareAddNoteEditor(presetDeck: deck.id) == nil)
                    #expect(!flow.isPresented && flow.runtime == nil && !flow.canSave)
                    #expect(await flow.save() == nil)
                } else if change == "replacement" { #expect(submitted.submissionAcknowledged) }
                else { #expect(!submitted.submissionAcknowledged) }
                #expect(try await CoreNoteService().readNote(id: actual.id) == durable)
                #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == before + UInt32(actual.cards.count))
                flow.afterAddingNote = nil
                flow.generationEnded(); preparation.replace(with: nil); editor = nil
            }
            // D5 must disagree with both ID order and D1's remembered type.
            // Undo removes an actually stored selected type without raw SQL.
            let types = CoreNotetypeService(), undo = CoreUndoService()
            let zulu = try await types.addStockNotetype(kind: .basic, name: "Zulu D5")
            let alpha = try await types.addNotetype(.init(name: "aARdvark D5", cloze: false,
                fields: ["Back", "Prompt"], templates: [.init(name: "Prompt", front: "{{Back}}", back: "{{Prompt}}",
                    browserFront: "", browserBack: "")], css: nil))
            #expect(zulu.id < alpha.id)
            let chosen = try await types.readNotetype(id: alpha.id)
            let removed = try await types.addStockNotetype(kind: .basic, name: "Removed D5")
            let missing = try await types.readNotetype(id: removed.id)
            let remove = try await undo.startUndo(scope: .global)
            _ = try await remove.value()
            let names = try await types.notetypeNames()
            #expect(!names.contains { $0.id == removed.id })
            #expect(names.map(\.id) == names.map(\.id).sorted() && names.first?.id != alpha.id)
            let defaultsBeforeRepair = try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id)
            #expect(defaultsBeforeRepair.notetypeId != alpha.id)
            let missingDraft = try NoteEditorDraft(generation: owner.generation, notetype: missing,
                deck: savingDraft.deck, values: ["D5 mapped front", "D5 mapped back"], tags: ["d5-kept"])
            preparation.replace(with: missingDraft)
            let missingHost: FieldEditorHost = try #require(await owner.prepareAddNoteEditor(presetDeck: deck.id))
            editor = missingHost
            let beforeRepair = try await CoreDeckService().studyState(deckID: deck.id).listCounts.new
            let undoBeforeRepair = try await undo.peekUndo(scope: .global)
            let presentSelection = try await CoreEditorServices().addNotetypeSelection(
                expectedGeneration: owner.generation, selectedId: zulu.id)
            #expect(presentSelection.generation == owner.generation)
            #expect(presentSelection.notetype == (try await types.readNotetype(id: zulu.id)))
            #expect(await flow.save() == nil)
            let repaired = try #require(flow.draft)
            #expect(repaired !== missingDraft && repaired.notetype == chosen && repaired.notetype.base == chosen.base)
            #expect(repaired.values == ["D5 mapped back", "D5 mapped front"])
            #expect(repaired.tags == missingDraft.tags && repaired.deck == missingDraft.deck)
            #expect(flow.runtime?.draft === repaired && flow.error != nil && flow.history.isEmpty && flow.lastAdded == nil)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == beforeRepair)
            #expect(try await undo.peekUndo(scope: .global) == undoBeforeRepair)
            #expect(try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id) == defaultsBeforeRepair)
            let explicitAdd = try #require(await flow.save())
            let repairedRecord = try await CoreNoteService().readNote(id: explicitAdd.id)
            #expect(repairedRecord.notetypeId == alpha.id && repairedRecord.fields == repaired.values && repairedRecord.tags == repaired.tags)
            #expect(flow.history == [explicitAdd.id] && flow.lastAdded == explicitAdd)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == beforeRepair + UInt32(explicitAdd.cards.count))
            // A real relevant change with a still-valid selection must keep
            // the exact successor, runtime and last-add comparison intact.
            let validDraft = try #require(flow.draft)
            let validHost = try #require(flow.runtime)
            let validFields = validDraft.fields, validTags = validDraft.tags
            let validObservation = FieldEditorTests.HeldResult()
            defer { validObservation.release(); flow.afterReadingSelection = nil }
            flow.afterReadingSelection = { await validObservation.suspend() }
            let liveType = try await types.addStockNotetype(kind: .basic, name: "Removed live D5")
            try await validObservation.wait()
            let validRefresh = try #require(flow.selectionRefreshCompletion)
            validObservation.release()
            await validRefresh.value
            #expect(!validObservation.expired && flow.draft === validDraft && flow.runtime === validHost)
            #expect(validDraft.fields == validFields && validDraft.tags == validTags && validDraft.unchangedSinceLastAdd)
            #expect(flow.history == [explicitAdd.id] && flow.lastAdded == explicitAdd)
            flow.afterReadingSelection = nil
            try #require(await flow.requestClose())
            editor = nil
            let liveNotetype = try await types.readNotetype(id: liveType.id)
            let original = "<span  class='original'>live front</span><br/>"
            let liveDraft = try NoteEditorDraft(generation: owner.generation, notetype: liveNotetype,
                deck: savingDraft.deck, values: [original, "before held observation"], tags: ["live-kept"])
            preparation.replace(with: liveDraft)
            let liveHost = try #require(await owner.prepareAddNoteEditor(presetDeck: deck.id))
            editor = liveHost
            try await liveHost.setMode(.html, ord: 1)
            let liveObservation = FieldEditorTests.HeldResult()
            defer { liveObservation.release(); flow.afterReadingSelection = nil }
            flow.afterReadingSelection = { await liveObservation.suspend() }
            let removeLive = try await undo.startUndo(scope: .global)
            _ = try await removeLive.value()
            try await liveObservation.wait()
            let liveRefresh = try #require(flow.selectionRefreshCompletion)
            #expect(flow.draft === liveDraft && flow.runtime === liveHost && !liveHost.held)
            try #require(try await FieldEditorTests.replaceSource(liveHost, ord: 1, value: "<b>latest live back</b>"))
            liveDraft.setTags(["live-kept", "latest-input"])
            let zuluRecord = try await types.readNotetype(id: zulu.id)
            _ = try await types.renameNotetype(id: zulu.id, base: zuluRecord.base, name: "Zulu renamed D5")
            let beforeLiveRefresh = try await CoreDeckService().studyState(deckID: deck.id).listCounts.new
            let undoBeforeLiveRefresh = try await undo.peekUndo(scope: .global)
            let defaultsBeforeLiveRefresh = try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id)
            liveObservation.release()
            await liveRefresh.value
            let liveRepaired = try #require(flow.draft)
            #expect(!liveObservation.expired && liveRepaired !== liveDraft && liveRepaired.notetype == chosen)
            #expect(liveRepaired.values == ["<b>latest live back</b>", original])
            #expect(liveRepaired.tags == ["live-kept", "latest-input"] && liveRepaired.deck == liveDraft.deck)
            #expect(flow.runtime?.draft === liveRepaired && flow.runtime !== liveHost && flow.canSave)
            #expect(flow.lastAdded == explicitAdd && flow.history == [explicitAdd.id] && flow.error != nil)
            #expect(try await CoreDeckService().studyState(deckID: deck.id).listCounts.new == beforeLiveRefresh)
            #expect(try await undo.peekUndo(scope: .global) == undoBeforeLiveRefresh)
            #expect(try await CoreEditorServices().addDefaults(reviewCard: nil, presetDeck: deck.id) == defaultsBeforeLiveRefresh)
            flow.afterReadingSelection = nil
            // End this test-owned unsaved presentation before the cold reopen.
            flow.generationEnded(); preparation.replace(with: nil)
            editor = nil
            try #require(await owner.close())
            // Cold reopen reads the durable coordinator write from the collection.
            try await core.open(location, randomSeed: 190196)
            #expect(try await CoreNoteService().readNote(id: added.id).fields == record.fields)
            let reopened = try await core.waitAvailable(timeoutMilliseconds: 0)
            #expect(reopened != savingDraft.generation)
            await #expect(throws: BridgeError.staleGeneration(planned: savingDraft.generation, current: reopened)) {
                try await CoreEditorServices().addNotetypeSelection(expectedGeneration: savingDraft.generation, selectedId: alpha.id)
            }
            try await core.close()
        } catch {
            editor?.dispose(); owner.addNoteFlow.runtime?.dispose()
            _ = await owner.close(); try? await core.close(); throw error
        }
    }
}
