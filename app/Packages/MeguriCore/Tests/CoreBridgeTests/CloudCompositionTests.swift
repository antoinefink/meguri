import Foundation
import Testing
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
