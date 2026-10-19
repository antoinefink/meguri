import Foundation
import WebKit

public final class CardWebConfiguration {
    // Stable production storage, distinct from the U65 SPIKE identifier.
    public static let storeIdentifier = UUID(uuidString: "6D656775-7269-4341-5244-53544F524531")!
    public static let shared = CardWebConfiguration()
    public let configuration: WKWebViewConfiguration
    public let media: MediaSchemeHandler
    public let bridge: CardBridge
    private var rules: Task<Void, any Error>?

    /// A store may be injected for tests; callers remove its fresh identifier
    /// even after failure. Production surfaces use `shared`.
    public init(store: WKWebsiteDataStore = WKWebsiteDataStore(forIdentifier: storeIdentifier), media: MediaSchemeHandler = MediaSchemeHandler()) {
        configuration = WKWebViewConfiguration()
        self.media = media
        bridge = CardBridge()
        configuration.websiteDataStore = store
        configuration.mediaTypesRequiringUserActionForPlayback = []
        configuration.setURLSchemeHandler(media, forURLScheme: MediaOrigin.scheme)
        #if !os(macOS)
        configuration.allowsInlineMediaPlayback = true
        if PlatformClasses.current == .iPad { configuration.defaultWebpagePreferences.preferredContentMode = .mobile }
        #endif
        let controller = configuration.userContentController
        let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)
        controller.addUserScript(WKUserScript(source: CardRuntime.interactionScript, injectionTime: .atDocumentStart,
                                             forMainFrameOnly: true, in: world))
        for name in [CardRuntime.signalHandlerName, CardRuntime.confirmationHandlerName] {
            controller.addScriptMessageHandler(bridge, contentWorld: world, name: name)
        }
        #if !os(macOS)
        for name in ["cb", "meguri"] { controller.addScriptMessageHandler(bridge, contentWorld: .page, name: name) }
        #endif
    }

    /// Must finish before either page navigates, including reviewExtra.
    public func ready() async throws {
        #if !os(macOS)
        if rules == nil {
            let controller = configuration.userContentController
            rules = Task {
                let source = Self.loopbackRules
                let list: WKContentRuleList = try await withCheckedThrowingContinuation { continuation in
                    WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "meguri.card.loopback.v1", encodedContentRuleList: source) { list, error in
                        if let list { continuation.resume(returning: list) }
                        else { continuation.resume(throwing: error ?? URLError(.cannotLoadFromNetwork)) }
                    }
                }
                controller.add(list)
            }
        }
        try await rules?.value
        #endif
    }

    // WebKit canonicalizes numeric IPv4 URLs before content-rule matching.
    // Include raw spellings too, IPv6-mapped loopback and localhost subdomains.
    public static let loopbackRules = #"[{"trigger":{"url-filter":"^https?://([^/@]+@)?(localhost\\.?|[^/:]+\\.localhost\\.?|127\\.[0-9.]+|2130706433|0x7f[0-9a-f.]*|0177\\.[0-7.]+|\\[(0*:)*1\\]|\\[::ffff:(127\\.[0-9.]+|7f[0-9a-f:]+)\\])(:[0-9]+)?/","url-filter-is-case-sensitive":false},"action":{"type":"block"}}]"#
}
