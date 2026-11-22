import Foundation

/// The one attachment capability. No URL or arbitrary media name crosses it.
nonisolated enum CardAttachmentPDF {
    static let maximumBytes = 8 * 1024 * 1024

    static func accepts(_ name: String) -> Bool {
        guard name.utf8.count == 75 else { return false }
        let bytes = Array(name.utf8)
        return bytes.starts(with: Array("meguri-".utf8))
            && bytes.suffix(4).elementsEqual(".pdf".utf8)
            && bytes[7..<71].allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }

    static func read(length: UInt64, chunk: (UInt64, Int) throws -> Data) throws -> Data {
        guard length > 0, length <= UInt64(maximumBytes) else { throw MediaPathResolver.Failure.read }
        var bytes = Data()
        bytes.reserveCapacity(Int(length))
        while bytes.count < Int(length) {
            try Task.checkCancellation()
            let count = min(65_536, Int(length) - bytes.count)
            let part = try chunk(UInt64(bytes.count), count)
            guard !part.isEmpty, part.count <= count else { throw MediaPathResolver.Failure.read }
            bytes.append(part)
        }
        try Task.checkCancellation()
        return bytes
    }
}
