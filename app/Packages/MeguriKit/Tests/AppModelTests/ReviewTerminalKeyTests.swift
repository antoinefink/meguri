import Testing
@testable import AppModel

@MainActor @Suite struct ReviewTerminalKeyTests {
    @Test func lastGradeRepeatsAndDuplicateDeliveryCannotFinishTheNewSummary() {
        var gate = ReviewTerminalKeyGate(presentedAt: 100)
        #expect(!gate.take(timestamp: 99, repeated: false, enabled: true))
        #expect(!gate.take(timestamp: 100, repeated: false, enabled: true))
        #expect(!gate.take(timestamp: 101, repeated: true, enabled: true))
        #expect(!gate.take(timestamp: 102, repeated: false, enabled: false))
        #expect(gate.take(timestamp: 103, repeated: false, enabled: true))
        #expect(!gate.take(timestamp: 103, repeated: false, enabled: true))
        #expect(!gate.take(timestamp: 104, repeated: false, enabled: true))
    }
}
