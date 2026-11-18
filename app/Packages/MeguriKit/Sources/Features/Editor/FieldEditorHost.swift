import AppModel
import Foundation
import Instrumentation
import Observation
import OSLog
import SwiftUI
import WebKit

/// One web view per draft, retained above SwiftUI layout changes. The only
/// data bridge is in a named content world and admits its main frame only.
/// This component deliberately offers no Add/Edit/Browse persistence command.
@Observable @MainActor
public final class FieldEditorHost: NSObject, WKNavigationDelegate, AddNoteFieldRuntime {
    public let draft: NoteEditorDraft
    public let webView: WKWebView
    public private(set) var editable = false
    public private(set) var held = false
    public private(set) var error: String?
    public private(set) var contentHeight: Double = 0
    public private(set) var canUndo = false
    public private(set) var canRedo = false
    /// The native containing form supplies the next/previous focus target.
    public var focusLeave: ((Bool) -> Void)?
    private let isCurrent: @MainActor () -> Bool
    private let document: EditorDocument
    private let relay: EditorMessageRelay
    private static let world = WKContentWorld.world(name: "Meguri.FieldEditor.v1")
    private static let log = Logger(subsystem: "com.antoinefink.meguri", category: "Editor")
    private var loadID: UInt64 = 0
    private var activeLoad: UInt64?
    private var openedLoad: UInt64?
    private static let maximumID: UInt64 = 9_007_199_254_740_991
    private var nextSequence: UInt64 = 1
    private var pending: [UInt64: Pending] = [:]
    private var draining: Task<Void, Never>?
    private var drainingLoad: UInt64?
    private var startup: Task<Void, any Error>?
    private var ready: CheckedContinuation<Void, any Error>?
    private var readyTimeout: Task<Void, Never>?
    private var gapTimeout: Task<Void, Never>?
    private var interval: IntervalToken?
    private var allowsDocumentNavigation = false
    private var documentNavigation: WKNavigation?
    private var closed = false
    private var commandLoad: UInt64?
    private var processing: Pending?
    private var processTerminated = false
    // Tests can join the actual drain after releasing a held Core result.
    var processingCompletion: Task<Void, Never>? { draining }
    @MainActor private final class Pending {
        let message: EditorBridgeMessage
        private var completion: ((Result<Void, any Error>) -> Void)?
        init(message: EditorBridgeMessage, complete: @escaping (Result<Void, any Error>) -> Void) {
            self.message = message; completion = complete
        }
        func settle(_ result: Result<Void, any Error>) {
            let complete = completion; completion = nil; complete?(result)
        }
    }

    public init(draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) throws {
        self.draft = draft; self.isCurrent = isCurrent
        document = try EditorDocument(resources: .module); relay = EditorMessageRelay()
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.mediaTypesRequiringUserActionForPlayback = .all
        configuration.setURLSchemeHandler(document, forURLScheme: "meguri-editor")
        configuration.userContentController.addScriptMessageHandler(relay, contentWorld: Self.world, name: "meguriEditor")
        configuration.userContentController.addUserScript(WKUserScript(source: document.script, injectionTime: .atDocumentEnd,
            forMainFrameOnly: true, in: Self.world))
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        relay.owner = self; webView.navigationDelegate = self
    }

    public func start() async throws {
        if let startup, let expected = activeLoad {
            try requireCurrent(expected)
            try await startup.value
            try requireCurrent(expected)
            return
        }
        // Only explicit process recovery may replace a failed load.
        guard loadID == 0 else { throw EditorRuntimeError.unavailable }
        try await startLoad()
    }

    private func startLoad() async throws {
        guard !closed, isCurrent() else { throw EditorRuntimeError.unavailable }
        guard loadID < Self.maximumID else { throw EditorRuntimeError.unavailable }
        loadID += 1
        let expected = loadID
        activeLoad = expected; nextSequence = 1
        let work = Task { [self] in
            try requireCurrent(expected)
            interval = Signposts.begin(IntervalNames.editorOpenToEditable)
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                ready = continuation; allowsDocumentNavigation = true
                readyTimeout = Task { [weak self] in
                    do { try await Task.sleep(for: .seconds(10)) } catch { return }
                    self?.fail(EditorRuntimeError.unavailable, load: expected)
                }
                documentNavigation = webView.load(URLRequest(url: EditorDocument.url))
            }
            try requireCurrent(expected)
            try await openDraft(load: expected)
        }
        startup = work
        do { try await work.value; try requireCurrent(expected) }
        catch { fail(error, load: expected); throw error }
    }

    private func openDraft(load expected: UInt64) async throws {
        try requireCurrent(expected)
        let display = try await draft.displayValues()
        try requireCurrent(expected)
        let fields: [[String: Any]] = draft.fields.enumerated().map { index, field in
            var family = field.definition.fontName
            if family.hasSuffix(" L") { family.removeLast(2); family += " Light" }
            if !family.isEmpty && family.range(of: #"^[a-z-]+$"#, options: .regularExpression) == nil {
                // A CSS string token, assigned to a property rather than parsed
                // as a stylesheet. Preserve quotes and slashes in font names.
                family = "\"" + family.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
            }
            if !family.isEmpty { family += ", " }
            family += "-apple-system"
            var value: [String: Any] = ["ord": index, "name": field.definition.name,
                "html": display.targets[index], "description": field.definition.description,
                "fontFamily": family, "fontSize": field.definition.fontSize == 0 ? 20 : field.definition.fontSize,
                "rtl": field.definition.rtl, "collapsed": field.collapsed, "mode": field.mode.rawValue,
                "clozeField": draft.notetype.clozeFields.contains(UInt32(index))]
            if field.changed { value["baselineHtml"] = display.baselines[index] }
            return value
        }
        let focus = draft.focusedField ?? draft.fields.indices.first(where: { !draft.fields[$0].collapsed })
        openedLoad = expected
        _ = try await call(["type": "openNote", "protocol": 1, "loadId": expected, "mode": "add", "fields": fields,
            "focus": focus.map { $0 as Any } ?? NSNull(), "theme": [String: String](),
            "options": ["closeHTMLTags": false, "shrinkImages": false]], load: expected)
        try requireCurrent(expected)
        guard editable else { throw EditorRuntimeError.unavailable }
        held = false
    }

    /// Resolve all earlier sequence numbers before returning the flushed draft
    /// targets. A caller must not submit a write before this returns.
    public func flush(hold: Bool) async throws {
        guard editable, let expected = activeLoad, commandLoad == nil else { throw EditorRuntimeError.unavailable }
        try requireCurrent(expected)
        commandLoad = expected
        defer { if commandLoad == expected { commandLoad = nil } }
        do {
            let result = try await call(["type": "flush", "loadId": expected, "hold": hold], load: expected)
            try requireCurrent(expected)
            guard let result else { throw EditorRuntimeError.invalidMessage }
            let message = try EditorBridgeMessage(result, ordinals: Set(draft.fields.indices))
            guard message.type == "flush", message.loadID == expected else { throw EditorRuntimeError.invalidMessage }
            try await enqueue(message, load: expected)
            try requireCurrent(expected)
            held = hold || held
        } catch { fail(error, load: expected); throw error }
    }

    public func release() async throws {
        guard editable, let expected = activeLoad else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "release", "loadId": expected], load: expected)
        try requireCurrent(expected); held = false
    }
    public func setFocus(_ ord: Int) async throws {
        guard editable, !held, let expected = activeLoad, draft.fields.indices.contains(ord) else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "setFocus", "loadId": expected, "ord": ord], load: expected)
    }
    public func setMode(_ mode: NoteEditorDraft.Mode, ord: Int) async throws {
        guard editable, !held, let expected = activeLoad, draft.fields.indices.contains(ord) else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "setFieldState", "loadId": expected, "ord": ord, "mode": mode.rawValue], load: expected)
    }

    /// Recovery is explicit so a containing flow can explain the possible
    /// loss of the last uncommitted input. It never rebases the draft.
    public func recoverCommittedDraft() async throws {
        guard processTerminated, !editable, !closed, isCurrent() else { throw EditorRuntimeError.unavailable }
        guard activeLoad == nil, loadID < Self.maximumID else { throw EditorRuntimeError.unavailable }
        // Revocation already settled the old processing item. Recovery must
        // not wait indefinitely for its outstanding Core conversion.
        draft.acknowledgeProcessLoss()
        processTerminated = false
        startup = nil
        try await startLoad()
    }

    private func requireCurrent(_ expected: UInt64) throws {
        guard !closed, activeLoad == expected else { throw EditorRuntimeError.unavailable }
        guard isCurrent() else {
            fail(EditorRuntimeError.unavailable, load: expected)
            throw EditorRuntimeError.unavailable
        }
    }

    private func call(_ message: [String: Any], load expected: UInt64) async throws -> Any? {
        try requireCurrent(expected)
        // Structured arguments preserve quotes, </script>, Unicode and HTML.
        // No part of field content is ever evaluated as JavaScript source.
        do {
            let result = try await webView.callAsyncJavaScript("return await meguriEditorRuntime.receive(message);",
                arguments: ["message": message], in: nil, contentWorld: Self.world)
            try requireCurrent(expected)
            return result
        } catch { fail(error, load: expected); throw error }
    }

    fileprivate func receive(_ scriptMessage: WKScriptMessage, reply: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        guard !closed, scriptMessage.name == "meguriEditor", scriptMessage.webView === webView, scriptMessage.frameInfo.isMainFrame,
              scriptMessage.frameInfo.securityOrigin.protocol == "meguri-editor",
              scriptMessage.frameInfo.securityOrigin.host == "editor", webView.url == EditorDocument.url else {
            Self.log.error("Rejected editor message origin"); reply(nil, "Invalid editor origin"); return
        }
        guard let expected = activeLoad else { reply(nil, "Editor load revoked"); return }
        do {
            try requireCurrent(expected)
            let message = try EditorBridgeMessage(scriptMessage.body, ordinals: Set(draft.fields.indices))
            if message.type == "ready" {
                guard let ready else { throw EditorRuntimeError.invalidMessage }
                self.ready = nil; readyTimeout?.cancel(); readyTimeout = nil
                reply(["accepted": true], nil); ready.resume(); return
            }
            if let incoming = message.loadID, incoming < expected { reply(["ignored": true], nil); return }
            guard openedLoad == expected, message.loadID == expected else { throw EditorRuntimeError.invalidMessage }
            insert(message, load: expected) { result in
                switch result {
                case .success: reply(["accepted": true], nil)
                case .failure: reply(nil, "Editor message refused")
                }
            }
        } catch {
            Self.log.error("Rejected editor payload"); reply(nil, "Invalid editor payload"); fail(error, load: expected)
        }
    }

    private func enqueue(_ message: EditorBridgeMessage, load expected: UInt64) async throws {
        try await withCheckedThrowingContinuation { continuation in
            insert(message, load: expected) { continuation.resume(with: $0) }
        }
    }
    private func insert(_ message: EditorBridgeMessage, load expected: UInt64,
                        complete: @escaping (Result<Void, any Error>) -> Void) {
        do { try requireCurrent(expected) }
        catch { complete(.failure(error)); return }
        guard openedLoad == expected, let sequence = message.sequence, message.loadID == expected, sequence >= nextSequence,
              sequence < Self.maximumID, sequence - nextSequence <= 256,
              pending[sequence] == nil, processing?.message.sequence != sequence else {
            complete(.failure(EditorRuntimeError.sequenceGap)); fail(EditorRuntimeError.sequenceGap, load: expected); return
        }
        pending[sequence] = Pending(message: message, complete: complete)
        guard draining == nil else { return }
        drainingLoad = expected
        draining = Task { [self] in
            defer {
                if drainingLoad == expected { draining = nil; drainingLoad = nil; processing = nil }
            }
            while activeLoad == expected, let item = pending.removeValue(forKey: nextSequence) {
                processing = item
                do {
                    try requireCurrent(expected)
                    try await apply(item.message, load: expected)
                    try requireCurrent(expected)
                    nextSequence += 1; processing = nil; item.settle(.success(()))
                    gapTimeout?.cancel(); gapTimeout = nil
                }
                catch { item.settle(.failure(error)); fail(error, load: expected); return }
            }
            if activeLoad == expected, !pending.isEmpty, gapTimeout == nil {
                gapTimeout = Task { [weak self] in
                    do { try await Task.sleep(for: .seconds(10)) } catch { return }
                    self?.fail(EditorRuntimeError.sequenceGap, load: expected)
                }
            }
        }
    }
    private func apply(_ message: EditorBridgeMessage, load expected: UInt64) async throws {
        try requireCurrent(expected)
        switch message.type {
        case "loaded":
            editable = true; error = nil
            if let interval { Signposts.end(interval); self.interval = nil }
            checkFields(load: expected)
        case "commit", "flush":
            try await draft.accept(message.commits, ifCurrent: { [weak self] in
                guard let self else { return false }
                return (try? self.requireCurrent(expected)) != nil
            })
            try requireCurrent(expected)
            checkFields(load: expected)
        case "dirty": draft.markDirty(EditorBridgeMessage.integer(message.body["ord"])!)
        case "focus": draft.setFocus(EditorBridgeMessage.integer(message.body["ord"]))
        case "blur": break
        case "modeChanged": draft.setMode(NoteEditorDraft.Mode(rawValue: message.body["mode"] as! String)!, ord: EditorBridgeMessage.integer(message.body["ord"])!)
        case "collapsedChanged": draft.setCollapsed(EditorBridgeMessage.boolean(message.body["collapsed"])!, ord: EditorBridgeMessage.integer(message.body["ord"])!)
        case "historyState":
            if draft.focusedField == EditorBridgeMessage.integer(message.body["ord"]) {
                canUndo = EditorBridgeMessage.boolean(message.body["canUndo"])!
                canRedo = EditorBridgeMessage.boolean(message.body["canRedo"])!
            }
        case "focusLeave": draft.setFocus(nil); focusLeave?(message.body["direction"] as? String == "previous")
        case "contentHeight": contentHeight = EditorBridgeMessage.number(message.body["height"])!
        default: throw EditorRuntimeError.invalidMessage
        }
    }
    private func checkFields(load expected: UInt64) {
        Task { [weak self] in
            guard let self, (try? requireCurrent(expected)) != nil else { return }
            await draft.checkFields(ifCurrent: { [weak self] in
                guard let self else { return false }
                return (try? self.requireCurrent(expected)) != nil
            })
        }
    }
    private func fail(_ failure: any Error, load expected: UInt64) {
        guard activeLoad == expected else { return }
        activeLoad = nil; openedLoad = nil; allowsDocumentNavigation = false; documentNavigation = nil
        error = String(describing: failure); editable = false
        commandLoad = nil
        readyTimeout?.cancel(); readyTimeout = nil
        gapTimeout?.cancel(); gapTimeout = nil
        let waiter = ready; ready = nil; waiter?.resume(throwing: failure)
        let buffered = pending.values; pending.removeAll()
        let actual = processing; processing = nil
        draining?.cancel(); draining = nil; drainingLoad = nil
        actual?.settle(.failure(failure))
        for item in buffered { item.settle(.failure(failure)) }
        if let interval { Signposts.end(interval); self.interval = nil }
    }

    /// Final disposal follows the containing owner's held flush/close guard.
    /// It never deletes targets or writes a note. Do not call on layout change.
    public func dispose() {
        guard !closed else { return }
        closed = true
        if let expected = activeLoad { fail(EditorRuntimeError.unavailable, load: expected) }
        relay.owner = nil; webView.stopLoading(); webView.navigationDelegate = nil
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "meguriEditor", contentWorld: Self.world)
    }
    public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        guard webView === self.webView, !closed, let expected = activeLoad else { return }
        processTerminated = true; fail(EditorRuntimeError.processTerminated, load: expected)
    }
    public func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        if webView === self.webView, let expected = activeLoad, let navigation,
           navigation === documentNavigation { fail(error, load: expected) }
    }
    public func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        if webView === self.webView, let expected = activeLoad, let navigation,
           navigation === documentNavigation { fail(error, load: expected) }
    }
    public func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                        decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        let allowed = allowsDocumentNavigation && navigationAction.targetFrame?.isMainFrame == true &&
            navigationAction.request.url == EditorDocument.url && navigationAction.navigationType == .other
        if allowed { allowsDocumentNavigation = false }
        decisionHandler(allowed ? .allow : .cancel)
    }
}

private final class EditorMessageRelay: NSObject, WKScriptMessageHandlerWithReply {
    weak var owner: FieldEditorHost?
    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage,
                               replyHandler: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        guard let owner else { replyHandler(nil, "Editor closed"); return }
        owner.receive(message, reply: replyHandler)
    }
}

/// A mount for the retained view, with WebKit as the sole scroll/focus owner
/// for fields. The surrounding native form must not wrap it in a ScrollView.
public struct FieldEditorView: View {
    public let host: FieldEditorHost
    public init(host: FieldEditorHost) { self.host = host }
    public var body: some View {
        VStack(spacing: 0) {
            FieldEditorMount(host: host)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("editor.fields")
    }
}
#if os(macOS)
private struct FieldEditorMount: NSViewRepresentable {
    let host: FieldEditorHost
    func makeNSView(context: Context) -> WKWebView { host.webView }
    func updateNSView(_ nsView: WKWebView, context: Context) {}
}
#else
private struct FieldEditorMount: UIViewRepresentable {
    let host: FieldEditorHost
    func makeUIView(context: Context) -> WKWebView { host.webView }
    func updateUIView(_ uiView: WKWebView, context: Context) {}
}
#endif

// The collection retains this actual host through the AppModel runtime boundary.
// A remounted form reads it; it must not create/dispose a host on appear/disappear.
extension ReviewCollection {
    @discardableResult public func prepareAddNoteEditor(presetDeck: Int64? = nil) async -> FieldEditorHost? {
        let flow = addNoteFlow
        guard await flow.open(presetDeck: presetDeck, makeRuntime: { draft, current in
            try FieldEditorHost(draft: draft, isCurrent: current)
        }) else { return nil }
        return flow.runtime as? FieldEditorHost
    }
}
