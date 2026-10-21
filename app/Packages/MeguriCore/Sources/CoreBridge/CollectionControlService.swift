import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum SuspendOutcome: Sendable, Hashable {
    case suspended
    case deferred(jobID: UInt64)
    init(_ raw: MeguriCoreBindings.SuspendOutcome) {
        switch raw { case .suspended: self = .suspended; case .deferred(let id): self = .deferred(jobID: id) }
    }
}
public struct ResumeReport: Sendable, Hashable {
    public let externalChange: Bool
    public let rolledOver: Bool
    public let nextDayAt: Int64
    public let activeSession: UInt64?
    public init(externalChange: Bool, rolledOver: Bool, nextDayAt: Int64, activeSession: UInt64?) {
        self.externalChange = externalChange
        self.rolledOver = rolledOver
        self.nextDayAt = nextDayAt
        self.activeSession = activeSession
    }
    init(_ value: MeguriCoreBindings.ResumeReport) {
        externalChange = value.externalChange
        rolledOver = value.rolledOver
        nextDayAt = value.nextDayAt
        activeSession = value.activeSession
    }
}

public protocol CollectionControlService: Sendable {
    func flushForBackground() async throws(BridgeError)
    func suspend(timeoutMilliseconds: UInt32) async throws(BridgeError) -> SuspendOutcome
    func resume() async throws(BridgeError) -> ResumeReport
}
extension CoreCollectionService: CollectionControlService {
    public func flushForBackground() async throws(BridgeError) {
        try await ControlLane.shared.call(.flushForBackground) { try MeguriCoreBindings.flushForBackground() }
    }
    public func suspend(timeoutMilliseconds: UInt32) async throws(BridgeError) -> SuspendOutcome {
        try await ControlLane.shared.call(.suspend) { SuspendOutcome(try MeguriCoreBindings.suspend(timeoutMs: timeoutMilliseconds)) }
    }
    public func resume() async throws(BridgeError) -> ResumeReport {
        try await ControlLane.shared.call(.resume) { ResumeReport(try MeguriCoreBindings.resume()) }
    }
}
