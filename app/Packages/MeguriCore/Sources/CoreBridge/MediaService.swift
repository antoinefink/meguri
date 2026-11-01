import Foundation
import Instrumentation
internal import MeguriCoreBindings

public nonisolated struct TrackedMediaName: Sendable, Equatable {
    public let name: String
    public let caseKey: String
    public init(name: String, caseKey: String) { self.name = name; self.caseKey = caseKey }
}
public nonisolated struct MediaNameSnapshot: Sendable {
    public let generation: UInt64
    public let root: URL
    public let names: [TrackedMediaName]
    public init(generation: UInt64, root: URL, names: [TrackedMediaName]) {
        self.generation = generation; self.root = root.standardizedFileURL; self.names = names
    }
    init(_ value: MeguriCoreBindings.MediaNameSnapshot) {
        self.init(generation: value.generation, root: URL(fileURLWithPath: value.root),
            names: value.names.map { TrackedMediaName(name: $0.name, caseKey: $0.caseKey) })
    }
}
public protocol MediaService: Sendable {
    func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String
}
public actor CoreMediaService: MediaService {
    public init() {}
    public func trackedNames() async throws(BridgeError) -> MediaNameSnapshot {
        try await InteractiveLane.shared.call(.trackedMediaNames) {
            MediaNameSnapshot(try MeguriCoreBindings.trackedMediaNames())
        }
    }
    public func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String {
        try await InteractiveLane.shared.call(.addMedia) { try MeguriCoreBindings.addMedia(name: name, bytes: Data(bytes)) }
    }
}
