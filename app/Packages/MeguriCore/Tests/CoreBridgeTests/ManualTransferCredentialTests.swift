import Testing
@testable import CoreBridge

@Suite struct ManualTransferCredentialTests {
    @Test func cancellationKeepsFirstCauseWhenAttachArrivesLater() async {
        let operation = ManualTransferOperation()
        await operation.cancel(.system); await operation.cancel(.user)
        #expect(operation.cancellation == .system)
        #expect(throws: BridgeError.self) { try operation.check() }
    }
}
