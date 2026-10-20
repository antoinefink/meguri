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
    public var settled: Bool { runtimeSettled && appearancePaints == 0 && !restoringText }
    public internal(set) var visibleToken: UInt64?
    public private(set) var closed = false
    public private(set) var terminations = 0
    public private(set) lazy var hostReports = CardHostReporting(page: self)
    public private(set) lazy var behavior = CardPageBehavior(page: self)
    var accessibilityEffects = CardAccessibilityEffects()
    public internal(set) var accessibilityTarget: CardAccessibilityTarget?
    public var onAccessibilityTarget: ((CardAccessibilityTarget) -> Void)?
    public internal(set) var typedValue: CardTypedValue?
    public var onTypedText: ((CardTypedValue) -> Void)?
    var renderedHTML: String?
    var retainedTypedText: String?
    private var reconstructionRequired = false
    private var restoringText = false
    private(set) var documentID = UUID()
    var needsReconstruction: Bool { reconstructionRequired }
    var contentProcessTerminated: Bool { processTerminated }
    var livenessEvaluation: ((@escaping @MainActor (Result<Bool, any Error>) -> Void) -> Void)?
    var livenessCancellations: [UUID: () -> Void] = [:]
    var behaviorAvailable: Bool { presented && !webView.idle && !closed && !reconstructionRequired && !processTerminated && !hostSuspended && !readyPhases.contains("failed") }
    private var hostInstalledDOM = false
    private var hostMutations = 0
    private var hostSuspended = false
    private var presentationRevision = UUID()
    private var hostPresentationPending = false
    var hostDOMKnown: Bool { hostInstalledDOM && hostMutations == 0 && !processTerminated && !hostSuspended && !hostPresentationPending && !restoringText }
    public var onTermination: ((CardPage) -> Void)?
    var onProcessTermination: ((CardPage) -> Void)?
    var onSignal: ((CardPage, String, [String: Any]) -> Void)?
    var onAction: ((CardPage, CardBridgeAction) -> Void)?
    var onConfirmation: ((CardPage, [String: Any]) -> Void)?
    private let configuration: CardWebConfiguration
    private var appearance: CardRuntime.Appearance
    private var questionAppearance: CardRuntime.Appearance?
    private var runtimeSettled = false
    private var processTerminated = false
    private var appearancePaints = 0
    private var loaded = false
    private var navigation: CheckedContinuation<Void, any Error>?
    private var navigationTarget: WKNavigation?
    private var rootLoadPending = false
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
    private var acknowledgements: [UUID: CheckedContinuation<Bool, any Error>] = [:]
    private var stringReads: [UUID: CheckedContinuation<String, any Error>] = [:]

    public nonisolated enum Failure: Error { case closed, superseded, updateFailed, notPrepared }
    static let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)

    init(configuration: CardWebConfiguration, resolver: MediaPathResolver?, platform: CardRuntime.Platform,
         appearance: CardRuntime.Appearance, reviewExtra: String, identifier: String, size: CGSize,
         diagnostics: CardDiagnostics, makeWebView: (CGRect, WKWebViewConfiguration) -> CardNativeWebView) {
        self.configuration = configuration
        self.platform = platform
        self.appearance = appearance
        self.diagnostics = diagnostics
        webView = makeWebView(CGRect(origin: .zero, size: size), configuration.configuration)
        super.init()
        webView.navigationDelegate = self
        webView.uiDelegate = behavior
        webView.setIdle(true)
        webView.applyAppearance(appearance)
        #if os(macOS)
        webView.setAccessibilityIdentifier(identifier)
        webView.autoresizingMask = [.width, .height]
        #else
        webView.accessibilityIdentifier = identifier
        webView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        webView.isOpaque = true
        webView.scrollView.contentInsetAdjustmentBehavior = .never
        webView.scrollView.automaticallyAdjustsScrollIndicatorInsets = false
        #endif
        let document = CardRuntime.pageDocument(platform: platform, appearance: appearance, mediaRoot: MediaOrigin.url, reviewExtra: reviewExtra)
        configuration.media.bind(webView, to: .init(resolver: resolver, document: document, diagnostics: diagnostics))
        configuration.bridge.register(self)
        configuration.hostBridge.register(self)
        webView.onHostGeometryChange = { [weak self] in
            guard let self else { return }
            if self.webView.window == nil { self.behavior.cancel() }
            self.hostReports.geometryChanged()
        }
    }

    public func load() async throws {
        guard !closed, !processTerminated else { throw Failure.closed }
        if let loadResult { return try loadResult.get() }
        if loading == nil {
            let documentID = documentID
            loading = Task { [weak self] in
                guard let self else { return }
                do {
                    try await self.configuration.ready()
                    guard !self.closed, self.documentID == documentID else { throw Failure.closed }
                    try await withCheckedThrowingContinuation { continuation in
                        self.navigation = continuation
                        self.rootLoadPending = true
                        self.navigationTarget = self.webView.load(URLRequest(url: MediaOrigin.url))
                    }
                    guard self.documentID == documentID else { return }
                    self.loaded = true
                    try await self.applyAppearance(self.appearance, forcePaint: false)
                    try await self.run(CardLayoutContinuity.source, world: Self.world)
                    guard self.documentID == documentID else { return }
                    self.finishLoad(.success(()))
                } catch {
                    guard self.documentID == documentID else { return }
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

    func begin(_ payload: CardPayload, update: CardUpdate, preservingVisit: Bool = false) {
        cancelLivenessProbes()
        behavior.cancel()
        accessibilityTarget = nil
        if update.side == .question && !preservingVisit {
            retainedTypedText = nil
            #if !os(macOS)
            webView.scrollView.setZoomScale(webView.scrollView.minimumZoomScale, animated: false)
            #endif
        }
        typedValue = nil
        renderedHTML = nil
        hostInstalledDOM = false
        hostReports.invalidate()
        updateOperation?.cancel()
        updateOperation = nil
        failWaiters(Failure.superseded)
        self.payload = payload
        self.update = update
        typedValue = retainedTypedText.map { CardTypedValue(update: update, text: $0) }
        runtimeSettled = false
        readyPhases.removeAll()
        if appearancePaints == 0 { readyPhases.insert("appearance") }
        visibleToken = nil
        visibleReceipt = nil
        pending.removeAll()
        configuration.media.setUpdate(update, for: webView)
    }

    func start(html: String, answer: String? = nil, update: CardUpdate, reconstruction: Bool = false) async throws {
        let preceding = operations
        let rendering = Task { [weak self] in
            _ = try? await preceding?.value
            guard let self, !self.closed, self.update == update else { throw Failure.superseded }
            try await self.load()
            guard !self.closed, self.update == update else { throw Failure.superseded }
            self.hostSuspended = false
            self.hostPresentationPending = false
            self.renderedHTML = html
            let identityArguments: [String: Any] = ["keyboardAllowed": self.webView.reviewOwnsFocus && !reconstruction,
                                                     "card": update.card, "generation": update.generation]
            try await self.run("meguriCardLayout.bind(); meguriCard.setFocusOwnership(keyboardAllowed); meguriCard.identity(card, generation);",
                               arguments: identityArguments, world: Self.world)
            guard !self.closed, self.update == update else { throw Failure.superseded }
            var entry: [String: Any] = ["flag": self.payload?.flag ?? 0, "mark": self.payload?.marked ?? false]
            let bodyStyle = CardRuntime.bodyClasses(platform: self.platform, appearance: self.appearance, cardOrdinal: self.payload?.ordinal)
            if update.side == .question {
                self.questionAppearance = self.appearance
                entry["q"] = html
                entry["a"] = answer ?? ""
                entry["classes"] = bodyStyle
                try await self.run("_showQuestion(q, a, classes); _drawFlag(flag); _drawMark(mark);", arguments: entry)
            } else {
                entry["a"] = html
                entry["classes"] = reconstruction ? bodyStyle : ""
                try await self.run("_showAnswer(a, classes); _drawFlag(flag); _drawMark(mark);", arguments: entry)
            }
        }
        operations = rendering
        updateOperation = rendering
        try await rendering.value
        guard !closed, self.update == update else { throw Failure.superseded }
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
        guard update != nil, !closed, !processTerminated, !readyPhases.contains("failed") else { return }
        if signal == "settled" || signal == "shown" {
            hostInstalledDOM = true
            if signal == "settled" { runtimeSettled = true }
            reach(signal)
            webView.callAsyncJavaScript("meguriCardLayout.ready();", arguments: [:], in: nil, in: Self.world) { _ in }
        } else if signal == "visible" {
            visibleReceipt = update
        } else if signal == "updateFailed" {
            hostInstalledDOM = false; hostReports.invalidate()
            readyPhases.insert("failed")
            runtimeSettled = false; visibleToken = nil
            diagnostics.record("updateFailed", update: update)
            failWaiters(Failure.updateFailed)
        } else if signal == "typedText", behaviorAvailable, let text = body["text"] as? String {
            captureTypedText(text)
        }
        onSignal?(self, signal, body)
    }
    private func reach(_ phase: String) {
        readyPhases.insert(phase)
        for id in waiters.keys.filter({ waiters[$0]?.1 == phase }) { waiters.removeValue(forKey: id)?.2.resume() }
    }
    func confirm(_ body: [String: Any]) {
        guard !readyPhases.contains("failed") else { return }
        if body["phase"] as? String == "visible", visibleReceipt != update { return }
        onConfirmation?(self, body)
    }
    func acceptsVisibleConfirmation() -> Bool { !readyPhases.contains("failed") && presented && update != nil && visibleReceipt == update }

    func admit(_ action: CardBridgeAction) {
        guard let update, !reconstructionRequired, !processTerminated, !readyPhases.contains("failed") else { return }
        if presented { onAction?(self, action) }
        else if action == .reveal || Self.isPlay(action) {
            if pending.count < 16 { pending.append((update, action)) }
        }
    }
    private static func isPlay(_ action: CardBridgeAction) -> Bool { if case .play = action { return true }; return false }

    func leavePresentation() {
        presented = false; pending.removeAll()
        suspendHost()
    }
    func beginPresentation() {
        // Native swap has already happened. Preserve only this slot's matched
        // preparation observations, never input or maps.
        hostSuspended = false
        hostPresentationPending = true
        hostReports.invalidate(keepSlot: true)
    }

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
        guard !closed, !readyPhases.contains("failed"), let update, visibleToken == token else { return }
        presented = true
        try await run("meguriCard.setFocusOwnership(ownsFocus); meguriCard.present(token);",
                      arguments: ["ownsFocus": webView.reviewOwnsFocus, "token": token], world: Self.world)
        guard self.update == update, presented, visibleToken == token, !closed else { return }
        hostPresentationPending = false
        await hostReports.publish()
        try Task.checkCancellation()
        guard self.update == update, presented, visibleToken == token, !closed, !readyPhases.contains("failed") else { return }
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

    func suspendHost() {
        presentationRevision = UUID()
        cancelLivenessProbes()
        behavior.cancel()
        hostSuspended = true
        hostReports.invalidate()
    }

    func suspendPresentation() {
        suspendHost()
        presented = false
        pending.removeAll()
    }

    func unpresent() async throws {
        suspendPresentation()
        guard !closed, !processTerminated else { return }
        try await run("meguriCard.unpresent();", world: Self.world)
    }
    func pauseMediaWithoutWaiting() {
        guard !closed, !processTerminated else { return }
        webView.callAsyncJavaScript("for (const media of document.querySelectorAll('audio,video')) media.pause();",
                                    arguments: [:], in: nil, in: Self.world) { _ in }
    }
    func pauseMedia() async throws {
        guard !closed, !processTerminated else { return }
        try await run("for (const media of document.querySelectorAll('audio,video')) media.pause();", world: Self.world)
    }
    func resumePresentation() async throws {
        try Task.checkCancellation()
        let revision = presentationRevision
        guard !closed, !readyPhases.contains("failed"), let update, let token = visibleToken else { return }
        presented = true
        try await run("meguriCard.setFocusOwnership(ownsFocus); meguriCard.resume(token);",
                      arguments: ["ownsFocus": webView.reviewOwnsFocus, "token": token], world: Self.world)
        try Task.checkCancellation()
        guard self.update == update, presented, visibleToken == token, !closed, !readyPhases.contains("failed") else { return }
        guard presentationRevision == revision else { throw Failure.superseded }
        hostSuspended = false
        hostPresentationPending = false
        await hostReports.publish()
        try Task.checkCancellation()
        guard self.update == update, presented, visibleToken == token, !closed, !hostSuspended,
              !readyPhases.contains("failed") else { throw Failure.superseded }
    }

    public func setAppearance(_ value: CardRuntime.Appearance) async throws {
        let update = self.update
        try await applyAppearance(value, forcePaint: false)
        await hostReports.publish()
        try Task.checkCancellation()
        guard self.update == update, !closed else { throw Failure.superseded }
    }
    private func applyAppearance(_ value: CardRuntime.Appearance, forcePaint: Bool) async throws {
        let changed = appearance != value || forcePaint
        if changed { beginHostMutation() }
        defer { if changed { endHostMutation() } }
        let paint = loaded && !processTerminated && update != nil && !presented && (appearance != value || forcePaint)
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
            if loaded && !processTerminated { try await run("meguriCard.setAppearance(dark, mac);", arguments: ["dark": value == .dark, "mac": platform == .mac], world: Self.world) }
            if paint { try await run("await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));", world: Self.world) }
        } catch {
            hostInstalledDOM = false
            hostReports.invalidate()
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
        beginHostMutation()
        let update = self.update
        payload?.flag = flag; payload?.marked = marked
        do {
            if loaded && !processTerminated { try await run("_drawFlag(flag); _drawMark(mark);", arguments: ["flag": flag, "mark": marked]) }
        } catch {
            endHostMutation()
            throw error
        }
        endHostMutation()
        await hostReports.publish()
        try Task.checkCancellation()
        guard self.update == update, !closed else { throw Failure.superseded }
    }
    func setAudioPolicy(_ policy: CardAudioPolicy) { payload?.audioPolicy = policy }

    func beginHostMutation() {
        hostMutations += 1
        hostReports.invalidate()
    }
    func endHostMutation() {
        hostMutations -= 1
    }

    func setFocusOwnership(_ ownsFocus: Bool) {
        webView.reviewOwnsFocus = ownsFocus
        guard loaded, !closed, !processTerminated else { return }
        // The native keyboard owner permits automatic answer-field
        // focus. Explicit DOM focus and ordinary card interaction stay intact.
        webView.callAsyncJavaScript("meguriCard.setFocusOwnership(ownsFocus);", arguments: ["ownsFocus": ownsFocus],
                                    in: nil, in: Self.world) { _ in }
    }

    func invalidate() {
        cancelLivenessProbes()
        behavior.cancel()
        hostInstalledDOM = false; hostReports.invalidate()
        update = nil; payload = nil; visibleToken = nil; runtimeSettled = false
        typedValue = nil; retainedTypedText = nil; renderedHTML = nil; accessibilityTarget = nil
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
        if loaded && !processTerminated {
            webView.callAsyncJavaScript("meguriCard.unpresent();", arguments: [:], in: nil, in: Self.world) { _ in }
        }
        closed = true
        presented = false
        invalidate()
        hostReports.close()
        webView.onHostGeometryChange = nil
        configuration.hostBridge.unregister(self)
        configuration.bridge.unregister(self)
        configuration.media.unbind(webView)
        finishLoad(.failure(Failure.closed))
        loading?.cancel(); loading = nil
        operations?.cancel(); operations = nil
        updateOperation?.cancel(); updateOperation = nil
        let pendingScripts = scripts, pendingReads = stringReads, pendingAcks = acknowledgements
        acknowledgements.removeAll()
        self.scripts.removeAll(); stringReads.removeAll()
        for continuation in pendingScripts.values { continuation.resume(throwing: Failure.closed) }
        for continuation in pendingReads.values { continuation.resume(throwing: Failure.closed) }
        for continuation in pendingAcks.values { continuation.resume(throwing: Failure.closed) }
        navigation?.resume(throwing: Failure.closed); navigation = nil
        navigationTarget = nil
        webView.stopLoading()
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        onTermination = nil
        onProcessTermination = nil
        onTypedText = nil; onAccessibilityTarget = nil
        onSignal = nil; onAction = nil; onConfirmation = nil
    }

    func run(_ source: String, arguments: [String: Any] = [:], world: WKContentWorld = .page) async throws {
        guard !closed else { throw Failure.closed }
        guard !processTerminated else { throw Failure.updateFailed }
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
        guard !processTerminated else { throw Failure.updateFailed }
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

    /// A real document/runtime round trip for collection-free performance
    /// warm-up. Shared storage/configuration does not guarantee process reuse.
    public func answersWarmup() async -> Bool {
        guard !closed, !processTerminated else { return false }
        do {
            let answer = try await string("""
                return document.readyState === 'complete' &&
                    document.getElementById('qa') !== null && document.body.offsetHeight > 0 &&
                    typeof _showQuestion === 'function' && typeof getTypedAnswer === 'function'
                    ? 'review-runtime-ready' : '';
                """)
            let isolated = try await string("return typeof meguriCard === 'object' ? 'isolated-ready' : '';", world: Self.world)
            return !closed && !processTerminated && answer == "review-runtime-ready" && isolated == "isolated-ready"
        } catch { return false }
    }

    func hostAcknowledgement(_ message: [String: Any]) async throws -> Bool {
        guard !closed else { throw Failure.closed }
        guard !processTerminated else { throw Failure.updateFailed }
        let id = UUID()
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await withCheckedThrowingContinuation { continuation in
                acknowledgements[id] = continuation
                webView.callAsyncJavaScript("""
                    const host = window.meguriReviewHost;
                    if (!host || typeof host.receive !== 'function') return false;
                    const acknowledgement = host.receive(message);
                    return typeof acknowledgement === 'boolean' && acknowledgement;
                    """, arguments: ["message": message], in: nil, in: Self.world) { [weak self] result in
                    self?.acknowledgements.removeValue(forKey: id)?.resume(with: result.map { CardBridge.bool($0) == true })
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.acknowledgements.removeValue(forKey: id)?.resume(throwing: CancellationError()) }
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
    public func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                        decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        guard !closed, webView === self.webView, let url = navigationAction.request.url else { decisionHandler(.cancel); return }
        if rootLoadPending, navigationAction.targetFrame?.isMainFrame == true, url == MediaOrigin.url {
            rootLoadPending = false
            decisionHandler(.allow)
            return
        }
        // A frame cannot navigate the root, open native UI or escape it.
        guard navigationAction.sourceFrame.isMainFrame, navigationAction.sourceFrame.request.url == MediaOrigin.url else { decisionHandler(.cancel); return }
        // WebKit calls the UI delegate for new windows. That delegate uses
        // the identical policy and always returns nil (exactly two pages).
        if navigationAction.targetFrame == nil { decisionHandler(.allow); return }
        guard navigationAction.targetFrame?.isMainFrame == true else { decisionHandler(.cancel); return }
        let route = behavior.route(url)
        decisionHandler(route == .javascript ? .allow : .cancel)
    }
    public func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse,
                        decisionHandler: @escaping @MainActor (WKNavigationResponsePolicy) -> Void) {
        decisionHandler(!closed && webView === self.webView && navigationResponse.isForMainFrame && navigationResponse.response.url == MediaOrigin.url ? .allow : .cancel)
    }
    public func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        if navigation === navigationTarget { navigationFailed() }
    }
    public func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        if navigation === navigationTarget { navigationFailed() }
    }
    public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        guard !closed, !processTerminated, webView === self.webView else { return }
        behavior.cancel()
        cancelLivenessProbes()
        terminations += 1
        processTerminated = true
        reconstructionRequired = true
        documentID = UUID()
        hostInstalledDOM = false
        hostReports.disable()
        runtimeSettled = false; visibleToken = nil; visibleReceipt = nil
        pending.removeAll()
        updateOperation?.cancel(); operations?.cancel(); loading?.cancel()
        drainJavaScript(Failure.updateFailed)
        finishLoad(.failure(Failure.updateFailed))
        readyPhases.insert("failed")
        navigationFailed(); failWaiters(Failure.updateFailed)
        diagnostics.record("processTerminated", update: update)
        onProcessTermination?(self)
        onTermination?(self)
    }
    private func navigationFailed() { navigation?.resume(throwing: Failure.updateFailed); navigation = nil; navigationTarget = nil }

    func captureTypedText(_ text: String) {
        guard let update, !closed, !processTerminated else { return }
        retainedTypedText = text
        let value = CardTypedValue(update: update, text: text)
        typedValue = value
        onTypedText?(value)
    }

    /// Only a confirmed termination permits skeleton reconstruction. Keep
    /// the native page, resolver binding and shared configuration in place.
    func prepareReplacementDocument(restoreValue: Bool = false) throws {
        guard reconstructionRequired, !closed else { throw Failure.superseded }
        behavior.cancel()
        cancelLivenessProbes()
        documentID = UUID()
        loading?.cancel()
        operations?.cancel()
        updateOperation?.cancel()
        navigationFailed()
        finishLoad(.failure(Failure.superseded))
        failWaiters(Failure.superseded)
        drainJavaScript(Failure.superseded)
        loading = nil
        operations = nil
        updateOperation = nil
        loadResult = nil
        loaded = false
        rootLoadPending = false
        presented = false
        processTerminated = false
        reconstructionRequired = restoreValue
        restoringText = restoreValue
        hostInstalledDOM = false
        hostSuspended = false
        hostPresentationPending = false
        runtimeSettled = false
        readyPhases.removeAll()
    }

    private func drainJavaScript(_ error: any Error) {
        let oldScripts = scripts, reads = stringReads, acks = acknowledgements
        scripts.removeAll(); stringReads.removeAll(); acknowledgements.removeAll()
        for item in oldScripts.values { item.resume(throwing: error) }
        for item in reads.values { item.resume(throwing: error) }
        for item in acks.values { item.resume(throwing: error) }
    }

    func restoreTypedText(_ text: String?, for update: CardUpdate) async throws {
        guard self.update == update, !closed else { throw Failure.superseded }
        if let text {
            try await run("meguriCard.restoreTypedText(text);", arguments: ["text": text], world: Self.world)
        }
        guard self.update == update, !closed else { throw Failure.superseded }
        retainedTypedText = text
        typedValue = text.map { CardTypedValue(update: update, text: $0) }
        restoringText = false
    }

    func presentRecovered(token: UInt64, update: CardUpdate, text: String?) async throws {
        guard self.update == update, !closed else { throw Failure.superseded }
        visibleToken = token
        presented = true
        // resume discards media held by recovery scripts. Focus ownership
        // remains off until the stock shown hook has completed.
        try await run("meguriCard.resume(token);", arguments: ["token": token], world: Self.world)
        try await waitForShown(update)
        guard self.update == update, !closed, presented else { throw Failure.superseded }
        try await restoreTypedText(text, for: update)
        setFocusOwnership(webView.reviewOwnsFocus)
        hostSuspended = false; hostPresentationPending = false
        await hostReports.publish()
    }

    func finishRecovery() { reconstructionRequired = false }
    private func cancelLivenessProbes() {
        let pending = livenessCancellations
        livenessCancellations.removeAll()
        for cancel in pending.values { cancel() }
    }
}
