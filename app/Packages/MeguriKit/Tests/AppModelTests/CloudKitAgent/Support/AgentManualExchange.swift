// The one original manual exchange. No mutation, session, retry or replacement
// exchange is admitted here. The production backend identifies its own issued
// exchange and reads only that exchange's actual terminal record.
@MainActor enum AgentManualExchange {
    static func run<Result>(prepare: @MainActor () throws -> Void, request: @MainActor () -> Bool,
                            afterRequest: @MainActor () async throws -> Void,
                            exactResult: @MainActor () async throws -> Result,
                            afterResult: @MainActor () async throws -> Void) async throws -> Result {
        try prepare()
        guard request() else { throw AgentFailure.execution }
        try await afterRequest()
        let result = try await exactResult()
        try await afterResult()
        return result
    }
}
