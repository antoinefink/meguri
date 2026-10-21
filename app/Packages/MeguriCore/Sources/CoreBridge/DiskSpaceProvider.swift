import Foundation
internal import MeguriCoreBindings

public protocol DiskSpaceProviding: Sendable {
    func availableBytes(at directory: URL) throws -> UInt64
}

public struct DiskSpaceProvider: DiskSpaceProviding {
    public init() {}
    public func availableBytes(at directory: URL) throws -> UInt64 {
        let values = try directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        guard let bytes = values.volumeAvailableCapacityForImportantUsage, bytes >= 0 else {
            throw BridgeError.unavailable(reason: "Important-usage volume capacity is unavailable")
        }
        return UInt64(bytes)
    }
}

final class DiskSpaceRelay: MeguriCoreBindings.DiskSpaceProvider {
    let provider: any DiskSpaceProviding
    init(_ provider: any DiskSpaceProviding) { self.provider = provider }
    func availableBytes(directory: String) -> MeguriCoreBindings.DiskCapacity {
        do { return .available(bytes: try provider.availableBytes(at: URL(fileURLWithPath: directory))) }
        catch { return .failed(detail: String(describing: error)) }
    }
}
