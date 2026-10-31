import Foundation
import Testing
@testable import CoreBridge

struct CloudKitEngineFakeTests {
    @Test func manualFetchRoutesExplicitZonesAndRetainsLegacyUnscopedRecords() async throws {
        let root = CloudRecord(name: "active", envelope: Data([1]), systemFields: nil,
                               recordType: "MGActive", zone: "meguri-root")
        let collection = CloudRecord(name: "note", envelope: Data([2]), systemFields: nil,
                                     zone: "meguri-collection")
        let legacy = CloudRecord(name: "legacy", envelope: Data([3]), systemFields: nil)
        let delegate = FakeRoutingDelegate()
        let engine = CloudKitEngineFake(
            configuration: CloudEngineConfiguration(instanceID: "routing", automaticallySync: false,
                                                     serialization: nil, collectionZone: "meguri-collection"),
            delegate: delegate, a23: true, keepAlive: false)
        let zones = ["meguri-root", "meguri-collection", "empty-zone"]
        await engine.appendFetchScripts([
            .manual(fetchedZones: zones, records: [root, collection, legacy], group: nil),
            .manual(fetchedZones: zones, records: [root, collection], group: nil)
        ])
        let options = CloudFetchOptions(zones: zones, priorityZones: [], groupID: "routing")
        try await engine.fetchChanges(options: options)
        try await engine.fetchChanges(options: options)
        #expect(await delegate.events == [
            .willFetch(groupID: "routing", reason: .manual),
            .willFetchZone(zone: zones[0]),
            .fetched(zone: zones[0], records: [root, legacy], deleted: []),
            .didFetchZone(zone: zones[0], error: nil),
            .willFetchZone(zone: zones[1]),
            .fetched(zone: zones[1], records: [collection, legacy], deleted: []),
            .didFetchZone(zone: zones[1], error: nil),
            .willFetchZone(zone: zones[2]),
            .fetched(zone: zones[2], records: [legacy], deleted: []),
            .didFetchZone(zone: zones[2], error: nil),
            .didFetch(groupID: "routing", reason: .manual),
            .willFetch(groupID: "routing", reason: .manual),
            .willFetchZone(zone: zones[0]),
            .fetched(zone: zones[0], records: [root], deleted: []),
            .didFetchZone(zone: zones[0], error: nil),
            .willFetchZone(zone: zones[1]),
            .fetched(zone: zones[1], records: [collection], deleted: []),
            .didFetchZone(zone: zones[1], error: nil),
            .willFetchZone(zone: zones[2]),
            .didFetchZone(zone: zones[2], error: nil),
            .didFetch(groupID: "routing", reason: .manual)
        ])
    }
}

private actor FakeRoutingDelegate: CloudKitEngineDelegate {
    private(set) var events: [CloudEngineEvent] = []
    func eventWillDeliver(engine: any CloudKitEngine) async {}
    func handleEvent(_ event: CloudEngineEvent, engine: any CloudKitEngine) async { events.append(event) }
    func eventHandlerReturned(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {}
    func nextRecordZoneChangeBatch(pending: [String: CloudPendingChange], zone: String,
                                   engine: any CloudKitEngine) async -> CloudEngineBatch? { nil }
    func nextFetchChangesOptions(_ options: CloudFetchOptions, engine: any CloudKitEngine) async -> CloudFetchOptions { options }
}
