import Foundation
import Instrumentation
internal import MeguriCoreBindings

public protocol MediaService: Sendable {
    func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String
}
public actor CoreMediaService: MediaService {
    public init() {}
    public func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String {
        try await InteractiveLane.shared.call(.addMedia) { try MeguriCoreBindings.addMedia(name: name, bytes: Data(bytes)) }
    }
}
