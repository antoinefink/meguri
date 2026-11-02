import CoreBridge
import Foundation

// J2/J2.2 admission only, not signed evidence. Protocol tests supply explicitly
// fake product/lookup callbacks; the live owner must supply its actual checks.
@MainActor final class AgentAccountAdmission {
    private let originalHeaderBytes: Data
    private let header: AgentHeader
    private let expected: AgentExpectedAccounts
    private var failure: AgentFailure?

    init(originalHeaderBytes: Data, header: AgentHeader,
         productCheck: () throws -> Void,
         applicationSupportRoot: () throws -> URL) throws {
        guard try AgentHeader.read(originalHeaderBytes) == header else { throw AgentFailure.binding }
        // Do not discover/open private setup, much less look up an account,
        // until the actual product checks have all succeeded.
        do { try productCheck() }
        catch { throw (error as? AgentFailure) ?? .execution }
        self.originalHeaderBytes = originalHeaderBytes
        self.header = header
        do {
            expected = try AgentExpectedAccounts(admittedApplicationSupportRoot: applicationSupportRoot(),
                                                 originalHeaderBytes: originalHeaderBytes)
        } catch { throw (error as? AgentFailure) ?? .storage }
    }

    func requireHeader(_ candidate: AgentHeader) throws {
        guard candidate == header else { failure = .binding; throw AgentFailure.binding }
        try revalidate()
    }

    func revalidate() throws {
        if let failure { throw failure }
        do { try expected.revalidate() }
        catch { failure = .storage; throw AgentFailure.storage }
    }

    // The available result cannot escape to Core/the adapter until the exact
    // protected input has survived the suspension and its bytes have matched.
    // A failed owner cannot regain authority from a later successful lookup.
    func resolveAccount(using lookup: () async throws -> CloudResolvedAccount?) async throws -> CloudResolvedAccount {
        try revalidate()
        do {
            guard !Task.isCancelled else { throw AgentFailure.execution }
            let account = try await lookup()
            try revalidate()
            guard !Task.isCancelled else { throw AgentFailure.execution }
            guard let account, account.status == .available, let identity = account.identity,
                  !identity.isEmpty else { throw AgentFailure.wrongContainer }
            try expected.requireMatch(Array(identity), label: "acct1")
            try revalidate()
            return account
        } catch {
            let fixed = (error as? AgentFailure) ?? .execution
            failure = failure ?? fixed
            throw failure!
        }
    }
}
