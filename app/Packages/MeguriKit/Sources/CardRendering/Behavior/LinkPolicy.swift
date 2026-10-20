import Foundation

/// Unknown means the OS query cannot establish whether a handler exists.
/// Only a known absence prevents a user-approved attempt.
public nonisolated enum CardURLAvailability: Equatable, Sendable { case available, unavailable, unknown }

public nonisolated enum CardBrowser: Sendable, Equatable { case external, safari }

public nonisolated struct CardOpenRequest: Sendable, Equatable {
    public let url: URL
    public let browser: CardBrowser
    public let update: CardUpdate
}

/// Shared by navigation and new-window delegates. No navigation produces a
/// review action, and no new web view is created for a card link.
public nonisolated enum LinkPolicy {
    public enum Route: Equatable, Sendable {
        case refuse, javascript, fragment(String), browser(CardBrowser), scheme(String)
    }

    /// Admission only for documents staying inside an embedded frame. This
    /// grants neither native actions nor navigation of the main document.
    static func admitsFrame(_ url: URL, root: URL) -> Bool {
        guard let scheme = canonicalScheme(url.scheme), scheme != "file" else { return false }
        if ["http", "https", "about", "data", "blob"].contains(scheme) { return true }
        return scheme == root.scheme && url.host == root.host && url.port == root.port
            && url.user == nil && url.password == nil
    }

    public static func route(_ url: URL, root: URL, platform: CardRuntime.Platform,
                             preferExternal: Bool) -> Route {
        guard let scheme = canonicalScheme(url.scheme) else { return .refuse }
        if scheme == "file" { return .refuse }
        if scheme == "javascript" { return .javascript }
        var components = URLComponents(url: url, resolvingAgainstBaseURL: true)
        let fragment = components?.fragment
        components?.fragment = nil
        if components?.url == root {
            return fragment.map(Route.fragment) ?? .refuse
        }
        if scheme == "http" || scheme == "https" {
            return .browser(platform == .mac || preferExternal ? .external : .safari)
        }
        // The collection origin and browser-internal documents are not apps.
        if ["meguri-media", "about", "data", "blob"].contains(scheme) { return .refuse }
        return .scheme(scheme)
    }

    public static func canonicalScheme(_ input: String?) -> String? {
        guard let input, !input.isEmpty, input.utf8.count <= 128 else { return nil }
        let lower = input.lowercased()
        let bytes = Array(lower.utf8)
        guard let first = bytes.first, (97...122).contains(first), bytes.allSatisfy({
            (97...122).contains($0) || (48...57).contains($0) || [43, 45, 46].contains($0)
        }) else { return nil }
        return lower
    }
}

/// Only scheme approvals belong here. Settings and collection configuration
/// remain with their owners. Tests inject an isolated defaults suite.
public final class CardSchemeApprovals {
    private let defaults: UserDefaults
    private static let key = "card.approvedURLSchemes"
    public init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    public func contains(_ scheme: String) -> Bool {
        guard let canonical = LinkPolicy.canonicalScheme(scheme), canonical != "file" else { return false }
        return approved.contains(canonical)
    }
    public func approve(_ scheme: String) {
        guard let canonical = LinkPolicy.canonicalScheme(scheme), canonical != "file" else { return }
        var schemes = approved
        schemes.insert(canonical)
        defaults.set(schemes.sorted(), forKey: Self.key)
    }
    private var approved: Set<String> {
        Set((defaults.stringArray(forKey: Self.key) ?? []).compactMap { value in
            guard let scheme = LinkPolicy.canonicalScheme(value), scheme != "file" else { return nil }
            return scheme
        })
    }
}
