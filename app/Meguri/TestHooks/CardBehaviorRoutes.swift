#if MEGURI_TEST_HOOKS
import AppModel
import CardRendering
import CoreBridge
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
            registry.register("card/behaviors/replay-identifier") { _ in .screen { CardBehaviorView(answerForm: 4) } }
        }
    }

    @Observable final class CardBehaviorModel {
        var pair: CardPagePair?
        private var status = "loading"
        private var recoverySequence: UInt64 = 0
        private var actionReceipt = "actionSequence=0 action=recover phase=none"
        private var terminationReceipt = "callback=none"
        private var nativeSnapshot = "native=none"
        private static let replayIdentifier = "review.card.replay.q0"
        private let probesReplayIdentifier: Bool
        private var replayIdentifierAttempted = false
        var probePagesDisabled: Bool { probesReplayIdentifier && replayIdentifierAttempted }
        private var replayIdentifierResult = ReplayIdentifierResult()
        private struct ReplayIdentifierResult {
            var leafFound = false
            var protocolSupported = false
            var writeReadbackMatched = false
            var rehitReadbackMatched = false

            var state: String {
                "leafFound=\(leafFound) protocolSupported=\(protocolSupported) writeReadbackMatched=\(writeReadbackMatched) rehitReadbackMatched=\(rehitReadbackMatched)"
            }
        }
        var state: String {
            get {
                if probesReplayIdentifier { return replayIdentifierResult.state }
                let receipts = "\(actionReceipt) \(terminationReceipt) \(nativeSnapshot)"
                if let json = status.firstIndex(of: "{") {
                    return "\(status[..<json])\(receipts) | \(status[json...])"
                }
                return "\(status) \(receipts)"
            }
            set { status = newValue }
        }
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

        init(answerForm: Int = 0) {
            self.answerForm = answerForm
            probesReplayIdentifier = answerForm == 4
        }

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
            func escapedLinkLabel(_ value: String) -> String {
                value.replacingOccurrences(of: "&", with: "&amp;")
                    .replacingOccurrences(of: "<", with: "&lt;")
                    .replacingOccurrences(of: ">", with: "&gt;")
                    .replacingOccurrences(of: "\"", with: "&quot;")
            }
            let jumpHTML = escapedLinkLabel(jump)
            let scriptLinkHTML = escapedLinkLabel(scriptLink)
            let html = """
                <style>body{margin:0}input{font:inherit;min-height:44px;max-width:100%;box-sizing:border-box}
                a{display:block;min-height:44px;box-sizing:border-box;padding:16px}#viewport{height:100vh}#space{height:200vh}#bottom{position:fixed;bottom:0;margin:0}</style>
                <h1>\(title)</h1><input id=typeans aria-label="\(inputLabel)" autocapitalize=off autocorrect=off spellcheck=false>
                <p><a href="#bottom" aria-label="\(jumpHTML)" style="display:flex;align-items:center;gap:8px">
                <svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="0 0 44 44" role="img" aria-label="\(jumpHTML)" style="flex:none" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M22 6V28M13 19L22 28L31 19M8 36H36"/></svg>
                <span>\(jumpHTML)</span></a> <a href="javascript:window.cardScriptRan=true;void(0)" aria-label="\(scriptLinkHTML)" style="display:flex;align-items:center;gap:8px">
                <svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="0 0 44 44" role="img" aria-label="\(scriptLinkHTML)" style="flex:none" fill="currentColor"><path d="M14 8L36 22L14 36Z"/></svg>
                <span>\(scriptLinkHTML)</span></a></p>
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
            // Synthetic D4 output with explicit ownership metadata; no media is bound.
            #if os(macOS)
            let replayAction = "pycmd('play:q:0')"
            #else
            let replayAction = "window.webkit.messageHandlers.meguri.postMessage('play:q:0')"
            #endif
            let replayHTML = """
                <h1>\(title)</h1><a class="replay-button soundLink" id="\(Self.replayIdentifier)" data-meguri-replay="true" aria-label="Play audio" href=# onclick="\(replayAction); return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>
                """
            let payload = CardPayload(cardID: card, question: answerForm == 4 ? replayHTML : html,
                                      answer: heading + "<input id=typeans aria-label=\"\(inputLabel)\" autocapitalize=off autocorrect=off spellcheck=false>",
                                      questionReplayIDs: answerForm == 4 ? [Self.replayIdentifier] : [])
            let update = try await pair.prepare(payload)
            CATransaction.begin()
            _ = try pair.present(update)
            CATransaction.commit()
        }

        func reveal() async throws { _ = try await pair?.showAnswer(); await refresh() }
        func zoom() { guard let pair else { return }; pair.cardZoom = CardZoom(percent: pair.cardZoom.percent == 100 ? 150 : 100); Task { await refresh() } }
        func suspend() async { await pair?.suspend(); await refresh() }
        func resume() async { await pair?.resume(); await refresh() }
        func probe() async {
            if probesReplayIdentifier {
                await probeReplayIdentifier()
                return
            }
            _ = await pair?.probeForegroundLiveness(); probes += 1; await refresh()
        }
        private func probeReplayIdentifier() async {
            guard !replayIdentifierAttempted else { return }
            // macOS has no supported setter probe in this experiment.
            #if os(iOS)
            guard let pair, let binding = pair.current.update, binding.side == .question,
                  pair.current.payload?.questionReplayIDs == [Self.replayIdentifier] else { return }
            let page = pair.current
            let webView = page.webView
            guard let window = webView.window, let context = page.hostReports.context,
                  context.update == binding, context.visibility == .presented else { return }
            let revision = page.hostReports.viewRevision
            let token = page.visibleToken
            let terminations = page.terminations
            let bounds = webView.bounds
            let frame = webView.convert(bounds, to: window)
            let windowBounds = window.bounds
            let windowFrame = window.frame
            let scrollBounds = webView.scrollView.bounds
            let scrollFrame = webView.scrollView.frame
            let inset = webView.scrollView.adjustedContentInset
            let zoom = webView.scrollView.zoomScale
            let pageZoom = webView.pageZoom
            func isCurrent() -> Bool {
                self.pair === pair && pair.current === page && page.update == binding
                    && page.presented && page.settled && !page.closed && page.visibleToken == token
                    && page.terminations == terminations && page.hostReports.context == context
                    && page.hostReports.viewRevision == revision && webView.window === window
                    && webView.bounds == bounds && webView.convert(bounds, to: window) == frame
                    && window.bounds == windowBounds && window.frame == windowFrame
                    && webView.scrollView.bounds == scrollBounds && webView.scrollView.frame == scrollFrame
                    && webView.scrollView.adjustedContentInset == inset
                    && webView.scrollView.zoomScale == zoom && webView.pageZoom == pageZoom
                    && UIApplication.shared.applicationState == .active && window.isKeyWindow
                    && !window.isHidden && window.alpha > 0
                    && window.windowScene?.activationState == .foregroundActive
                    && window.rootViewController?.presentedViewController == nil
                    && !webView.isHidden && webView.alpha > 0 && !webView.accessibilityElementsHidden
                    && bounds.width > 0 && bounds.height > 0 && window.bounds.intersects(frame)
            }
            guard isCurrent() else { return }
            let result = try? await webView.callAsyncJavaScript("""
                const anchor = document.getElementById(identifier);
                if (!anchor || !anchor.matches('a[data-meguri-replay="true"][href]')) return null;
                for (let node = anchor; node; node = node.parentElement) {
                    const style = getComputedStyle(node);
                    if (node.hidden || node.getAttribute('aria-hidden') === 'true' ||
                        style.display === 'none' || ['hidden', 'collapse'].includes(style.visibility) ||
                        Number(style.opacity) === 0) return null;
                }
                const viewport = window.visualViewport;
                const left = viewport?.offsetLeft ?? 0, top = viewport?.offsetTop ?? 0;
                const width = viewport?.width ?? innerWidth, height = viewport?.height ?? innerHeight;
                const rect = anchor.getBoundingClientRect();
                const x1 = Math.max(rect.left, left), y1 = Math.max(rect.top, top);
                const x2 = Math.min(rect.right, left + width), y2 = Math.min(rect.bottom, top + height);
                if (!(x2 > x1 && y2 > y1)) return null;
                const x = (x1 + x2) / 2, y = (y1 + y2) / 2;
                const hit = document.elementFromPoint(x, y);
                if (!hit || !(hit === anchor || anchor.contains(hit))) return null;
                return {x, y, left, top, width, height};
                """, arguments: ["identifier": Self.replayIdentifier], in: nil, contentWorld: .page)
            guard isCurrent(), let value = result as? [String: Double],
                  let x = value["x"], let y = value["y"], let left = value["left"], let top = value["top"],
                  let width = value["width"], let height = value["height"],
                  [x, y, left, top, width, height].allSatisfy(\.isFinite), width > 0, height > 0,
                  x >= left, y >= top, x < left + width, y < top + height else { return }
            // The visual viewport includes page/pinch zoom; DOM rects include scroll.
            let point = CGPoint(x: bounds.minX + CGFloat((x - left) / width) * bounds.width,
                                y: bounds.minY + CGFloat((y - top) / height) * bounds.height)
            guard bounds.contains(point), window.bounds.contains(webView.convert(point, to: window)) else { return }
            func nativeLeaf() -> NSObject? {
                guard let element = webView.accessibilityHitTest(point, event: nil) as? NSObject,
                      element !== webView, !(element is WKWebView), !(element is UIScrollView), !(element is UIWindow),
                      [0, NSNotFound].contains(element.accessibilityElementCount()),
                      element.accessibilityElements?.isEmpty != false,
                      element.accessibilityContainerType == .none, !element.shouldGroupAccessibilityChildren,
                      element.accessibilityTraits.contains(.link) else { return nil }
                if !element.isAccessibilityElement {
                    // As in CardAccessibility, an opaque remote hit may lack the
                    // in-process element flag; ordinary native containers cannot qualify.
                    guard !(element is UIView), !(element is UIAccessibilityElement) else { return nil }
                }
                return element
            }
            guard isCurrent(), !replayIdentifierAttempted else { return }
            replayIdentifierAttempted = true
            guard let leaf = nativeLeaf() else { return }
            replayIdentifierResult.leafFound = true
            guard let identified = leaf as? any UIAccessibilityIdentification else { return }
            replayIdentifierResult.protocolSupported = true
            guard isCurrent() else { return }
            identified.accessibilityIdentifier = Self.replayIdentifier
            replayIdentifierResult.writeReadbackMatched = identified.accessibilityIdentifier == Self.replayIdentifier
            guard isCurrent(), let rehit = nativeLeaf(),
                  let reidentified = rehit as? any UIAccessibilityIdentification else { return }
            replayIdentifierResult.rehitReadbackMatched = reidentified.accessibilityIdentifier == Self.replayIdentifier
            #endif
        }
        func beginRecoveryReceipt() -> UInt64 {
            recoverySequence += 1
            recordRecoveryReceipt(recoverySequence, phase: "queued")
            return recoverySequence
        }
        func recordRecoveryReceipt(_ sequence: UInt64?, phase: String) {
            guard let sequence, sequence == recoverySequence else { return }
            actionReceipt = "actionSequence=\(sequence) action=recover phase=\(phase)"
        }
        func terminationCallback() {
            guard let page = pair?.current else { return }
            let before = "callbackBeforeTerminations=\(page.terminations) callbackBeforeGeneration=\(page.update.map { String($0.generation) } ?? "none")"
            terminationReceipt = "\(before) callbackAfter=pending"
            // Public native callback seam: this is not an OS process kill.
            page.webViewWebContentProcessDidTerminate(page.webView)
            terminationReceipt = "\(before) callbackAfterTerminations=\(page.terminations) callbackAfterGeneration=\(page.update.map { String($0.generation) } ?? "none")"
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
            if probesReplayIdentifier { return }
            guard let pair else { nativeSnapshot = "native=no-pair"; return }
            let page = pair.current
            // Publish native-only evidence before JavaScript can suspend this read.
            // Only fixed technical kinds are exported, never entry names or errors.
            let kinds = Set(pair.diagnostics.entries.map(\.kind))
                .intersection(["processTerminated", "recoveryUnavailable"]).sorted()
            nativeSnapshot = "nativeClosed=\(page.closed) nativeTerminations=\(page.terminations) nativeGeneration=\(page.update.map { String($0.generation) } ?? "none") nativeSettled=\(page.settled) nativeKinds=\(kinds.isEmpty ? "none" : kinds.joined(separator: ","))"
            guard let update = page.update else { return }
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
            let unignoredChildren = NSAccessibility.unignoredChildren(from: [webView]) as [Any]
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
        func perform(_ action: @escaping @MainActor () async throws -> Void, recoverySequence: UInt64? = nil) {
            Task {
                do {
                    recordRecoveryReceipt(recoverySequence, phase: "invoking")
                    try await action()
                    recordRecoveryReceipt(recoverySequence, phase: "returned")
                } catch { state = "failed: \(error)" }
            }
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
        @State private var actionsPresented = false
        #if os(iOS)
        @State private var pendingAction: (@MainActor () async throws -> Void)?
        @State private var pendingRecoverySequence: UInt64?
        @State private var sheetActionTask: Task<Void, Never>?
        @State private var fixtureActive = false
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
                    #if os(macOS)
                    if dynamicTypeSize.isAccessibilitySize {
                        ScrollView { controls }
                            .frame(maxHeight: geometry.size.height / 2)
                    } else {
                        controls
                    }
                    #else
                    controls.fixedSize(horizontal: false, vertical: true)
                    #endif
                    if let pair = model.pair {
                        CardBehaviorHost(pair: pair, presenter: model.presenter)
                            .frame(maxWidth: cardMaxWidth, maxHeight: .infinity)
                            .accessibilityIdentifier("card.behavior.surface")
                    }
                }
            }
            .padding(.horizontal, Spacing.small)
            .background(ColorToken.backgroundPrimary.color)
            #if os(iOS)
            .sheet(isPresented: $actionsPresented, onDismiss: performPendingAction) {
                VStack(spacing: 0) {
                    let headerLayout = dynamicTypeSize.isAccessibilitySize
                        ? AnyLayout(VStackLayout(alignment: .leading, spacing: Spacing.xSmall))
                        : AnyLayout(HStackLayout(spacing: Spacing.small))
                    headerLayout {
                        Text(String(localized: "Card actions", table: "CardBehaviorRoutes"))
                            .font(.headline)
                            .lineLimit(nil)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Button {
                            model.recordRecoveryReceipt(pendingRecoverySequence, phase: "drop-cancelled")
                            pendingRecoverySequence = nil
                            pendingAction = nil
                            actionsPresented = false
                        } label: {
                            Text(String(localized: "Close", table: "CardBehaviorRoutes"))
                                .font(.body)
                                .lineLimit(nil)
                                .fixedSize(horizontal: false, vertical: true)
                                .padding(.horizontal, Spacing.small)
                                .padding(.vertical, Spacing.xSmall)
                                .frame(minWidth: Spacing.large * 2, minHeight: Spacing.large * 2)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("card.behavior.actions.close")
                    }
                    .foregroundStyle(ColorToken.textPrimary.color)
                    .padding(Spacing.small)
                    .fixedSize(horizontal: false, vertical: true)
                    .background(ColorToken.backgroundPrimary.color)
                    ScrollView {
                        VStack(alignment: .leading, spacing: Spacing.small) {
                            control("Confirm", id: "confirm", sheetAction: true) { try await model.dialog(prompt: false) }
                            control("Prompt", id: "prompt", sheetAction: true) { try await model.dialog(prompt: true) }
                            nativeActions
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Spacing.small)
                    }
                    .frame(maxHeight: .infinity)
                    .accessibilityIdentifier("card.behavior.actions.list")
                    .foregroundStyle(ColorToken.textPrimary.color)
                    .background(ColorToken.backgroundPrimary.color)
                    .tint(ColorToken.textPrimary.color)
                }
                .background(ColorToken.backgroundPrimary.color)
                .presentationSizing(.page)
            }
            #endif
            .onAppear {
                #if os(iOS)
                fixtureActive = true
                #endif
                model.start(appearance: appearance == .dark ? .dark : .light)
            }
            .onDisappear {
                #if os(iOS)
                fixtureActive = false
                model.recordRecoveryReceipt(pendingRecoverySequence, phase: "drop-inactive")
                pendingRecoverySequence = nil
                pendingAction = nil
                sheetActionTask?.cancel()
                sheetActionTask = nil
                actionsPresented = false
                #endif
                model.stop()
            }
            .onChange(of: nativeFocused) { _, focused in if focused { model.pair?.ownsKeyboardFocus = false } }
            .onChange(of: appearance) { _, value in
                model.perform { try await model.pair?.setAppearance(value == .dark ? .dark : .light) }
            }
        }
        private var controls: some View {
            VStack(spacing: Spacing.xSmall) {
                #if os(macOS)
                control("Confirm", id: "confirm") { try await model.dialog(prompt: false) }
                control("Prompt", id: "prompt") { try await model.dialog(prompt: true) }
                #endif
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
                    Button { actionsPresented = true } label: { actionsLabel }
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
            control("Answer separator", id: "separator", sheetAction: true) { model.answerVariant(1) }
            control("Answer fallback", id: "fallback", sheetAction: true) { model.answerVariant(2) }
            control("Answer graphic", id: "graphic", sheetAction: true) { model.answerVariant(3) }
            control("Card zoom", id: "zoom", sheetAction: true) { model.zoom() }
            control("Resize card", id: "resize", sheetAction: true) { model.narrow.toggle(); await model.refresh() }
            control("Read card state", id: "refresh", sheetAction: true) { await model.refresh() }
            control("Suspend card", id: "suspend", sheetAction: true) { await model.suspend() }
            control("Resume card", id: "resume", sheetAction: true) { await model.resume() }
            control("Probe pages", id: "probe", sheetAction: true) { await model.probe() }
                .disabled(model.probePagesDisabled)
            control("Simulate termination callback", id: "recover", sheetAction: true) { model.terminationCallback() }
            control("Focus card", id: "focus", sheetAction: true) { try await model.focusCard() }
        }
        #if os(iOS)
        private func performPendingAction() {
            // Take synchronously: a second dismissal callback cannot schedule it again.
            let action = pendingAction
            pendingAction = nil
            let sequence = pendingRecoverySequence
            pendingRecoverySequence = nil
            if action != nil { model.recordRecoveryReceipt(sequence, phase: "dismissed") }
            guard let action, fixtureActive, let pair = model.pair, !pair.current.closed else {
                let reason = action == nil ? "drop-cancelled" : !fixtureActive ? "drop-inactive"
                    : model.pair == nil ? "drop-no-pair" : "drop-closed"
                model.recordRecoveryReceipt(sequence, phase: reason)
                return
            }
            sheetActionTask = Task { @MainActor in
                guard !Task.isCancelled, fixtureActive, model.pair === pair, !pair.current.closed else {
                    let reason = Task.isCancelled ? "drop-cancelled" : !fixtureActive ? "drop-inactive"
                        : model.pair == nil ? "drop-no-pair" : model.pair !== pair ? "drop-identity-changed" : "drop-closed"
                    model.recordRecoveryReceipt(sequence, phase: reason)
                    return
                }
                do {
                    model.recordRecoveryReceipt(sequence, phase: "invoking")
                    try await action()
                    model.recordRecoveryReceipt(sequence, phase: "returned")
                }
                catch {
                    if !Task.isCancelled, fixtureActive, model.pair === pair, !pair.current.closed {
                        model.state = "failed: \(error)"
                    }
                }
            }
        }
        #endif
        @ViewBuilder private func control(
            _ key: String.LocalizationValue, id: String, sheetAction: Bool = false,
            action: @escaping @MainActor () async throws -> Void
        ) -> some View {
            #if os(iOS)
            if sheetAction {
                controlButton(key, id: id, sheetAction: true, action: action)
                    .buttonStyle(.plain).tint(ColorToken.textPrimary.color)
            } else {
                controlButton(key, id: id, sheetAction: false, action: action)
                    .buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
            }
            #else
            controlButton(key, id: id, sheetAction: false, action: action)
                .buttonStyle(.bordered).controlSize(.large).tint(ColorToken.textPrimary.color)
            #endif
        }
        private func controlButton(
            _ key: String.LocalizationValue, id: String, sheetAction: Bool,
            action: @escaping @MainActor () async throws -> Void
        ) -> some View {
            Button {
                let recoverySequence = id == "recover" ? model.beginRecoveryReceipt() : nil
                #if os(macOS)
                actionsPresented = false
                #else
                if sheetAction {
                    guard fixtureActive, pendingAction == nil, let pair = model.pair, !pair.current.closed else {
                        let reason = !fixtureActive ? "drop-inactive" : pendingAction != nil ? "drop-pending"
                            : model.pair == nil ? "drop-no-pair" : "drop-closed"
                        model.recordRecoveryReceipt(recoverySequence, phase: reason)
                        return
                    }
                    pendingAction = action
                    pendingRecoverySequence = recoverySequence
                    actionsPresented = false
                    return
                }
                #endif
                model.perform(action, recoverySequence: recoverySequence)
            } label: {
                Text(String(localized: key, table: "CardBehaviorRoutes"))
                    .multilineTextAlignment(sheetAction ? .leading : .center)
                    .lineLimit(nil)
                    .fixedSize(horizontal: false, vertical: true)
                    #if os(iOS)
                    .frame(maxWidth: sheetAction ? .infinity : nil,
                           minHeight: sheetAction ? Spacing.large * 2 : nil, alignment: .leading)
                    .contentShape(Rectangle())
                    #endif
            }
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
