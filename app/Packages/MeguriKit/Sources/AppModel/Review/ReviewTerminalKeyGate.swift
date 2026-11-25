/// One physical press can finish one terminal presentation. Event time fences
/// the grade that created it; repeats never become a second press on Done.
public struct ReviewTerminalKeyGate {
    private let presentedAt: Double
    private var finished = false
    public init(presentedAt: Double) { self.presentedAt = presentedAt }
    public mutating func take(timestamp: Double, repeated: Bool, enabled: Bool) -> Bool {
        guard enabled, !finished, !repeated, timestamp > presentedAt else { return false }
        finished = true
        return true
    }
}
