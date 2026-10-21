import Foundation
import Instrumentation
internal import MeguriCoreBindings

public struct MutationSummary: Sendable, Hashable {
    public let seq: UInt64
    public let generation: UInt64
    public let undoName: String?
    public let changes: ChangeSet
    public let pastCancelPoint: Bool
    public init(seq: UInt64, generation: UInt64, undoName: String?, changes: ChangeSet, pastCancelPoint: Bool) {
        self.seq = seq
        self.generation = generation
        self.undoName = undoName
        self.changes = changes
        self.pastCancelPoint = pastCancelPoint
    }
    init(_ value: MeguriCoreBindings.MutationSummary) {
        seq = value.seq
        generation = value.generation
        undoName = value.undoName
        changes = ChangeSet(value.changes)
        pastCancelPoint = value.pastCancelPoint
    }
}

public struct CountMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let count: UInt64
    public init(summary: MutationSummary, count: UInt64) {
        self.summary = summary
        self.count = count
    }
    init(_ value: MeguriCoreBindings.CountMutation) {
        summary = MutationSummary(value.summary)
        count = value.count
    }
}

public struct BoolMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let written: Bool
    public init(summary: MutationSummary, written: Bool) {
        self.summary = summary
        self.written = written
    }
    init(_ value: MeguriCoreBindings.BoolMutation) {
        summary = MutationSummary(value.summary)
        written = value.written
    }
}

