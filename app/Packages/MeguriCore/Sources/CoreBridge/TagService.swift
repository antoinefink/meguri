import Foundation
import Instrumentation
internal import MeguriCoreBindings

public struct TagNode: Sendable, Hashable {
    public let name: String
    public let fullName: String
    public let collapsed: Bool
    public let implicit: Bool
    public let children: [TagNode]
    public init(name: String, fullName: String, collapsed: Bool, implicit: Bool, children: [TagNode]) {
        self.name = name
        self.fullName = fullName
        self.collapsed = collapsed
        self.implicit = implicit
        self.children = children
    }
    init(_ value: MeguriCoreBindings.TagNode) {
        name = value.name
        fullName = value.fullName
        collapsed = value.collapsed
        implicit = value.implicit
        children = value.children.map(TagNode.init)
    }
}

public final class CoreTagMutationJob: Sendable {
    public let job: CoreJob
    private let raw: MeguriCoreBindings.TagMutationJob
    init(job: CoreJob, raw: MeguriCoreBindings.TagMutationJob) { self.job = job; self.raw = raw }
    public func value() async throws(BridgeError) -> CountMutation {
        try await job.value()
        let raw = raw
        return try await MaintenanceLane.shared.call(.tagMutationJobResult) {
            guard let result = try raw.result() else { throw BridgeError.internal("Successful job has no result") }
            return CountMutation(result)
        }
    }
}

public protocol TagService: Sendable {
    func tagTree() async throws(BridgeError) -> [TagNode]
    func flagNames() async throws(BridgeError) -> [String]
    func setTagsCollapsed(names: [String], collapsed: Bool) async throws(BridgeError) -> CountMutation
    func renameFlag(flag: UInt8, name: String) async throws(BridgeError) -> BoolMutation
    func restoreFlagName(flag: UInt8) async throws(BridgeError) -> BoolMutation
    func addNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CountMutation
    func startAddNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CoreTagMutationJob
    func removeNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CountMutation
    func startRemoveNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CoreTagMutationJob
    func setCardFlag(cards: [Int64], flag: UInt64) async throws(BridgeError) -> CountMutation
    func startSetCardFlag(cards: [Int64], flag: UInt64) async throws(BridgeError) -> CoreTagMutationJob
    func toggleCardFlag(current: Int64, cards: [Int64], chosen: UInt8) async throws(BridgeError) -> CountMutation
    func startToggleCardFlag(current: Int64, cards: [Int64], chosen: UInt8) async throws(BridgeError) -> CoreTagMutationJob
    func markNotes(notes: [Int64]) async throws(BridgeError) -> CountMutation
    func startMarkNotes(notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob
    func unmarkNotes(notes: [Int64]) async throws(BridgeError) -> CountMutation
    func startUnmarkNotes(notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob
    func toggleNoteMark(current: Int64, notes: [Int64]) async throws(BridgeError) -> CountMutation
    func startToggleNoteMark(current: Int64, notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob
    func startRenameTag(from: String, to: String) async throws(BridgeError) -> CoreTagMutationJob
    func startReparentTags(sources: [String], target: String?) async throws(BridgeError) -> CoreTagMutationJob
    func startDeleteTags(text: String) async throws(BridgeError) -> CoreTagMutationJob
    func startDeleteTags(text: String, expectedGeneration: UInt64) async throws(BridgeError) -> CoreTagMutationJob
    func startClearUnusedTags() async throws(BridgeError) -> CoreTagMutationJob
}

extension TagService {
    public func startDeleteTags(text: String, expectedGeneration: UInt64) async throws(BridgeError) -> CoreTagMutationJob {
        throw .invalidArgument("Collection-scoped tag deletion is unavailable")
    }
}

public actor CoreTagService: TagService {
    public init() {}
    public func tagTree() async throws(BridgeError) -> [TagNode] {
        try await QueryLane.shared.call(.tagTree) { try MeguriCoreBindings.tagTree().map(TagNode.init) }
    }
    public func flagNames() async throws(BridgeError) -> [String] {
        try await QueryLane.shared.call(.flagNames) { try MeguriCoreBindings.flagNames() }
    }
    public func setTagsCollapsed(names: [String], collapsed: Bool) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.setTagsCollapsed) { CountMutation(try MeguriCoreBindings.setTagsCollapsed(names: names, collapsed: collapsed)) }
    }
    public func renameFlag(flag: UInt8, name: String) async throws(BridgeError) -> BoolMutation {
        try await InteractiveLane.shared.call(.renameFlag) { BoolMutation(try MeguriCoreBindings.renameFlag(flag: flag, name: name)) }
    }
    public func restoreFlagName(flag: UInt8) async throws(BridgeError) -> BoolMutation {
        try await InteractiveLane.shared.call(.restoreFlagName) { BoolMutation(try MeguriCoreBindings.restoreFlagName(flag: flag)) }
    }
    public func addNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.addNoteTags) { CountMutation(try MeguriCoreBindings.addNoteTags(notes: notes, text: text)) }
    }
    public func startAddNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startAddNoteTags) {
            let raw = try MeguriCoreBindings.startAddNoteTags(notes: notes, text: text)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func removeNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.removeNoteTags) { CountMutation(try MeguriCoreBindings.removeNoteTags(notes: notes, text: text)) }
    }
    public func startRemoveNoteTags(notes: [Int64], text: String) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startRemoveNoteTags) {
            let raw = try MeguriCoreBindings.startRemoveNoteTags(notes: notes, text: text)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func setCardFlag(cards: [Int64], flag: UInt64) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.setCardFlag) { CountMutation(try MeguriCoreBindings.setCardFlag(cards: cards, flag: flag)) }
    }
    public func startSetCardFlag(cards: [Int64], flag: UInt64) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startSetCardFlag) {
            let raw = try MeguriCoreBindings.startSetCardFlag(cards: cards, flag: flag)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func toggleCardFlag(current: Int64, cards: [Int64], chosen: UInt8) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.toggleCardFlag) { CountMutation(try MeguriCoreBindings.toggleCardFlag(current: current, cards: cards, chosen: chosen)) }
    }
    public func startToggleCardFlag(current: Int64, cards: [Int64], chosen: UInt8) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startToggleCardFlag) {
            let raw = try MeguriCoreBindings.startToggleCardFlag(current: current, cards: cards, chosen: chosen)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func markNotes(notes: [Int64]) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.markNotes) { CountMutation(try MeguriCoreBindings.markNotes(notes: notes)) }
    }
    public func startMarkNotes(notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startMarkNotes) {
            let raw = try MeguriCoreBindings.startMarkNotes(notes: notes)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func unmarkNotes(notes: [Int64]) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.unmarkNotes) { CountMutation(try MeguriCoreBindings.unmarkNotes(notes: notes)) }
    }
    public func startUnmarkNotes(notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startUnmarkNotes) {
            let raw = try MeguriCoreBindings.startUnmarkNotes(notes: notes)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func toggleNoteMark(current: Int64, notes: [Int64]) async throws(BridgeError) -> CountMutation {
        try await InteractiveLane.shared.call(.toggleNoteMark) { CountMutation(try MeguriCoreBindings.toggleNoteMark(current: current, notes: notes)) }
    }
    public func startToggleNoteMark(current: Int64, notes: [Int64]) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startToggleNoteMark) {
            let raw = try MeguriCoreBindings.startToggleNoteMark(current: current, notes: notes)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func startRenameTag(from: String, to: String) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startRenameTag) {
            let raw = try MeguriCoreBindings.startRenameTag(from: from, to: to)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func startReparentTags(sources: [String], target: String?) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startReparentTags) {
            let raw = try MeguriCoreBindings.startReparentTags(sources: sources, target: target)
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
    public func startDeleteTags(text: String) async throws(BridgeError) -> CoreTagMutationJob {
        try await startDeleteTags(text: text, trackingSetup: { try Jobs.track($0) })
    }
    /// Pass the authoritative generation from the scope that offered the menu.
    /// Keep it across event-stream and lane waits; never refresh it here.
    public func startDeleteTags(text: String, expectedGeneration: UInt64) async throws(BridgeError) -> CoreTagMutationJob {
        try await startDeleteTags(text: text, expectedGeneration: expectedGeneration, trackingSetup: { try Jobs.track($0) })
    }
    func startDeleteTags(
        text: String,
        expectedGeneration: UInt64? = nil,
        trackingSetup: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob
    ) async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        let call: BridgeCall = expectedGeneration == nil ? .startDeleteTags : .startDeleteTagsForGeneration
        let raw = try await MaintenanceLane.shared.call(call) {
            if let expectedGeneration {
                return try MeguriCoreBindings.startDeleteTagsForGeneration(text: text, generation: expectedGeneration)
            } else {
                return try MeguriCoreBindings.startDeleteTags(text: text)
            }
        }
        let job = await Jobs.trackMutationDraining(raw.handle, setup: trackingSetup)
        return CoreTagMutationJob(job: job, raw: raw.result)
    }
    public func startClearUnusedTags() async throws(BridgeError) -> CoreTagMutationJob {
        try await CoreEventStream.shared.start()
        return try await MaintenanceLane.shared.call(.startClearUnusedTags) {
            let raw = try MeguriCoreBindings.startClearUnusedTags()
            return CoreTagMutationJob(job: try Jobs.track(raw.handle), raw: raw.result)
        }
    }
}
