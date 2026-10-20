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
            registry.register("card/behaviors/separator") { _ in .screen { CardBehaviorView(answerForm: 1) } }
            registry.register("card/behaviors/fallback") { _ in .screen { CardBehaviorView(answerForm: 2) } }
            registry.register("card/behaviors/graphic") { _ in .screen { CardBehaviorView(answerForm: 3) } }
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
        private var answerForm: Int
        #if os(iOS)
        private var focusObservation: NSObjectProtocol?
        private var nativeVoiceFocus = "none"
        #endif

        init(answerForm: Int = 0) { self.answerForm = answerForm }

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
                <h1>\(title)</h1><input id=typeans aria-label="\(inputLabel)" autocapitalize=off autocorrect=off spellcheck=false>
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
            let payload = CardPayload(cardID: card, question: html, answer: heading + "<input id=typeans aria-label=\"\(inputLabel)\" autocapitalize=off autocorrect=off spellcheck=false>")
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
                const scalarRect = r => {
                  return [r.x,r.y,r.width,r.height].every(Number.isFinite)
                    ? {x:r.x,y:r.y,width:r.width,height:r.height} : null;
                };
                const rect = element => element ? scalarRect(element.getBoundingClientRect()) : null;
                const visual = window.visualViewport;
                const links = [['footer', '#qa a[href="#bottom"]'], ['script', '#qa a[href^="javascript:"]']].map(([id, selector]) => {
                  const element = document.querySelector(selector);
                  if (!element) return {id, exists:false};
                  const style = getComputedStyle(element), bounds = element.getBoundingClientRect();
                  const range = document.createRange();
                  range.selectNodeContents(element);
                  const left = Math.max(bounds.left, visual?.offsetLeft ?? 0);
                  const top = Math.max(bounds.top, visual?.offsetTop ?? 0);
                  const right = Math.min(bounds.right, (visual?.offsetLeft ?? 0) + (visual?.width ?? innerWidth));
                  const bottom = Math.min(bounds.bottom, (visual?.offsetTop ?? 0) + (visual?.height ?? innerHeight));
                  const center = right > left && bottom > top ? {x:(left+right)/2,y:(top+bottom)/2} : null;
                  const hit = center ? document.elementFromPoint(center.x, center.y) : null;
                  return {id, exists:true,
                    style:{display:style.display,paddingTop:style.paddingTop,paddingRight:style.paddingRight,
                      paddingBottom:style.paddingBottom,paddingLeft:style.paddingLeft,minHeight:style.minHeight,
                      lineHeight:style.lineHeight,pointerEvents:style.pointerEvents},
                    rect:scalarRect(bounds),clientRects:Array.from(element.getClientRects(), scalarRect),
                    textRects:Array.from(range.getClientRects(), scalarRect),visibleCenter:center,
                    hitIsAnchor:center ? hit === element : null,
                    hitIsDescendant:center ? hit !== element && element.contains(hit) : null};
                });
                return JSON.stringify({text:document.getElementById('typeans')?.value||'', marker:window.domMarker,
                  resizes:window.resizeEvents||0, height:innerHeight, layoutHeight:document.documentElement.clientHeight,
                  viewport:document.getElementById('viewport')?.offsetHeight,
                  scroll:scrollY, bottom:document.getElementById('bottom')?.getBoundingClientRect().bottom,
                  answerRect:rect(document.getElementById('answer')), firstHeadingRect:rect(document.querySelector('#qa h1')),
                  root:location.href, scriptRan:window.cardScriptRan===true,
                  links, linkViewport:{width:innerWidth,height:innerHeight},
                  ...([visual?.offsetLeft,visual?.offsetTop,visual?.scale].every(Number.isFinite)
                    ? {visualViewport:{offsetLeft:visual.offsetLeft,offsetTop:visual.offsetTop,scale:visual.scale}} : {}),
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
            let webView = page.webView
            let unignoredChildren = NSAccessibilityUnignoredChildren([webView]) as [Any]
            observations["macAccessibility"] = [
                "currentIsElement": webView.isAccessibilityElement(),
                "currentRole": webView.accessibilityRole()?.rawValue ?? "none",
                "currentParentIsContainer": (webView.accessibilityParent() as AnyObject?) === pair.container,
                "containerIsElement": pair.container.isAccessibilityElement(),
                "containerRole": pair.container.accessibilityRole()?.rawValue ?? "none",
                "currentRawChildrenCount": webView.accessibilityChildren()?.count ?? 0,
                "unignoredChildrenCount": unignoredChildren.count,
                "unignoredChildrenAreRawWK": unignoredChildren.map { ($0 as AnyObject) === webView }
            ]
            if let window = webView.window, let contentView = window.contentView {
                func rectReport(_ rect: CGRect) -> [String: CGFloat] {
                    ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
                }
                observations["native"] = [
                    "frame": rectReport(webView.convert(webView.bounds, to: contentView)),
                    "contentBounds": rectReport(contentView.bounds),
                    "safeContentLayoutRect": rectReport(contentView.convert(window.contentLayoutRect, from: nil))
                ]
            }
            #else
            observations["currentHidden"] = page.webView.accessibilityElementsHidden
            observations["idleHidden"] = pair.idle.webView.accessibilityElementsHidden
            #endif
            #if os(iOS)
            observations["voiceFocus"] = nativeVoiceFocus
            if let focused = UIAccessibility.focusedElement(using: .notificationVoiceOver) as? NSObject {
                let frame = focused.accessibilityFrame
                if frame.minX.isFinite, frame.minY.isFinite, frame.width.isFinite, frame.height.isFinite {
                    observations["voiceFocusGeometry"] = ["x": frame.minX, "y": frame.minY,
                                                           "width": frame.width, "height": frame.height]
                    observations["voiceFocusTraits"] = focused.accessibilityTraits.rawValue
                }
            }
            let nativeZoom = page.webView.scrollView.zoomScale
            if nativeZoom.isFinite { observations["nativeZoom"] = nativeZoom }
            if let window = page.webView.window {
                let frame = page.webView.convert(page.webView.bounds, to: window)
                observations["native"] = ["x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height,
                                          "safeTop": window.safeAreaInsets.top, "safeBottom": window.safeAreaInsets.bottom,
                                          "safeLeft": window.safeAreaInsets.left, "safeRight": window.safeAreaInsets.right,
                                          "windowHeight": window.bounds.height, "windowWidth": window.bounds.width]
            }
            if var links = observations["links"] as? [[String: Any]] {
                let webView = page.webView
                let viewport = observations["linkViewport"] as? [String: Double]
                let visual = observations["visualViewport"] as? [String: Double]
                // Only map a verified 1:1 CSS viewport to WebView points. A DOM
                // client rect already accounts for document scroll. Zoomed or
                // inset viewports remain unknown rather than guessing a scale.
                let mapsDirectly = webView.pageZoom == 1 && nativeZoom == 1 && visual?["scale"] == 1
                    && visual?["offsetLeft"] == 0 && visual?["offsetTop"] == 0
                    && abs((viewport?["width"] ?? -1) - webView.bounds.width) < 1
                    && abs((viewport?["height"] ?? -1) - webView.bounds.height) < 1
                    && webView.scrollView.frame == webView.bounds
                    && webView.scrollView.adjustedContentInset == .zero
                func hitReport(_ hit: UIView?) -> [String: Any] {
                    var classes: [String] = []
                    var ancestor = hit
                    while let view = ancestor, classes.count < 16 {
                        classes.append(NSStringFromClass(type(of: view)))
                        ancestor = view.superview
                    }
                    return ["classes": classes, "belongsToWebView": hit.map { $0 === webView || $0.isDescendant(of: webView) } ?? false]
                }
                for index in links.indices {
                    guard mapsDirectly, let window = webView.window,
                          let center = links[index]["visibleCenter"] as? [String: Double],
                          let x = center["x"], let y = center["y"], x.isFinite, y.isFinite else {
                        links[index]["nativeHitTest"] = ["status": "unknown"]
                        continue
                    }
                    let point = CGPoint(x: webView.bounds.minX + x, y: webView.bounds.minY + y)
                    links[index]["nativeHitTest"] = ["status": "unzoomedViewport",
                        "webView": hitReport(webView.hitTest(point, with: nil)),
                        "window": hitReport(window.hitTest(webView.convert(point, to: window), with: nil))]
                }
                observations["links"] = links
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
        @State private var model: CardBehaviorModel
        @State private var nativeText = ""
        @FocusState private var nativeFocused: Bool
        #if os(macOS)
        @State private var actionsPresented = false
        #endif
        @Environment(\.colorScheme) private var appearance
        @Environment(\.dynamicTypeSize) private var dynamicTypeSize
        #if os(iOS)
        @Environment(\.horizontalSizeClass) private var sizeClass
        #endif
        init(answerForm: Int = 0) {
            _model = State(initialValue: CardBehaviorModel(answerForm: answerForm))
        }
        private var cardMaxWidth: CGFloat {
            if model.narrow { return ContentWidth.primaryAction }
            #if os(iOS)
            return sizeClass == .regular ? ContentWidth.readable : .infinity
            #else
            return ContentWidth.readable
            #endif
        }
        var body: some View {
            GeometryReader { geometry in
                VStack(spacing: Spacing.xSmall) {
                    if dynamicTypeSize.isAccessibilitySize {
                        ScrollView { controls }
                            .frame(maxHeight: geometry.size.height / 2)
                    } else {
                        controls
                    }
                    if let pair = model.pair {
                        CardBehaviorHost(pair: pair, presenter: model.presenter)
                            .frame(maxWidth: cardMaxWidth, maxHeight: .infinity)
                            .accessibilityIdentifier("card.behavior.surface")
                    }
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
        private var controls: some View {
            VStack(spacing: Spacing.xSmall) {
                // Native dialogs start from visible controls, after no menu
                // or other presentation is competing for the explicit host.
                control("Confirm", id: "confirm") { try await model.dialog(prompt: false) }
                control("Prompt", id: "prompt") { try await model.dialog(prompt: true) }
                let answerLayout = dynamicTypeSize.isAccessibilitySize
                    ? AnyLayout(VStackLayout(spacing: Spacing.xSmall))
                    : AnyLayout(HStackLayout(spacing: Spacing.xSmall))
                answerLayout {
                    control("Reveal answer", id: "reveal") { try await model.reveal() }
                    control("Next card", id: "next") { try await model.next() }
                }
                Group {
                    #if os(macOS)
                    Button { actionsPresented = true } label: { actionsLabel }
                        .popover(isPresented: $actionsPresented) {
                            ScrollView {
                                VStack(spacing: Spacing.xSmall) { nativeActions }
                                    .padding(Spacing.small)
                            }
                            .frame(width: ContentWidth.primaryAction)
                            .frame(maxHeight: ContentWidth.readable)
                        }
                    #else
                    Menu { nativeActions } label: { actionsLabel }
                    #endif
                }
                .buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
                .accessibilityIdentifier("card.behavior.actions")
                TextField(String(localized: "Native editor", table: "CardBehaviorRoutes"), text: $nativeText)
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    #if os(macOS)
                    .controlSize(.extraLarge)
                    .font(.title2)
                    #else
                    .controlSize(.large)
                    #endif
                    .frame(minHeight: Spacing.large * 2).contentShape(Rectangle())
                    .focused($nativeFocused)
                    .accessibilityLabel(String(localized: "Native editor", table: "CardBehaviorRoutes"))
                    .accessibilityIdentifier("card.behavior.editor")
                Text(String(localized: "Card status", table: "CardBehaviorRoutes"))
                    .font(.caption).accessibilityValue(model.state)
                    .accessibilityIdentifier("card.behavior.state")
            }
        }
        private var actionsLabel: some View {
            Text(String(localized: "Card actions", table: "CardBehaviorRoutes"))
                .lineLimit(nil)
                .fixedSize(horizontal: false, vertical: true)
        }
        @ViewBuilder private var nativeActions: some View {
            control("Answer separator", id: "separator") { model.answerVariant(1) }
            control("Answer fallback", id: "fallback") { model.answerVariant(2) }
            control("Answer graphic", id: "graphic") { model.answerVariant(3) }
            control("Card zoom", id: "zoom") { model.zoom() }
            control("Resize card", id: "resize") { model.narrow.toggle(); await model.refresh() }
            control("Read card state", id: "refresh") { await model.refresh() }
            control("Suspend card", id: "suspend") { await model.suspend() }
            control("Resume card", id: "resume") { await model.resume() }
            control("Probe pages", id: "probe") { await model.probe() }
            control("Simulate termination callback", id: "recover") { model.terminationCallback() }
            control("Focus card", id: "focus") { try await model.focusCard() }
        }
        private func control(_ key: String.LocalizationValue, id: String, action: @escaping @MainActor () async throws -> Void) -> some View {
            Button {
                #if os(macOS)
                actionsPresented = false
                #endif
                model.perform(action)
            } label: {
                Text(String(localized: key, table: "CardBehaviorRoutes"))
                    .multilineTextAlignment(.center)
                    .lineLimit(nil)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
            .accessibilityIdentifier("card.behavior.\(id)")
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
