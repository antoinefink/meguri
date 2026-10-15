import MetricKit
import os

/// Emits the registered intervals (R1861). Each interval is an os_signpost
/// interval in `IntervalNames.subsystem` and its area's category, which is
/// what Instruments shows and what `XCTOSSignpostMetric` measures. A
/// field-reported interval is also a MetricKit signpost under the same name
/// and category (`MetricManager.logHandle(category:)`), so the field
/// diagnostics store (R1867) receives its daily statistics. Bridge calls are
/// os_signposts only.
public enum Signposts {
    /// Begins `interval`; end it with `end(_:)`, on any thread.
    public static func begin(_ interval: IntervalName) -> IntervalToken {
        let signposter = signposter(for: interval.category)
        let id = signposter.makeSignpostID()
        let state = signposter.beginInterval(interval.staticName, id: id)
        var field: (OSLog, OSSignpostID)?
        if interval.fieldReported {
            let log = metricKitLog(for: interval.category)
            let fieldID = OSSignpostID(log: log)
            mxSignpost(.begin, log: log, name: interval.staticName, signpostID: fieldID)
            field = (log, fieldID)
        }
        return IntervalToken(interval: interval, state: state, field: field)
    }

    /// Ends the interval `token` began.
    public static func end(_ token: IntervalToken) {
        signposter(for: token.interval.category).endInterval(token.interval.staticName, token.state)
        if let (log, id) = token.field {
            mxSignpost(.end, log: log, name: token.interval.staticName, signpostID: id)
        }
    }

    /// Runs `body` inside `interval`.
    public static func measure<T, E: Error>(_ interval: IntervalName, _ body: () throws(E) -> T) throws(E) -> T {
        let token = begin(interval)
        defer { end(token) }
        return try body()
    }

    /// Runs one call into the core inside its bridge interval, with the lane
    /// as the interval's message (CoreBridge lanes, KTD9).
    public static func bridge<T, E: Error>(_ call: BridgeCall, lane: String, _ body: () throws(E) -> T) throws(E) -> T {
        let signposter = signposters.bridge
        let state = signposter.beginInterval(call.interval.staticName, id: signposter.makeSignpostID(), "\(lane)")
        defer { signposter.endInterval(call.interval.staticName, state) }
        return try body()
    }

    // MARK: Logs, one per category

    private struct Signposters: Sendable {
        let review = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.review.rawValue)
        let launch = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.launch.rawValue)
        let search = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.search.rawValue)
        let browse = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.browse.rawValue)
        let stats = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.stats.rawValue)
        let cardInfo = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.cardInfo.rawValue)
        let editor = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.editor.rawValue)
        let sync = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.sync.rawValue)
        let bridge = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.bridge.rawValue)
        let probe = OSSignposter(subsystem: IntervalNames.subsystem, category: IntervalCategory.probe.rawValue)
    }

    private static let signposters = Signposters()

    private static func signposter(for category: IntervalCategory) -> OSSignposter {
        switch category {
        case .review: signposters.review
        case .launch: signposters.launch
        case .search: signposters.search
        case .browse: signposters.browse
        case .stats: signposters.stats
        case .cardInfo: signposters.cardInfo
        case .editor: signposters.editor
        case .sync: signposters.sync
        case .bridge: signposters.bridge
        case .probe: signposters.probe
        }
    }

    /// MetricKit's log handles, made once per category.
    private static let metricKitLogs: [IntervalCategory: OSLog] = Dictionary(
        uniqueKeysWithValues: IntervalCategory.allCases.map { ($0, MetricManager.logHandle(category: $0.rawValue)) }
    )

    private static func metricKitLog(for category: IntervalCategory) -> OSLog {
        metricKitLogs[category] ?? MetricManager.logHandle(category: category.rawValue)
    }
}

/// A begun interval.
public struct IntervalToken: Sendable {
    public let interval: IntervalName
    let state: OSSignpostIntervalState
    let field: (OSLog, OSSignpostID)?
}
