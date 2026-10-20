import Foundation
import WebKit

public final class CardWebConfiguration {
    // Stable production storage, distinct from the U65 SPIKE identifier.
    public static let storeIdentifier = UUID(uuidString: "6D656775-7269-4341-5244-53544F524531")!
    public static let shared = CardWebConfiguration()
    public let configuration: WKWebViewConfiguration
    public let media: MediaSchemeHandler
    public let bridge: CardBridge
    public let hostBridge: CardHostBridge
    private var rules: Task<Void, any Error>?

    /// A store may be injected for tests; callers remove its fresh identifier
    /// even after failure. Production surfaces use `shared`.
    public init(store: WKWebsiteDataStore = WKWebsiteDataStore(forIdentifier: storeIdentifier), media: MediaSchemeHandler = MediaSchemeHandler()) {
        configuration = WKWebViewConfiguration()
        self.media = media
        bridge = CardBridge()
        hostBridge = CardHostBridge()
        configuration.websiteDataStore = store
        configuration.mediaTypesRequiringUserActionForPlayback = []
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = true
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
        controller.addScriptMessageHandler(hostBridge, contentWorld: world, name: CardHostBridge.name)
        #if !os(macOS)
        for name in ["cb", "meguri"] { controller.addScriptMessageHandler(bridge, contentWorld: .page, name: name) }
        #endif
    }

    /// A reviewer document with no collection/media access. The root and
    /// bundled runtime use the production loader; all collection paths fail.
    public func makeWarmupPage(size: CGSize = CGSize(width: 320, height: 480)) -> CardPage {
        CardPage(configuration: self, resolver: nil, platform: PlatformClasses.current, appearance: .light,
                 reviewExtra: "<p>Meguri</p>", identifier: "testhooks.reviewWarmup", size: size,
                 diagnostics: CardDiagnostics(), makeWebView: { CardNativeWebView(frame: $0, configuration: $1) })
    }

    /// Must finish before either page navigates, including reviewExtra.
    public func ready() async throws {
        #if !os(macOS)
        if rules == nil {
            let controller = configuration.userContentController
            rules = Task {
                let source = try Self.loopbackRules
                let list: WKContentRuleList = try await withCheckedThrowingContinuation { continuation in
                    WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "meguri.card.loopback.v2", encodedContentRuleList: source) { list, error in
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

    // Content blockers match canonical URLs: numeric IPv4 becomes dotted
    // decimal, IPv6 is compressed, and mapped IPv4 becomes hexadecimal IPv6.
    // WebKit's regex subset has no alternation; keep host forms in separate
    // rules and let JSON serialization handle their backslashes.
    public static var loopbackRules: String {
        get throws {
            let hosts = [
                #"([^/:@?#]*\.)?localhost\.?"#,
                #"127\.[0-9.]+"#,
                #"\[(0*:)*1\]"#,
                #"\[::ffff:127\.[0-9.]+\]"#,
                #"\[::ffff:7f[0-9a-f][0-9a-f]:[0-9a-f]+\]"#,
            ]
            let rules: [[String: Any]] = hosts.map { host in
                let trigger: [String: Any] = ["url-filter": #"^https?://([^/?#@]*@)?"# + host + #"(:[0-9]+)?/"#,
                                              "url-filter-is-case-sensitive": false]
                return ["trigger": trigger, "action": ["type": "block"]]
            }
            return String(decoding: try JSONSerialization.data(withJSONObject: rules), as: UTF8.self)
        }
    }
}
