import Instrumentation
internal import MeguriCoreBindings

public struct DayClockReport: Sendable, Hashable {
    public let today: UInt32
    public let nextDayAt: Int64
    public let rolledOverFrom: UInt32?
    public init(today: UInt32, nextDayAt: Int64, rolledOverFrom: UInt32?) {
        self.today = today; self.nextDayAt = nextDayAt; self.rolledOverFrom = rolledOverFrom
    }
}

public enum FsrsPolicyStatus: Sendable, Hashable {
    case ok, needsEnable, needsReEnable
}

extension Core {
    public static func checkRollover() async throws(BridgeError) -> DayClockReport {
        try await InteractiveLane.shared.call(.checkRollover) {
            let report = try MeguriCoreBindings.checkRollover()
            return DayClockReport(today: report.today, nextDayAt: report.nextDayAt, rolledOverFrom: report.rolledOverFrom)
        }
    }
    public static func fsrsPolicyStatus() async throws(BridgeError) -> FsrsPolicyStatus {
        try await QueryLane.shared.call(.fsrsPolicyStatus) {
            switch try MeguriCoreBindings.fsrsPolicyStatus() {
            case .ok: .ok
            case .needsEnable: .needsEnable
            case .needsReEnable: .needsReEnable
            }
        }
    }
    public static func enableFsrs() async throws(BridgeError) -> CoreJob {
        try await Jobs.start(.enableFsrs) { try MeguriCoreBindings.enableFsrs() }
    }
}
