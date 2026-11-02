#if DEBUG && MEGURI_TEST_HOOKS
import Foundation
import Testing

// Dedicated CloudAgent plan only. A successful test return requires actual
// smoke execution; setup evidence is attached and the test remains failing.
@MainActor @Suite(.serialized) struct CloudKitSoakTests {
    @Test func signedGeneratedMutationAndSyncNow() async throws {
        guard ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil,
              let path = ProcessInfo.processInfo.environment["MEGURI_CLOUD_AGENT_RUN_DIRECTORY"],
              path.hasPrefix("/") else {
            Issue.record("Cloud agent requires its dedicated run directory; signed runtime UNRUN")
            return
        }
        let admitted = try AgentRunDirectory.admitActual(expectedPath: path)
        let directory = admitted.directory
        let storage = admitted.storage!
        let originalHeaderBytes = admitted.originalHeaderBytes
        let runtime = CloudKitSmokeRuntime(admittedRun: admitted)
        let engine = try AgentEngine(storage: storage, runtime: runtime, instance: "launch-" + UUID().uuidString)
        do {
            let deliveries = try CloudKitAgentDeliveries(directory: directory)
            defer { deliveries.cancel() }
            var completed = false
            for await _ in deliveries.stream {
                try admitted.revalidate()
                let journal = try await engine.deliver(header: originalHeaderBytes, commands: storage.read(.delivery), retainedJournal: storage.read(.retained))
                try admitted.revalidate()
                let records = try AgentJSON.lines(journal).complete.map { try AgentRecord.read($0) }
                guard !records.contains(where: { $0.kind == .failed }) else { throw AgentFailure.execution }
                // Serve the exact ten-action program through its durable,
                // explicit joined close terminal. A mutation/exchange is partial.
                if AgentEngine.smokeCompleted(records) { completed = true; break }
                if records.contains(where: { $0.kind == .started && $0.action == .close }) { throw AgentFailure.execution }
            }
            guard completed else { throw AgentFailure.execution }
            try admitted.revalidate()
            try storage.replace(.status, with: JSONEncoder().encode(SmokeStatus(outcome: "smoke-completed", failure: nil, user_steps: [], dependencies: [])))
        } catch {
            // These fields are fixed protocol labels, never error descriptions.
            let failure = (error as? AgentFailure) ?? .execution
            let awaiting = failure.disposition == .awaitingUser
            let status = SmokeStatus(outcome: failure.disposition.rawValue, failure: failure, user_steps: awaiting ? runtime.userSteps : [], dependencies: runtime.missing)
            let bytes = try JSONEncoder().encode(status)
            try await runtime.cancelAndJoin()
            try admitted.revalidate()
            try storage.replace(.status, with: bytes)
            Attachment.record(bytes, named: "cloud-agent-status.json")
            Issue.record(awaiting ? "Signed smoke awaiting_user; prerequisites absent; runtime UNRUN" : "Signed smoke failed; no signed acceptance")
        }
    }
}

// App-local status consumed by the MG coordinator; this is not an MG result
// envelope and cannot supply a signed-lane or soak passing scope cell.
private nonisolated struct SmokeStatus: Encodable {
    let scope = "smoke"
    let signed_acceptance = false
    let outcome: String
    let failure: AgentFailure?
    let user_steps: [String]
    let dependencies: [CloudKitSmokeRuntime.Dependency]
}
#endif
