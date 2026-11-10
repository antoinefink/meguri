import Testing
@testable import CoreBridge

struct NativeCloudSavedRemovalTests {
    @Test
    func fullPendingUpdateKeepsMatchingEntriesAndAppliesRealChanges() {
        let pending: [String: CloudPendingChange] = ["kept": .save, "changed": .delete, "readded": .save, "edited": .save]
        let additions: [String: CloudPendingChange] = ["kept": .save, "changed": .save, "readded": .save, "new": .delete, "edited": .save]
        let full = CloudPendingUpdate(id: 1, kind: .full, adds: additions,
            removes: ["readded", "absent"], forcedAdditions: ["edited"])
        let changes = CloudPendingUpdate(id: 2, kind: .changes, adds: additions, removes: ["readded"])
        let applied = full.mutations(to: pending)
        #expect(applied.adds == ["changed": .save, "readded": .save, "new": .delete, "edited": .save])
        #expect(applied.removes == ["readded"])
        #expect(changes.mutations(to: pending) == changes)
    }

    @Test
    func nativeSavedRemovalRequiresActualAbsenceAfterOriginalIssue() {
        var proof = NativeCloudSavedRemoval()
        let issue = proof.capture(pending: ["n": .save], generation: 7)
        proof.handed(42, names: ["n"], issue: issue)
        proof.sent(42, names: ["n"])
        proof.observe(instance: "original", pending: ["n": .save], generation: 7)
        #expect(proof.take(generation: 7).isEmpty)
        proof.observe(instance: "original", pending: [:], generation: 7)
        #expect(proof.take(generation: 7) == [.init(instanceID: "original", requestID: 42, names: ["n"])])
        #expect(proof.take(generation: 7).isEmpty)
    }

    @Test(arguments: [false, true])
    func nativeNewPendingUpdateCannotMasqueradeAsOriginalRemoval(readded: Bool) {
        var proof = NativeCloudSavedRemoval()
        proof.handed(42, names: ["n"], issue: proof.capture(pending: ["n": .save], generation: 7))
        proof.sent(42, names: ["n"])
        // A Core removal or a new revision's re-add is an application mutation.
        proof.applicationWillChangePending()
        proof.observe(instance: "original", pending: readded ? ["n": .save] : [:], generation: 7)
        #expect(proof.take(generation: 7).isEmpty)
        proof.observe(instance: "original", pending: [:], generation: 7)
        #expect(proof.take(generation: 7).isEmpty)
    }

    @Test
    func nativeRemovalObservedBeforeReaddRetainsOnlyOriginalReceipt() {
        var proof = NativeCloudSavedRemoval()
        proof.handed(42, names: ["n"], issue: proof.capture(pending: ["n": .save], generation: 7))
        proof.sent(42, names: ["n"])
        proof.observe(instance: "original", pending: [:], generation: 7)
        proof.applicationWillChangePending()
        proof.observe(instance: "original", pending: ["n": .save], generation: 7)
        #expect(proof.take(generation: 7) == [.init(instanceID: "original", requestID: 42, names: ["n"])])
    }

    @Test(arguments: [false, true])
    func nativeFailedOrUnknownSavedCannotConfirm(unknown: Bool) {
        var proof = NativeCloudSavedRemoval()
        proof.handed(42, names: ["n"], issue: proof.capture(pending: ["n": .save], generation: 7))
        proof.sent(unknown ? 43 : 42, names: unknown ? ["n"] : [])
        proof.observe(instance: "original", pending: [:], generation: 7)
        #expect(proof.take(generation: 7).isEmpty)
    }

    @Test(arguments: [false, true])
    func nativeChangedNetworkGenerationRefusesUnconfirmedAndReadyProof(ready: Bool) {
        var proof = NativeCloudSavedRemoval()
        proof.handed(42, names: ["n"], issue: proof.capture(pending: ["n": .save], generation: 7))
        proof.sent(42, names: ["n"])
        if ready { proof.observe(instance: "original", pending: [:], generation: 7) }
        proof.observe(instance: "original", pending: [:], generation: 8)
        #expect(proof.take(generation: 8).isEmpty)
    }
}
