#if DEBUG && MEGURI_TEST_HOOKS
import Foundation

nonisolated enum CloudKitSelectedRestoration {
    static func bytes(_ digest: String) throws -> Data {
        guard AgentJSON.digest(digest) else { throw AgentFailure.recovery }
        let chars = Array(digest)
        return try Data(stride(from: 0, to: chars.count, by: 2).map {
            guard let byte = UInt8(String(chars[$0...($0 + 1)]), radix: 16) else { throw AgentFailure.recovery }
            return byte
        })
    }
}
#endif
