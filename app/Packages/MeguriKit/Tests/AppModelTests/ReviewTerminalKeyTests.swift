import Testing
@testable import AppModel

@MainActor @Suite struct ReviewTerminalKeyTests {
    @Test func lastGradeRepeatsAndDuplicateDeliveryCannotFinishTheNewSummary() {
        var gate = ReviewTerminalKeyGate(presentedAt: 100)
        let results = [
            gate.take(timestamp: 99, repeated: false, enabled: true),
            gate.take(timestamp: 100, repeated: false, enabled: true),
            gate.take(timestamp: 101, repeated: true, enabled: true),
            gate.take(timestamp: 102, repeated: false, enabled: false),
            gate.take(timestamp: 103, repeated: false, enabled: true),
            gate.take(timestamp: 103, repeated: false, enabled: true),
            gate.take(timestamp: 104, repeated: false, enabled: true),
        ]
        #expect(results == [false, false, false, false, true, false, false])
    }
}
