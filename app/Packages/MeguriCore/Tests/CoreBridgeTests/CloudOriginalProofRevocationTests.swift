import Foundation
import Synchronization
import Testing
@testable import CoreBridge

struct CloudOriginalProofRevocationTests {
    @Test(arguments: ["original", "otherOwner", "noRequest", "unownedCapture", "earlierFailure"])
    func revocationRequiresPendingProofAndItsRecordedOriginalRequest(mode: String) async throws {
        let original = CloudPolicyCancellationContext(), other = CloudPolicyCancellationContext()
        let observed = Mutex<CloudPolicyCancellationContext.Snapshot?>(nil)
        let proof = CloudOriginalCommandProofYield(cancellationIntent: original, originalRevocation: { cause in
            observed.withLock { $0 = cause }
        })
        let failure = BridgeError.internal("synthetic-earlier-proof-failure")
        if mode == "unownedCapture" { _ = original.capture() }
        if mode != "noRequest" { original.record(.replacement, sequence: UUID()) }
        other.record(.replacement, sequence: UUID())
        if mode == "earlierFailure" { proof.finish(failure) }
        proof.revoke(cancellation: mode == "otherOwner" ? other : original)
        #expect(!proof.isPending)
        let cause = observed.withLock { $0 }
        #expect((cause != nil) == (mode == "original"))
        if let cause {
            #expect(cause.ownerID == original.id && cause.request?.source == .replacement)
        }
        do { try await proof.wait(); Issue.record("Revoked proof unexpectedly succeeded") }
        catch {
            if mode == "earlierFailure" { #expect((error as? BridgeError) == failure) }
            else { #expect(error is CancellationError) }
        }
    }
}
