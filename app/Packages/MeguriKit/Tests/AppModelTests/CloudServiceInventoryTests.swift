#if os(macOS)
@testable import CoreBridge
import Foundation
import Testing

@Suite nonisolated struct CloudServiceInventoryTests {
    @Test func ownInventoryUsesOriginalCompleteServiceRequestAndFreshRecreation() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "service-inventory-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let process = try await CloudServiceProcess.open(stateDirectory: root)
        do {
            let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
            let transport = ServiceOwnTransport(wire: wire)
            try await wire.createZone("c-a")
            try await wire.createZone("c-b")
            let original = try transport.prepare(.zoneInventory)
            try #require(original.enqueue())
            let removed = try await wire.call(.object(["DeleteZone": .object(["zone": .string("c-a"), "cause": .string("Deleted")])]))
            try #require(try removed.ok().text == "Done")
            guard case .zoneInventory(let held) = try await original.reply() else { throw ServiceWorkflowFailure.wire }
            #expect(held == ["c-a", "c-b"])
            let afterRemoval = try transport.prepare(.zoneInventory)
            try #require(afterRemoval.enqueue())
            guard case .zoneInventory(let remaining) = try await afterRemoval.reply() else { throw ServiceWorkflowFailure.wire }
            #expect(remaining == ["c-b"])
            try await wire.createZone("c-a")
            let fresh = try transport.prepare(.zoneInventory)
            try #require(fresh.enqueue())
            guard case .zoneInventory(let recreated) = try await fresh.reply() else { throw ServiceWorkflowFailure.wire }
            #expect(recreated == ["c-a", "c-b"])
            let exit = await process.close()
            #expect(exit.status == 0 && exit.joined && !exit.ioFailed && !exit.unexpectedOutput)
        } catch { _ = await process.cancel(); throw error }
    }
}
#endif
