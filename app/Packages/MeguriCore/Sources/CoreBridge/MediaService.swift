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
    private let expectedGeneration: UInt64?
    public init(expectedGeneration: UInt64? = nil) { self.expectedGeneration = expectedGeneration }
    public func trackedNames() async throws(BridgeError) -> MediaNameSnapshot {
        try await InteractiveLane.shared.call(.trackedMediaNames) {
            MediaNameSnapshot(try MeguriCoreBindings.trackedMediaNames())
        }
    }
    public func addMedia(name: String, bytes: [UInt8]) async throws(BridgeError) -> String {
        let expected = expectedGeneration
        return try await InteractiveLane.shared.call(expected == nil ? .addMedia : .editorAddPicture) {
            if let expected {
                return try MeguriCoreBindings.editorAddPicture(expectedGeneration: expected, name: name, bytes: Data(bytes))
            }
            return try MeguriCoreBindings.addMedia(name: name, bytes: Data(bytes))
        }
    }
}
