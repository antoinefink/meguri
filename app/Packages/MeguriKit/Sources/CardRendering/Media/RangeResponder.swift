import Foundation

public nonisolated struct RangeResponder: Sendable {
    public let status: Int
    public let offset: UInt64
    public let count: UInt64
    public let contentRange: String?

    public init(total: UInt64, header: String?, ifRange: String? = nil, head: Bool = false) {
        var first: UInt64 = 0
        var length = total
        var status = 200
        var contentRange: String?
        if total > 0, !head, ifRange == nil, let header {
            let parts = header.trimmingCharacters(in: CharacterSet(charactersIn: " \t")).split(separator: "=", omittingEmptySubsequences: false)
            if parts.count == 2, parts[0].lowercased() == "bytes" {
                let item = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: " \t"))
                let bounds = item.split(separator: "-", omittingEmptySubsequences: false)
                if bounds.count == 2 {
                    let a = Self.decimal(bounds[0]), b = Self.decimal(bounds[1])
                    if bounds[0].isEmpty, let b {
                        length = min(b.value, total)
                        first = total - length
                        status = length == 0 ? 416 : 206
                    } else if let a, bounds[1].isEmpty || b != nil {
                        if bounds[1].isEmpty || !b!.less(than: a) {
                            first = a.value
                            status = a.value >= total ? 416 : 206
                            length = status == 416 ? 0 : min(b?.value ?? (total - 1), total - 1) - a.value + 1
                        }
                    }
                }
            }
            if status == 206 { contentRange = "bytes \(first)-\(first + length - 1)/\(total)" }
            if status == 416 { contentRange = "bytes */\(total)"; length = 0; first = 0 }
        }
        self.status = status
        offset = first
        count = length
        self.contentRange = contentRange
    }

    private nonisolated struct Decimal {
        let digits: Substring
        let value: UInt64
        func less(than other: Self) -> Bool {
            if digits.count != other.digits.count { return digits.count < other.digits.count }
            return digits.utf8.lexicographicallyPrecedes(other.digits.utf8)
        }
    }
    private static func decimal(_ text: Substring) -> Decimal? {
        guard !text.isEmpty else { return nil }
        var result: UInt64 = 0
        for byte in text.utf8 {
            guard byte >= 48 && byte <= 57 else { return nil }
            let (times, overflow1) = result.multipliedReportingOverflow(by: 10)
            let (next, overflow2) = times.addingReportingOverflow(UInt64(byte - 48))
            result = overflow1 || overflow2 ? UInt64.max : next
        }
        return Decimal(digits: text.drop(while: { $0 == "0" }), value: result)
    }
}
