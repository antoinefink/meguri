#if os(macOS)
import AppKit
import Foundation
import Synchronization
import XCTest

extension XCTestCase {
    /// Read-only probe of the real focused registration and review gates. No
    /// action, focus change, field text or media name crosses this transport.
    @MainActor func reviewInputEvidence(bundle: String) throws -> [String: String] {
        let reply = expectation(description: "Actual review command and input state")
        let received = Mutex<[String: String]?>(nil)
        let center = DistributedNotificationCenter.default()
        let observer = center.addObserver(forName: .init(bundle + ".testhooks.reviewInputReply"), object: nil, queue: .main) { note in
            guard let text = note.object as? String, let data = text.data(using: .utf8),
                  let value = try? JSONSerialization.jsonObject(with: data) as? [String: String] else { return }
            received.withLock { $0 = value }; reply.fulfill()
        }
        defer { center.removeObserver(observer) }
        center.postNotificationName(.init(bundle + ".testhooks.reviewInput"),
            object: "snapshot", userInfo: nil, deliverImmediately: true)
        wait(for: [reply], timeout: 10)
        return try XCTUnwrap(received.withLock { $0 })
    }

    @MainActor func attachReviewInputEvidence(bundle: String, stage: String) throws {
        let fields = try reviewInputEvidence(bundle: bundle)
        let attachment = XCTAttachment(string: fields.sorted { $0.key < $1.key }
            .map { "\($0.key)=\($0.value)" }.joined(separator: "\n"))
        attachment.name = "review-command-state-" + stage
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
#endif
