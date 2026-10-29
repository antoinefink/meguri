import Instrumentation
internal import MeguriCoreBindings

/// A synchronous admission seam for already prepared coordinator work. It
/// never awaits, reads providers, performs UI work or calls back into core.
public protocol CloudPolicyQueue: Sendable { func enqueue() -> Bool }
private final class ForeignPolicyQueue: MeguriCoreBindings.CloudPolicyQueue {
    let queue: any CloudPolicyQueue
    init(_ queue: any CloudPolicyQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}

/// Retains the original Host policy. Native control work must run outside SDK
/// callbacks, in the emitted order. No Swift policy or completion inference.
public actor CoreCloudPolicyService {
    private let raw: MeguriCoreBindings.CloudPolicy
    init(_ raw: MeguriCoreBindings.CloudPolicy) { self.raw = raw }

    nonisolated static func call<T: Sendable>(_ name: BridgeCall, _ body: @Sendable @escaping () throws -> T) async throws -> T {
        try await MaintenanceLane.shared.call(name, body)
    }

    public func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let event = event.raw
        return try await Self.call(.cloudPolicyInput) { [raw] in CloudPolicyOutput(try raw.input(event: event)) }
    }
    public func refreshAccount() async throws -> CloudPolicyOutput {
        let raw = self.raw
        return try await Self.call(.cloudPolicyRefreshAccount) { [raw] in CloudPolicyOutput(try raw.refreshAccount()) }
    }
    /// Latest applied Driver snapshot. Feed Refresh after core events and
    /// consume its commands before reading status; this read emits no work.
    public func status() async throws -> CloudPolicyStatus {
        let raw = self.raw
        return try await Self.call(.cloudPolicyStatus) { [raw] in CloudPolicyStatus(try raw.status()) }
    }
    public func executeCore(_ command: CloudPolicyCommand) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let command = command.raw
        return try await Self.call(.cloudPolicyExecuteCore) { [raw] in CloudPolicyOutput(try raw.executeCore(command: command)) }
    }
    /// Prepare before calling; enqueue synchronously during the callback;
    /// await completion afterwards. An accepted result means queue admission only.
    public func submitNative(_ command: CloudPolicyCommand, queue: any CloudPolicyQueue) async throws -> CloudPolicyAdmission {
        let raw = self.raw
        let command = command.raw
        let queue = ForeignPolicyQueue(queue)
        return try await Self.call(.cloudPolicySubmitNative) { [raw] in CloudPolicyAdmission(try raw.submitNative(command: command, queue: queue)) }
    }
    public func captureExchange(_ command: CloudPolicyCommand) async throws -> CloudPolicyCapture {
        let raw = self.raw
        let command = command.raw
        return try await Self.call(.cloudPolicyCaptureExchange) { [raw] in CloudPolicyCapture(try raw.captureExchange(command: command)) }
    }
    public func completeExchange(_ exchange: UInt64) async throws -> CloudPolicyOutput {
        let raw = self.raw
        return try await Self.call(.cloudPolicyCompleteExchange) { [raw] in CloudPolicyOutput(try raw.completeExchange(exchange: exchange)) }
    }
    public func endBeforeCapture(_ exchange: UInt64, outcome: CloudPolicyEarlyEnd) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let outcome = outcome.raw
        return try await Self.call(.cloudPolicyEndBeforeCapture) { [raw] in CloudPolicyOutput(try raw.endBeforeCapture(exchange: exchange, outcome: outcome)) }
    }
}
