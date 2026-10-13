import MeguriCoreBindings
import Testing

@testable import CoreBridge

@Suite struct ErrorTests {
    static let cases: [(CoreError, BridgeError)] = [
        (.Cancelled, .cancelled),
        (.Busy(runningOp: "Check Database"), .busy(runningOperation: "Check Database")),
        (.Unavailable(reason: "exclusive phase"), .unavailable(reason: "exclusive phase")),
        (.Unsupported(feature: "servers"), .unsupported(feature: "servers")),
        (.InvalidArgument(message: "deck id 0"), .invalidArgument("deck id 0")),
        (.Internal(message: "invariant"), .internal("invariant")),
    ]

    @Test(arguments: cases)
    func aRustErrorArrivesAsItsTypedBridgeError(core: CoreError, expected: BridgeError) async {
        await #expect(throws: expected) {
            try await MaintenanceLane.shared.call("test.returnError") { try testReturnError(error: core) }
        }
    }

    @Test func aRustPanicArrivesAsAnInternalErrorAndTheProcessSurvives() async throws {
        let error = await #expect(throws: BridgeError.self) {
            try await MaintenanceLane.shared.call("test.panic") { try testPanic(message: "deliberate test panic") }
        }
        guard case .internal(let message) = error else {
            Issue.record("expected .internal, got \(String(describing: error))")
            return
        }
        #expect(message.contains("deliberate test panic"))
        // The core is still usable after the caught panic.
        #expect(try await Core.version() == (try VersionTests.workspaceVersion()))
    }
}
