import Foundation
import DesignSystem
#if os(macOS)
import AppKit
#else
import SafariServices
import UIKit
#endif

private enum CardBehaviorCopy {
    static var ok: String { String(localized: "card.dialog.ok", table: "CardBehavior", bundle: .module) }
    static var cancel: String { String(localized: "card.dialog.cancel", table: "CardBehavior", bundle: .module) }
    static var title: String { String(localized: "card.dialog.title", table: "CardBehavior", bundle: .module) }
    static var allow: String { String(localized: "card.scheme.allow", table: "CardBehavior", bundle: .module) }
    static func scheme(_ scheme: String) -> String {
        String(localized: "card.scheme.confirm \(scheme)", table: "CardBehavior", bundle: .module)
    }
}

/// The session keeps this presenter alive and supplies its own native host.
/// It never searches application windows or activates the Mac application.
public final class NativeCardBehaviorPresenter: CardBehaviorPresenting {
    #if os(macOS)
    public weak var window: NSWindow?
    public init(window: NSWindow? = nil) { self.window = window }
    public var isActive: Bool { NSApp?.isActive == true && NSApp?.isHidden == false && window?.isVisible == true }
    public var isAvailable: Bool {
        isActive && window?.attachedSheet == nil
    }
    public func availability(of url: URL) -> CardURLAvailability {
        guard url.scheme?.lowercased() != "file" else { return .unavailable }
        return NSWorkspace.shared.urlForApplication(toOpen: url) == nil ? .unavailable : .available
    }

    public func dialog(_ request: CardDialog, completion: @escaping @MainActor (CardDialogResult) -> Void) -> CardBehaviorPresentation? {
        guard isAvailable, let window else { return nil }
        let alert = NSAlert()
        alert.messageText = CardBehaviorCopy.title
        let field: NSTextField?
        switch request {
        case .alert(let message), .confirm(let message): alert.informativeText = message; field = nil
        case .prompt(let message, let defaultText):
            alert.informativeText = message
            let input = NSTextField(string: defaultText ?? "")
            input.frame.size = CGSize(width: ContentWidth.primaryAction, height: input.fittingSize.height)
            input.setAccessibilityLabel(message)
            alert.accessoryView = input
            field = input
        case .scheme(let scheme): alert.informativeText = CardBehaviorCopy.scheme(scheme); field = nil
        }
        alert.addButton(withTitle: { if case .scheme = request { return CardBehaviorCopy.allow }; return CardBehaviorCopy.ok }())
        if case .alert = request {} else { alert.addButton(withTitle: CardBehaviorCopy.cancel) }
        alert.beginSheetModal(for: window) { response in
            let accepted = response == .alertFirstButtonReturn
            switch request {
            case .alert: completion(.alert)
            case .confirm, .scheme: completion(.confirm(accepted))
            case .prompt: completion(.prompt(accepted ? field?.stringValue ?? "" : nil))
            }
        }
        return CardBehaviorPresentation { [weak window, weak alert] in
            guard let alert, alert.window.sheetParent === window else { return }
            window?.endSheet(alert.window, returnCode: .cancel)
            alert.window.orderOut(nil)
        }
    }
    public func open(_ request: CardOpenRequest) -> CardBehaviorPresentation? {
        guard isAvailable, availability(of: request.url) == .available, let app = NSWorkspace.shared.urlForApplication(toOpen: request.url) else { return nil }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        NSWorkspace.shared.open([request.url], withApplicationAt: app, configuration: configuration) { _, _ in }
        return nil
    }
    #else
    public weak var host: UIViewController?
    private weak var dialogController: UIAlertController?
    private weak var safariController: SFSafariViewController?
    var externalURLs = CardIOSURLOpener()
    var onExternalOpenOutcome: ((Bool) -> Void)?
    public init(host: UIViewController? = nil) { self.host = host }
    public var isActive: Bool {
        guard let host, host.viewIfLoaded?.window != nil, !host.isBeingDismissed else { return false }
        return host.view.window?.windowScene?.activationState == .foregroundActive
    }
    public var isAvailable: Bool {
        isActive && host?.presentedViewController == nil && dialogController == nil && safariController == nil
    }
    public func availability(of url: URL) -> CardURLAvailability { externalURLs.availability(of: url) }

    public func dialog(_ request: CardDialog, completion: @escaping @MainActor (CardDialogResult) -> Void) -> CardBehaviorPresentation? {
        guard isAvailable, let host else { return nil }
        let message: String
        switch request {
        case .alert(let value), .confirm(let value), .prompt(let value, _): message = value
        case .scheme(let value): message = CardBehaviorCopy.scheme(value)
        }
        let panel = UIAlertController(title: CardBehaviorCopy.title, message: message, preferredStyle: .alert)
        if case .prompt(_, let initial) = request { panel.addTextField { $0.text = initial } }
        let acceptTitle = { if case .scheme = request { return CardBehaviorCopy.allow }; return CardBehaviorCopy.ok }()
        let finish: (CardDialogResult) -> Void = { [weak self, weak panel] choice in
            guard let panel else { completion(choice); return }
            panel.dismiss(animated: false) { [weak self] in
                self?.dialogController = nil
                completion(choice)
            }
        }
        panel.addAction(UIAlertAction(title: acceptTitle, style: .default) { [weak panel] _ in
            switch request {
            case .alert: finish(.alert)
            case .confirm, .scheme: finish(.confirm(true))
            case .prompt: finish(.prompt(panel?.textFields?.first?.text ?? ""))
            }
        })
        if case .alert = request {} else {
            panel.addAction(UIAlertAction(title: CardBehaviorCopy.cancel, style: .cancel) { _ in
                if case .prompt = request { finish(.prompt(nil)) }
                else { finish(.confirm(false)) }
            })
        }
        dialogController = panel
        host.present(panel, animated: true)
        guard panel.presentingViewController != nil else { dialogController = nil; return nil }
        return CardBehaviorPresentation { [weak self, weak panel] in
            panel?.dismiss(animated: false)
            if self?.dialogController === panel { self?.dialogController = nil }
        }
    }
    public func open(_ request: CardOpenRequest) -> CardBehaviorPresentation? {
        guard isAvailable, let host else { return nil }
        if request.browser == .safari {
            guard ["http", "https"].contains(request.url.scheme?.lowercased() ?? "") else { return nil }
            let safari = SFSafariViewController(url: request.url)
            safariController = safari
            host.present(safari, animated: true)
            guard safari.presentingViewController != nil else { safariController = nil; return nil }
            return CardBehaviorPresentation { [weak self, weak safari] in
                safari?.dismiss(animated: false)
                if self?.safariController === safari { self?.safariController = nil }
            }
        }
        externalURLs.open(request.url) { [weak self] succeeded in self?.onExternalOpenOutcome?(succeeded) }
        return nil
    }
    #endif
}
