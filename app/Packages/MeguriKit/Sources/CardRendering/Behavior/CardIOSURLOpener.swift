#if os(iOS)
import Foundation
import UIKit

/// canOpenURL's negative result is authoritative only for eligible queries.
/// Undeclared device schemes instead reach the public open completion after
/// the page's per-scheme approval. No finite query list limits card links.
final class CardIOSURLOpener {
    private let queryEligible: (URL) -> Bool
    private let query: (URL) -> Bool
    private let performOpen: (URL, @escaping @MainActor (Bool) -> Void) -> Void

    init(queryEligible: @escaping (URL) -> Bool = { url in
        let declared = Bundle.main.object(forInfoDictionaryKey: "LSApplicationQueriesSchemes") as? [String] ?? []
        return declared.contains { $0.lowercased() == url.scheme?.lowercased() }
    }, query: @escaping (URL) -> Bool = { UIApplication.shared.canOpenURL($0) },
         performOpen: @escaping (URL, @escaping @MainActor (Bool) -> Void) -> Void = { url, completion in
             UIApplication.shared.open(url, options: [:], completionHandler: completion)
         }) {
        self.queryEligible = queryEligible
        self.query = query
        self.performOpen = performOpen
    }

    func availability(of url: URL) -> CardURLAvailability {
        guard let scheme = LinkPolicy.canonicalScheme(url.scheme), scheme != "file" else { return .unavailable }
        guard queryEligible(url) else { return .unknown }
        return query(url) ? .available : .unavailable
    }

    func open(_ url: URL, completion: @escaping @MainActor (Bool) -> Void) {
        guard availability(of: url) != .unavailable else { completion(false); return }
        performOpen(url, completion)
    }
}
#endif
