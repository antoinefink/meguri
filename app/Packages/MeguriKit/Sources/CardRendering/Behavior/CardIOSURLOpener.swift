#if os(iOS)
import Foundation
import UIKit

/// Handler availability stays unknown until a user-approved public open attempt.
/// Only its completion establishes whether that attempt succeeded.
final class CardIOSURLOpener {
    private let performOpen: (URL, @escaping @MainActor (Bool) -> Void) -> Void

    init(performOpen: @escaping (URL, @escaping @MainActor (Bool) -> Void) -> Void = { url, completion in
        UIApplication.shared.open(url, options: [:], completionHandler: completion)
    }) {
        self.performOpen = performOpen
    }

    func availability(of url: URL) -> CardURLAvailability {
        guard let scheme = LinkPolicy.canonicalScheme(url.scheme), scheme != "file" else { return .unavailable }
        return .unknown
    }

    func open(_ url: URL, completion: @escaping @MainActor (Bool) -> Void) {
        guard availability(of: url) != .unavailable else { completion(false); return }
        performOpen(url, completion)
    }
}
#endif
