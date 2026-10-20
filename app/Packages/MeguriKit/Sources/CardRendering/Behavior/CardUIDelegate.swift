import Foundation
import WebKit

public nonisolated enum CardDialog: Equatable, Sendable {
    case alert(String), confirm(String), prompt(String, defaultText: String?), scheme(String)
}
public nonisolated enum CardDialogResult: Equatable, Sendable {
    case alert, confirm(Bool), prompt(String?)
}

/// A presenter returns nil if it cannot present *now*. Nothing queues a
/// request. Cancellation must dismiss its own UI without activating a host.
public protocol CardBehaviorPresenting: AnyObject {
    var isActive: Bool { get }
    var isAvailable: Bool { get }
    func availability(of url: URL) -> CardURLAvailability
    func dialog(_ request: CardDialog, completion: @escaping @MainActor (CardDialogResult) -> Void) -> CardBehaviorPresentation?
    func open(_ request: CardOpenRequest) -> CardBehaviorPresentation?
}

extension CardBehaviorPresenting {
    public var isActive: Bool { isAvailable }
}

public final class CardBehaviorPresentation {
    private var dismiss: (@MainActor () -> Void)?
    public init(dismiss: @escaping @MainActor () -> Void) { self.dismiss = dismiss }
    public func cancel() { let action = dismiss; dismiss = nil; action?() }
}

/// Per-page requests, bound to the actual native page and its immutable
/// update. Invalidating a binding drains WebKit callbacks synchronously.
public final class CardPageBehavior: NSObject, WKUIDelegate {
    private weak var page: CardPage?
    public weak var presenter: (any CardBehaviorPresenting)? {
        didSet { cancel() }
    }
    public var preferExternalBrowser = false
    public var approvals = CardSchemeApprovals()
    public var onOpenRequest: ((CardOpenRequest) -> Void)?
    private var pending: [UUID: Pending] = [:]
    private var opened: [CardBehaviorPresentation] = []
    private var epoch = UUID()
    private struct Pending {
        let finish: @MainActor (CardDialogResult?) -> Void
        var presentation: CardBehaviorPresentation?
    }

    init(page: CardPage) { self.page = page }
    isolated deinit { cancel() }
    private var active: Bool {
        guard let page else { return false }
        return page.behaviorAvailable && page.webView.window != nil && presenter?.isActive == true
    }
    public func cancel() {
        epoch = UUID()
        let old = pending; pending.removeAll()
        let presentations = opened; opened.removeAll()
        for item in old.values { item.presentation?.cancel(); item.finish(nil) }
        for item in presentations { item.cancel() }
    }
    private func matches(_ update: CardUpdate, _ epoch: UUID) -> Bool {
        active && page?.update == update && self.epoch == epoch
    }
    private func accepts(_ webView: WKWebView, frame: WKFrameInfo) -> Bool {
        page?.allowsDocumentCallback(from: webView, frame: frame) == true
    }
    func dialog(_ request: CardDialog, completion: @escaping @MainActor (CardDialogResult?) -> Void) {
        guard active, let page, let update = page.update, let presenter, presenter.isAvailable else { completion(nil); return }
        let epoch = epoch, id = UUID()
        pending[id] = Pending(finish: completion)
        let presentation = presenter.dialog(request) { [weak self] result in
            guard let self, let item = self.pending.removeValue(forKey: id) else { return }
            item.finish(self.matches(update, epoch) ? result : nil)
        }
        // Presenters may respond synchronously, including cancellation.
        if pending[id] != nil {
            if let presentation { pending[id]?.presentation = presentation }
            else { pending.removeValue(forKey: id)?.finish(nil) }
        } else { presentation?.cancel() }
    }

    @discardableResult
    func route(_ url: URL) -> LinkPolicy.Route {
        guard let page else { return .refuse }
        let route = LinkPolicy.route(url, root: MediaOrigin.url, platform: page.platform, preferExternal: preferExternalBrowser)
        switch route {
        case .fragment(let fragment):
            guard page.behaviorAvailable, let update = page.update else { return route }
            let epoch = epoch
            Task { [weak self, weak page] in
                guard let self, self.epoch == epoch, let page, page.update == update, page.behaviorAvailable else { return }
                try? await page.run("""
                    if (fragment === '') window.scrollTo({left:0, top:0, behavior:'instant'});
                    else {
                        const target = document.getElementById(fragment) || document.getElementsByName(fragment)[0];
                        if (target) target.scrollIntoView({block:'start', behavior:'instant'});
                    }
                    """, arguments: ["fragment": fragment], world: CardPage.world)
            }
        case .browser(let browser): open(url, browser: browser)
        case .scheme(let scheme):
            guard active, presenter?.availability(of: url) != .unavailable, let update = page.update else { return route }
            let epoch = epoch
            if approvals.contains(scheme) { open(url, browser: .external) }
            else {
                dialog(.scheme(scheme)) { [weak self] result in
                    guard let self, result == .confirm(true), self.matches(update, epoch),
                          self.presenter?.availability(of: url) != .unavailable else { return }
                    self.approvals.approve(scheme)
                    guard self.matches(update, epoch) else { return }
                    self.open(url, browser: .external)
                }
            }
        case .javascript, .refuse: break
        }
        return route
    }

    private func open(_ url: URL, browser: CardBrowser) {
        guard active, let update = page?.update, let presenter, presenter.isAvailable,
              browser == .safari || presenter.availability(of: url) != .unavailable else { return }
        let epoch = epoch
        let request = CardOpenRequest(url: url, browser: browser, update: update)
        onOpenRequest?(request)
        guard matches(update, epoch) else { return }
        if let presentation = presenter.open(request) {
            if opened.count == 16 { opened.removeFirst().cancel() }
            opened.append(presentation)
        }
    }

    public func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                        for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if accepts(webView, frame: navigationAction.sourceFrame), let url = navigationAction.request.url { route(url) }
        return nil
    }
    public func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                        initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor () -> Void) {
        guard accepts(webView, frame: frame) else { completionHandler(); return }
        dialog(.alert(message)) { _ in completionHandler() }
    }
    public func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                        initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (Bool) -> Void) {
        guard accepts(webView, frame: frame) else { completionHandler(false); return }
        dialog(.confirm(message)) { result in
            if case .confirm(let accepted) = result { completionHandler(accepted) } else { completionHandler(false) }
        }
    }
    public func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                        initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping @MainActor (String?) -> Void) {
        guard accepts(webView, frame: frame) else { completionHandler(nil); return }
        dialog(.prompt(prompt, defaultText: defaultText)) { result in
            if case .prompt(let value) = result { completionHandler(value) } else { completionHandler(nil) }
        }
    }
}
