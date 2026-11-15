import Foundation
import Testing
import MeguriCoreBindings
@testable import CoreBridge

/// Successful ordinary delivery leaves durable ack evidence after the inbox
/// settles. Queue residency is evidence only for entries still waiting to apply.
func expectAppliedCloudIncoming(_ fixture: CloudKitBridgeFixture) async throws {
    let facts = try #require(try await fixture.core.entityFacts(name: fixture.incoming.name))
    #expect(facts.payloadDigest == facts.ackedPayloadDigest)
    #expect(facts.ackedSystemFields == fixture.incoming.systemFields)
    #expect(!(try await fixture.core.durableSnapshot()).inboxNames.contains(fixture.incoming.name))
}

extension EventTests {
    @Test func fetchedNativeEventSettlesThroughTheRealCoreAndRedeliveryIsSafe() async throws {
        try await withCloudKitFixture { fixture in
            try await fixture.qualifyCompleteEpoch()
            let engine = try await fixture.engine()
            #expect(try await fixture.core.entityFacts(name: fixture.incoming.name) == nil)
            await engine.emitRetained(.fetched(zone: fixture.zone, records: [fixture.incoming], deleted: []), label: "applyIncoming")
            #expect(await fixture.adapter.lastFailure == nil)
            let facts = try #require(try await fixture.core.entityFacts(name: fixture.incoming.name))
            #expect(facts.payloadDigest == facts.ackedPayloadDigest)
            #expect(facts.ackedSystemFields == fixture.incoming.systemFields)
            let applied = try await fixture.core.durableSnapshot()
            #expect(!applied.inboxNames.contains(fixture.incoming.name))
            await engine.emitRetained(.fetched(zone: fixture.zone, records: [fixture.incoming], deleted: []), label: "redelivery")
            #expect(await fixture.adapter.lastFailure == nil)
            #expect(try await fixture.core.entityFacts(name: fixture.incoming.name) == facts)
            let cursor = Data([0x31, 0x32, 0x33])
            await engine.emitRetained(.stateUpdate(cursor), label: "afterApply")
            #expect(try await fixture.core.durableSnapshot().serialization == cursor)
        }
    }
}

extension MaintenanceLane {
    fileprivate func interruptCloudIncoming(cancel: Bool) throws {
        try call("test.cloudInterruptNextIncoming") {
            try MeguriCoreBindings.testCloudInterruptNextIncoming(cancel: cancel)
        }
    }
    fileprivate func referenceWaitRecord(erev: UInt64) throws -> CoreBridge.CloudRecord {
        try call("test.cloudReferenceWaitRecord") {
            CoreBridge.CloudRecord(try MeguriCoreBindings.testCloudReferenceWaitRecord(erev: erev))
        }
    }
}

extension EventTests {
    /// Real rollback (constraint or cancellation) must not retire native authority.
    @Test(arguments: [false, true])
    func nativeIncomingRollbackKeepsSDKAndDurableQueue(cancel: Bool) async throws {
        try await withCloudKitFixture(keepAlive: true) { fixture in
            let engine = try await fixture.engine()
            let sdk = engine.instanceID
            try await MaintenanceLane.shared.interruptCloudIncoming(cancel: cancel)
            await engine.emitRetained(.fetched(zone: fixture.zone, records: [fixture.incoming], deleted: []), label: "rolledBackIncoming")
            try await fixture.adapter.waitForIdle()
            #expect(await fixture.adapter.lastFailure == nil)
            #expect(await fixture.adapter.retainedInstanceIDs() == [sdk])
            let queued = try await fixture.core.durableSnapshot()
            #expect(queued.inboxNames.contains(fixture.incoming.name))
            #expect(!queued.trackingStopped)
            #expect(try await fixture.core.entityFacts(name: fixture.incoming.name) == nil)
            let trace = await engine.recordedTrace()
            #expect(!trace.contains(.cancelEntered) && !trace.contains(.released))
            let cursor = Data([0x64, 0x65, 0x66])
            await engine.emitRetained(.stateUpdate(cursor), label: "cursorAfterRollback")
            let after = try await fixture.core.durableSnapshot()
            #expect(after.serialization == cursor)
            #expect(after.inboxNames == queued.inboxNames)
            #expect(await fixture.factory.engines().count == 1)
            #expect(await fixture.adapter.retainedInstanceIDs() == [sdk])
        }
    }

    /// Separate from the applied, same-erev stale-redelivery equality test.
    @Test func nativeSnapshotDistinguishesSameNameUnappliedInboxContent() async throws {
        try await withCloudKitFixture { fixture in
            let engine = try await fixture.engine()
            let firstRecord = try await MaintenanceLane.shared.referenceWaitRecord(erev: 2)
            let newerRecord = try await MaintenanceLane.shared.referenceWaitRecord(erev: 3)
            #expect(firstRecord.name == newerRecord.name)
            #expect(firstRecord.envelope != newerRecord.envelope)
            await engine.emitRetained(.fetched(zone: fixture.zone, records: [firstRecord], deleted: []), label: "referenceWait")
            let first = try await fixture.core.durableSnapshot()
            await engine.emitRetained(.fetched(zone: fixture.zone, records: [newerRecord], deleted: []), label: "newerReferenceWait")
            let second = try await fixture.core.durableSnapshot()
            #expect(await fixture.adapter.lastFailure == nil)
            #expect(first.inboxNames.count == 1 && first.inboxNames == second.inboxNames)
            #expect(first.inboxNames.contains(firstRecord.name))
            #expect(first.serialization == second.serialization)
            #expect(first.trackingContent != second.trackingContent)
            #expect(first != second)
            #expect(try await fixture.core.entityFacts(name: firstRecord.name) == nil)
        }
    }
}
