import Foundation
import Security
internal import MeguriCoreBindings

/// Synchronous secure entropy only. Rust requests bytes at publication
/// confirmation, outside core locks; collection ID randomness is independent.
final class SystemCloudKeyByteProvider: MeguriCoreBindings.CloudKeyByteProvider {
    private let copyBytes: @Sendable (UnsafeMutableRawBufferPointer) -> OSStatus

    init(copyBytes: @escaping @Sendable (UnsafeMutableRawBufferPointer) -> OSStatus = { buffer in
        guard let base = buffer.baseAddress else { return errSecParam }
        return SecRandomCopyBytes(kSecRandomDefault, buffer.count, base)
    }) {
        self.copyBytes = copyBytes
    }

    func drawKey() -> Data? {
        var bytes = Data(count: 32)
        let status = bytes.withUnsafeMutableBytes { copyBytes($0) }
        guard status == errSecSuccess else { return nil }
        return bytes
    }
}
