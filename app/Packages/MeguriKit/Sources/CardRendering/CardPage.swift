import CoreBridge
import Foundation
import WebKit

public final class CardPage: NSObject, WKNavigationDelegate {
    public let webView: CardNativeWebView
    public let platform: CardRuntime.Platform
    public let diagnostics: CardDiagnostics
    public private(set) var update: CardUpdate?
    public private(set) var payload: CardPayload?
    public private(set) var presented = false
    public var settled: Bool { runtimeSettled && appearancePaints == 0 }
    public internal(set) var visibleToken: UInt64?
    public private(set) var closed = false
    public private(set) var terminations = 0
    var onSignal: ((CardPage, String, [String: Any]) -> Void)?
    var onAction: ((CardPage, CardBridgeAction) -> Void)?
    var onConfirmation: ((CardPage, [String: Any]) -> Void)?
    private let configuration: CardWebConfiguration
    private var appearance: CardRuntime.Appearance
    private var questionAppearance: CardRuntime.Appearance?
    private var runtimeSettled = false
    private var appearancePaints = 0
    private var loaded = false
    private var navigation: CheckedContinuation<Void, any Error>?
    private var navigationTarget: WKNavigation?
    private var loading: Task<Void, Never>?
    private var loadResult: Result<Void, any Error>?
    private var loadWaiters: [UUID: CheckedContinuation<Void, any Error>] = [:]
    private var operations: Task<Void, any Error>?
    private var updateOperation: Task<Void, any Error>?
    private var waiters: [UUID: (CardUpdate, String, CheckedContinuation<Void, any Error>)] = [:]
    private var readyPhases: Set<String> = []
    private var pending: [(CardUpdate, CardBridgeAction)] = []
    private var visibleReceipt: CardUpdate?
    private var scripts: [UUID: CheckedContinuation<Void, any Error>] = [:]
    private var stringReads: [UUID: CheckedContinuation<String, any Error>] = [:]

    public nonisolated enum Failure: Error { case closed, superseded, updateFailed, notPrepared }
    static let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)

    init(configuration: CardWebConfiguration, resolver: MediaPathResolver, platform: CardRuntime.Platform,
         appearance: CardRuntime.Appearance, reviewExtra: String, identifier: String, size: CGSize,
         diagnostics: CardDiagnostics, makeWebView: (CGRect, WKWebViewConfiguration) -> CardNativeWebView) {
        self.configuration = configuration
        self.platform = platform
        self.appearance = appearance
        self.diagnostics = diagnostics
        webView = makeWebView(CGRect(origin: .zero, size: size), configuration.configuration)
        super.init()
        webView.navigationDelegate = self
        webView.setIdle(true)
        webView.applyAppearance(appearance)
        #if os(macOS)
        webView.setAccessibilityIdentifier(identifier)
        webView.autoresizingMask = [.width, .height]
        #else
        webView.accessibilityIdentifier = identifier
        webView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        webView.isOpaque = true
        #endif
        let document = CardRuntime.pageDocument(platform: platform, appearance: appearance, mediaRoot: MediaOrigin.url, reviewExtra: reviewExtra)
        configuration.media.bind(webView, to: .init(resolver: resolver, document: document, diagnostics: diagnostics))
        configuration.bridge.register(self)
    }

    public func load() async throws {
        guard !closed else { throw Failure.closed }
        if let loadResult { return try loadResult.get() }
        if loading == nil {
            loading = Task { [weak self] in
                guard let self else { return }
                do {
                    try await self.configuration.ready()
                    guard !self.closed else { throw Failure.closed }
                    try await withCheckedThrowingContinuation { continuation in
                        self.navigation = continuation
                        self.navigationTarget = self.webView.load(URLRequest(url: MediaOrigin.url))
                    }
                    self.loaded = true
                    try await self.setAppearance(self.appearance)
                    self.finishLoad(.success(()))
                } catch {
                    self.finishLoad(.failure(error))
                }
            }
        }
        let id = UUID()
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await withCheckedThrowingContinuation { continuation in
                if let loadResult { continuation.resume(with: loadResult) }
                else { loadWaiters[id] = continuation }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.loadWaiters.removeValue(forKey: id)?.resume(throwing: CancellationError()) }
        }
    }

    private func finishLoad(_ result: Result<Void, any Error>) {
        guard loadResult == nil else { return }
        loadResult = result
        let pending = loadWaiters.values
        loadWaiters.removeAll()
        for continuation in pending { continuation.resume(with: result) }
    }

    func begin(_ payload: CardPayload, update: CardUpdate) {
        updateOperation?.cancel()
        updateOperation = nil
        failWaiters(Failure.superseded)
        self.payload = payload
        self.update = update
        runtimeSettled = false
        readyPhases.removeAll()
        if appearancePaints == 0 { readyPhases.insert("appearance") }
        visibleToken = nil
        visibleReceipt = nil
        pending.removeAll()
        configuration.media.setUpdate(update, for: webView)
    }

    func start(html: String, answer: String? = nil, update: CardUpdate) async throws {
        let previous = operations
        let operation = Task { [weak self] in
            _ = try? await previous?.value
            guard let self, !self.closed, self.update == update else { throw Failure.superseded }
            try await self.load()
            guard self.update == update else { throw Failure.superseded }
            try await self.run("meguriCard.identity(card, generation);", arguments: ["card": update.card, "generation": update.generation], world: Self.world)
            // No other native operation can interleave identity and entry.
            if update.side == .question {
                self.questionAppearance = self.appearance
                let classes = CardRuntime.bodyClasses(platform: self.platform, appearance: self.appearance, cardOrdinal: self.payload?.ordinal)
                try await self.run("_showQuestion(q, a, classes); _drawFlag(flag); _drawMark(mark);",
                                   arguments: ["q": html, "a": answer ?? "", "classes": classes, "flag": self.payload?.flag ?? 0, "mark": self.payload?.marked ?? false])
            } else {
                try await self.run("_showAnswer(a);", arguments: ["a": html])
            }
        }
        operations = operation
        updateOperation = operation
        try await operation.value
        guard self.update == update, !closed else { throw Failure.superseded }
    }

    func waitForSettled(_ update: CardUpdate) async throws {
        try await waitFor("settled", update: update)
    }
    func waitForShown(_ update: CardUpdate) async throws {
        try await waitFor("shown", update: update)
    }
    private func waitFor(_ phase: String, update: CardUpdate) async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            guard self.update == update, !closed else { throw Failure.superseded }
            if readyPhases.contains("failed") { throw Failure.updateFailed }
            if readyPhases.contains(phase) { return }
            try await withCheckedThrowingContinuation { continuation in waiters[id] = (update, phase, continuation) }
        } onCancel: {
            Task { @MainActor [weak self] in
                self?.waiters.removeValue(forKey: id)?.2.resume(throwing: CancellationError())
            }
        }
    }

    func signal(_ signal: String, body: [String: Any]) {
        if signal == "settled" || signal == "shown" {
            if signal == "settled" { runtimeSettled = true }
            reach(signal)
        } else if signal == "visible" {
            visibleReceipt = update
        } else if signal == "updateFailed" {
            readyPhases.insert("failed")
            runtimeSettled = false; visibleToken = nil
            diagnostics.record("updateFailed", update: update)
            failWaiters(Failure.updateFailed)
        }
        onSignal?(self, signal, body)
    }
    private func reach(_ phase: String) {
        readyPhases.insert(phase)
        for id in waiters.keys.filter({ waiters[$0]?.1 == phase }) { waiters.removeValue(forKey: id)?.2.resume() }
    }
    func confirm(_ body: [String: Any]) {
        if body["phase"] as? String == "visible", visibleReceipt != update { return }
        onConfirmation?(self, body)
    }
    func acceptsVisibleConfirmation() -> Bool { presented && update != nil && visibleReceipt == update }

    func admit(_ action: CardBridgeAction) {
        guard let update, !readyPhases.contains("failed") else { return }
        if presented { onAction?(self, action) }
        else if action == .reveal || Self.isPlay(action) {
            if pending.count < 16 { pending.append((update, action)) }
        }
    }
    private static func isPlay(_ action: CardBridgeAction) -> Bool { if case .play = action { return true }; return false }

    func leavePresentation() { presented = false; pending.removeAll() }

    /// Ordered before the next native update even if its owner prepares the
    /// old page again in the same turn as the swap.
    func queueUnpresent() {
        let previous = operations
        operations = Task { [weak self] in
            _ = try? await previous?.value
            guard let self, !self.closed else { return }
            try await self.unpresent()
        }
    }

    /// Called only after native z-order and the enclosing commit.
    func present(token: UInt64) async throws {
        guard !closed, let update, visibleToken == token else { return }
        presented = true
        try await run("meguriCard.present(token);", arguments: ["token": token], world: Self.world)
        guard self.update == update, presented else { return }
        let actions = pending
        pending.removeAll()
        var revealed = false
        for (identity, action) in actions where identity == self.update && presented {
            if action == .reveal {
                guard !revealed, identity.side == .question else { continue }
                revealed = true
            }
            onAction?(self, action)
        }
    }

    func unpresent() async throws {
        presented = false
        pending.removeAll()
        try await run("meguriCard.unpresent();", world: Self.world)
    }
    func pauseMedia() async throws {
        try await run("for (const media of document.querySelectorAll('audio,video')) media.pause();", world: Self.world)
    }
    func resumePresentation() async throws {
        guard !closed, let token = visibleToken else { return }
        presented = true
        try await run("meguriCard.resume(token);", arguments: ["token": token], world: Self.world)
    }

    public func setAppearance(_ value: CardRuntime.Appearance) async throws {
        try await applyAppearance(value, forcePaint: false)
    }
    private func applyAppearance(_ value: CardRuntime.Appearance, forcePaint: Bool) async throws {
        let paint = loaded && update != nil && !presented && (appearance != value || forcePaint)
        if paint { appearancePaints += 1; readyPhases.remove("appearance") }
        defer {
            if paint {
                appearancePaints -= 1
                if appearancePaints == 0 { reach("appearance") }
            }
        }
        appearance = value
        webView.applyAppearance(value)
        do {
            if loaded { try await run("meguriCard.setAppearance(dark, mac);", arguments: ["dark": value == .dark, "mac": platform == .mac], world: Self.world) }
            if paint { try await run("await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));", world: Self.world) }
        } catch {
            if !presented { runtimeSettled = false }
            diagnostics.record("appearanceFailed", update: update)
            throw error
        }
    }
    func refreshAppearance() async throws {
        guard let update else { throw Failure.superseded }
        try await applyAppearance(appearance, forcePaint: questionAppearance != appearance)
        questionAppearance = appearance
        try await waitFor("appearance", update: update)
    }

    func setState(flag: UInt8, marked: Bool) async throws {
        payload?.flag = flag; payload?.marked = marked
        if loaded { try await run("_drawFlag(flag); _drawMark(mark);", arguments: ["flag": flag, "mark": marked]) }
    }
    func setAudioPolicy(_ policy: CardAudioPolicy) { payload?.audioPolicy = policy }

    func invalidate() {
        update = nil; payload = nil; visibleToken = nil; runtimeSettled = false
        readyPhases.removeAll()
        visibleReceipt = nil
        pending.removeAll()
        failWaiters(Failure.superseded)
    }

    func cancelUpdate() {
        invalidate()
        updateOperation?.cancel()
    }

    private func failWaiters(_ error: any Error) {
        let old = waiters.values
        waiters.removeAll()
        for (_, _, continuation) in old { continuation.resume(throwing: error) }
    }

    public func close() {
        guard !closed else { return }
        if loaded {
            webView.callAsyncJavaScript("meguriCard.unpresent();", arguments: [:], in: nil, in: Self.world) { _ in }
        }
        closed = true
        invalidate()
        configuration.bridge.unregister(self)
        configuration.media.unbind(webView)
        finishLoad(.failure(Failure.closed))
        loading?.cancel(); loading = nil
        operations?.cancel(); operations = nil
        updateOperation?.cancel(); updateOperation = nil
        let pendingScripts = scripts, pendingReads = stringReads
        self.scripts.removeAll(); stringReads.removeAll()
        for continuation in pendingScripts.values { continuation.resume(throwing: Failure.closed) }
        for continuation in pendingReads.values { continuation.resume(throwing: Failure.closed) }
        navigation?.resume(throwing: Failure.closed); navigation = nil
        navigationTarget = nil
        webView.stopLoading()
        webView.navigationDelegate = nil
        onSignal = nil; onAction = nil; onConfirmation = nil
    }

    func run(_ source: String, arguments: [String: Any] = [:], world: WKContentWorld = .page) async throws {
        guard !closed else { throw Failure.closed }
        let id = UUID()
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await withCheckedThrowingContinuation { continuation in
                scripts[id] = continuation
                webView.callAsyncJavaScript(source, arguments: arguments, in: nil, in: world) { [weak self] result in
                    self?.scripts.removeValue(forKey: id)?.resume(with: result.map { _ in () })
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.scripts.removeValue(forKey: id)?.resume(throwing: CancellationError()) }
        }
    }

    func string(_ source: String, arguments: [String: Any] = [:], world: WKContentWorld = .page) async throws -> String {
        guard !closed else { throw Failure.closed }
        let id = UUID()
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await withCheckedThrowingContinuation { continuation in
                stringReads[id] = continuation
                webView.callAsyncJavaScript(source, arguments: arguments, in: nil, in: world) { [weak self] result in
                    self?.stringReads.removeValue(forKey: id)?.resume(with: result.map { $0 as? String ?? "" })
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.stringReads.removeValue(forKey: id)?.resume(throwing: CancellationError()) }
        }
    }

    func typedText() async -> String {
        do { return try await string("return getTypedAnswer();") }
        catch { diagnostics.record("typedAnswerRead", update: update); return "" }
    }

    public func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard navigation === navigationTarget else { return }
        self.navigation?.resume(); self.navigation = nil; navigationTarget = nil
    }
    public func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        if navigation === navigationTarget { navigationFailed() }
    }
    public func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        if navigation === navigationTarget { navigationFailed() }
    }
    public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        terminations += 1
        readyPhases.insert("failed")
        navigationFailed(); failWaiters(Failure.updateFailed)
        diagnostics.record("processTerminated", update: update)
        onSignal?(self, "updateFailed", ["error": "processTerminated"])
    }
    private func navigationFailed() { navigation?.resume(throwing: Failure.updateFailed); navigation = nil; navigationTarget = nil }
}
