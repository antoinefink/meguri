#if MEGURI_TEST_HOOKS
import AppModel
import CardRendering
import DesignSystem
import Foundation
import QuartzCore
import SwiftUI
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

extension TestHooks {
    enum CardBehaviorRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("card/behaviors") { _ in .screen { CardBehaviorView() } }
        }
    }

    @Observable final class CardBehaviorModel {
        var pair: CardPagePair?
        var state = "loading"
        var narrow = false
        var dialogResult = "none"
        let presenter = NativeCardBehaviorPresenter()
        private var root: URL?
        private var card: Int64 = 0
        private var work: Task<Void, Never>?
        private var report = ""
        private var probes = 0
        private var answerForm = 0
        #if os(iOS)
        private var focusObservation: NSObjectProtocol?
        private var nativeVoiceFocus = "none"
        #endif

        func answerVariant(_ variant: Int) { answerForm = variant }

        func start(appearance: CardRuntime.Appearance) {
            guard work == nil, pair == nil else { return }
            #if os(iOS)
            focusObservation = NotificationCenter.default.addObserver(forName: UIAccessibility.elementFocusedNotification,
                                                                       object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    let element = UIAccessibility.focusedElement(using: .notificationVoiceOver) as? NSObject
                    self.nativeVoiceFocus = element?.accessibilityLabel ?? "none"
                    await self.refresh()
                }
            }
            #endif
            work = Task { [weak self] in
                guard let self else { return }
                do {
                    let root = FileManager.default.temporaryDirectory.appending(path: "card-behavior-\(UUID())", directoryHint: .isDirectory)
                    self.root = root
                    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
                    let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
                    try Task.checkCancellation()
                    let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, appearance: appearance)
                    self.pair = pair
                    pair.behaviorPresenter = self.presenter
                    pair.inspectorPolicy = CardBehaviorBuildPolicy.inspectorPolicy()
                    pair.onCommitted = { [weak self] _, _ in Task { await self?.refresh() } }
                    pair.onTypedText = { [weak self] _ in Task { await self?.refresh() } }
                    pair.onRecovery = { [weak self] _, _ in Task { await self?.refresh() } }
                    try await pair.load()
                    try await next()
                } catch { if !Task.isCancelled { state = "failed: \(error)" } }
            }
        }

        func next() async throws {
            guard let pair else { return }
            card += 1
            let title = String(localized: "Behavior question", table: "CardBehaviorRoutes")
            let answer = String(localized: "Behavior answer", table: "CardBehaviorRoutes")
            let bottom = String(localized: "Fixed card footer", table: "CardBehaviorRoutes")
            let inputLabel = String(localized: "Typed answer", table: "CardBehaviorRoutes")
            let jump = String(localized: "Jump to footer", table: "CardBehaviorRoutes")
            let scriptLink = String(localized: "Run card script", table: "CardBehaviorRoutes")
            let html = """
                <style>body{margin:0}input{font:inherit;min-height:44px;max-width:100%;box-sizing:border-box}
                a{display:inline-block;padding:16px}#viewport{height:100vh}#space{height:200vh}#bottom{position:fixed;bottom:0;margin:0}</style>
                <h1>\(title)</h1><input id=typeans aria-label="\(inputLabel)">
                <p><a href="#bottom">\(jump)</a> <a href="javascript:window.cardScriptRan=true;void(0)">\(scriptLink)</a></p>
                <div id=viewport></div><div id=space></div><p id=bottom>\(bottom)</p>
                <script>window.resizeEvents=0;window.addEventListener('resize',()=>resizeEvents++);window.domMarker=Math.random();</script>
                """
            let heading: String
            switch answerForm {
            case 1: heading = "<div style='height:1600px'></div><hr id=answer><h1>\(answer)</h1>"
            case 2: heading = "<h1>\(answer)</h1>"
            case 3: heading = "<div id=answer role=img aria-label=\"\(answer)\" style='width:160px;height:80px'></div>"
            default: heading = "<h1 id=answer>\(answer)</h1>"
            }
            #if os(iOS)
            nativeVoiceFocus = "none"
            #endif
            let payload = CardPayload(cardID: card, question: html, answer: heading + "<input id=typeans aria-label=\"\(inputLabel)\">")
            let update = try await pair.prepare(payload)
            CATransaction.begin()
            _ = try pair.present(update)
            CATransaction.commit()
        }

        func reveal() async throws { _ = try await pair?.showAnswer(); await refresh() }
        func zoom() { guard let pair else { return }; pair.cardZoom = CardZoom(percent: pair.cardZoom.percent == 100 ? 150 : 100); Task { await refresh() } }
        func suspend() async { await pair?.suspend(); await refresh() }
        func resume() async { await pair?.resume(); await refresh() }
        func probe() async { _ = await pair?.probeForegroundLiveness(); probes += 1; await refresh() }
        func terminationCallback() {
            guard let page = pair?.current else { return }
            // Public native callback seam: this is not an OS process kill.
            page.webViewWebContentProcessDidTerminate(page.webView)
        }
        func focusCard() async throws {
            guard let pair else { return }
            pair.ownsKeyboardFocus = true
            _ = try await pair.current.webView.callAsyncJavaScript("document.getElementById('typeans')?.focus();", arguments: [:], in: nil, contentWorld: .page)
        }
        func dialog(prompt: Bool) async throws {
            guard let page = pair?.current else { return }
            let message = String(localized: "Card dialog example", table: "CardBehaviorRoutes")
            let result = try await page.webView.callAsyncJavaScript(prompt ? "return prompt(message, '');" : "return confirm(message);",
                                                                  arguments: ["message": message], in: nil, contentWorld: .page)
            if let text = result as? String { dialogResult = text.isEmpty ? "empty" : text }
            else if let value = result as? Bool { dialogResult = value ? "true" : "false" }
            else { dialogResult = "cancelled" }
            await refresh()
        }
        func refresh() async {
            guard let pair, let update = pair.current.update else { return }
            let page = pair.current
            let result = try? await page.webView.callAsyncJavaScript("""
                return JSON.stringify({text:document.getElementById('typeans')?.value||'', marker:window.domMarker,
                  resizes:window.resizeEvents||0, height:innerHeight, layoutHeight:document.documentElement.clientHeight,
                  viewport:document.getElementById('viewport')?.offsetHeight,
                  scroll:scrollY, bottom:document.getElementById('bottom')?.getBoundingClientRect().bottom,
                  root:location.href, scriptRan:window.cardScriptRan===true,
                  ...(Number.isFinite(window.visualViewport?.scale) ? {visualScale:window.visualViewport.scale} : {})});
                """, arguments: [:], in: nil, contentWorld: .page)
            guard pair.current === page, page.update == update, !page.closed else { return }
            var observations = (result as? String).flatMap { $0.data(using: .utf8) }
                .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
            observations["pageZoom"] = page.webView.pageZoom
            observations["pageCount"] = pair.pages.count
            observations["distinctPages"] = Set(pair.pages.map { ObjectIdentifier($0.webView) }).count == pair.pages.count
            #if os(macOS)
            observations["currentHidden"] = page.webView.isAccessibilityHidden()
            observations["idleHidden"] = pair.idle.webView.isAccessibilityHidden()
            #else
            observations["currentHidden"] = page.webView.accessibilityElementsHidden
            observations["idleHidden"] = pair.idle.webView.accessibilityElementsHidden
            #endif
            #if os(iOS)
            observations["voiceFocus"] = nativeVoiceFocus
            let nativeZoom = page.webView.scrollView.zoomScale
            if nativeZoom.isFinite { observations["nativeZoom"] = nativeZoom }
            if let window = page.webView.window {
                let frame = page.webView.convert(page.webView.bounds, to: window)
                observations["native"] = ["x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height,
                                          "safeTop": window.safeAreaInsets.top, "safeBottom": window.safeAreaInsets.bottom,
                                          "safeLeft": window.safeAreaInsets.left, "safeRight": window.safeAreaInsets.right,
                                          "windowHeight": window.bounds.height, "windowWidth": window.bounds.width]
            }
            #endif
            report = (try? JSONSerialization.data(withJSONObject: observations)).flatMap { String(data: $0, encoding: .utf8) } ?? "unavailable"
            state = "ready card=\(update.card) side=\(update.sideName) generation=\(update.generation) zoom=\(Int(pair.cardZoom.percent)) dialog=\(dialogResult) target=\(page.accessibilityTarget?.elementID ?? "none") probe=\(probes) \(report)"
        }
        func perform(_ action: @escaping @MainActor () async throws -> Void) {
            Task { do { try await action() } catch { state = "failed: \(error)" } }
        }
        func stop() {
            #if os(iOS)
            if let focusObservation { NotificationCenter.default.removeObserver(focusObservation) }
            focusObservation = nil
            #endif
            work?.cancel(); work = nil
            pair?.close(); pair = nil
            if let root { try? FileManager.default.removeItem(at: root) }
            root = nil
        }
    }

    struct CardBehaviorView: View {
        @State private var model = CardBehaviorModel()
        @State private var nativeText = ""
        @FocusState private var nativeFocused: Bool
        @Environment(\.colorScheme) private var appearance
        #if os(iOS)
        @Environment(\.horizontalSizeClass) private var sizeClass
        #endif
        private var cardMaxWidth: CGFloat {
            if model.narrow { return ContentWidth.primaryAction }
            #if os(iOS)
            return sizeClass == .regular ? ContentWidth.readable : .infinity
            #else
            return ContentWidth.readable
            #endif
        }
        var body: some View {
            VStack(spacing: Spacing.xSmall) {
                // Native dialogs start from visible controls, after no menu
                // or other presentation is competing for the explicit host.
                control("Confirm", id: "confirm") { try await model.dialog(prompt: false) }
                control("Prompt", id: "prompt") { try await model.dialog(prompt: true) }
                control("Reveal answer", id: "reveal") { try await model.reveal() }
                Menu(String(localized: "Card actions", table: "CardBehaviorRoutes")) {
                    control("Answer separator", id: "separator") { model.answerVariant(1) }
                    control("Answer fallback", id: "fallback") { model.answerVariant(2) }
                    control("Answer graphic", id: "graphic") { model.answerVariant(3) }
                    control("Next card", id: "next") { try await model.next() }
                    control("Card zoom", id: "zoom") { model.zoom() }
                    control("Resize card", id: "resize") { model.narrow.toggle(); await model.refresh() }
                    control("Read card state", id: "refresh") { await model.refresh() }
                    control("Suspend card", id: "suspend") { await model.suspend() }
                    control("Resume card", id: "resume") { await model.resume() }
                    control("Probe pages", id: "probe") { await model.probe() }
                    control("Simulate termination callback", id: "recover") { model.terminationCallback() }
                    control("Focus card", id: "focus") { try await model.focusCard() }
                }
                .buttonStyle(.bordered).controlSize(.large).accessibilityIdentifier("card.behavior.actions")
                TextField(String(localized: "Native editor", table: "CardBehaviorRoutes"), text: $nativeText)
                    .textFieldStyle(.roundedBorder).controlSize(.large)
                    .frame(minHeight: Spacing.large * 2).contentShape(Rectangle())
                    .focused($nativeFocused)
                    .accessibilityLabel(String(localized: "Native editor", table: "CardBehaviorRoutes"))
                    .accessibilityIdentifier("card.behavior.editor")
                Text(String(localized: "Card status", table: "CardBehaviorRoutes"))
                    .font(.caption).accessibilityValue(model.state)
                    .accessibilityIdentifier("card.behavior.state")
                if let pair = model.pair {
                    CardBehaviorHost(pair: pair, presenter: model.presenter)
                        .frame(maxWidth: cardMaxWidth, maxHeight: .infinity)
                        .accessibilityIdentifier("card.behavior.surface")
                }
            }
            .padding(.horizontal, Spacing.small)
            .background(ColorToken.backgroundPrimary.color)
            .onAppear { model.start(appearance: appearance == .dark ? .dark : .light) }
            .onDisappear { model.stop() }
            .onChange(of: nativeFocused) { _, focused in if focused { model.pair?.ownsKeyboardFocus = false } }
            .onChange(of: appearance) { _, value in
                model.perform { try await model.pair?.setAppearance(value == .dark ? .dark : .light) }
            }
        }
        private func control(_ key: String.LocalizationValue, id: String, action: @escaping @MainActor () async throws -> Void) -> some View {
            Button(String(localized: key, table: "CardBehaviorRoutes")) { model.perform(action) }
                .buttonStyle(.bordered).controlSize(.large).accessibilityIdentifier("card.behavior.\(id)")
        }
    }

    #if os(macOS)
    struct CardBehaviorHost: NSViewControllerRepresentable {
        let pair: CardPagePair
        let presenter: NativeCardBehaviorPresenter
        func makeNSViewController(context: Context) -> CardBehaviorController { CardBehaviorController(pair: pair, presenter: presenter) }
        func updateNSViewController(_ controller: CardBehaviorController, context: Context) { presenter.window = controller.view.window }
    }
    final class CardBehaviorController: NSViewController {
        let pair: CardPagePair
        let presenter: NativeCardBehaviorPresenter
        init(pair: CardPagePair, presenter: NativeCardBehaviorPresenter) { self.pair = pair; self.presenter = presenter; super.init(nibName: nil, bundle: nil) }
        @available(*, unavailable) required init?(coder: NSCoder) { fatalError("unused") }
        override func loadView() { view = pair.container }
        override func viewDidAppear() { super.viewDidAppear(); presenter.window = view.window }
    }
    #else
    struct CardBehaviorHost: UIViewControllerRepresentable {
        let pair: CardPagePair
        let presenter: NativeCardBehaviorPresenter
        func makeUIViewController(context: Context) -> CardBehaviorController { CardBehaviorController(pair: pair, presenter: presenter) }
        func updateUIViewController(_ controller: CardBehaviorController, context: Context) { presenter.host = controller }
    }
    final class CardBehaviorController: UIViewController {
        let pair: CardPagePair
        let presenter: NativeCardBehaviorPresenter
        init(pair: CardPagePair, presenter: NativeCardBehaviorPresenter) { self.pair = pair; self.presenter = presenter; super.init(nibName: nil, bundle: nil) }
        @available(*, unavailable) required init?(coder: NSCoder) { fatalError("unused") }
        override func loadView() { view = pair.container }
        override func viewDidAppear(_ animated: Bool) { super.viewDidAppear(animated); presenter.host = self }
    }
    #endif
}
#endif
