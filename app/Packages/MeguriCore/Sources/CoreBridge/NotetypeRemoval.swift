import Instrumentation
internal import MeguriCoreBindings

/// S12: each loss category the native Delete Note Type question confirmed.
public struct NotetypeRemoval: Sendable, Hashable {
    public let notetypeId: Int64
    public let confirmedNotes: UInt64
    public let confirmedCards: UInt64
    /// Authoritative collection generation that supplied the deletion question.
    /// Nil retains the legacy current-collection caller's behavior.
    public let expectedGeneration: UInt64?
    public init(notetypeId: Int64, confirmedNotes: UInt64, confirmedCards: UInt64, expectedGeneration: UInt64? = nil) {
        self.notetypeId = notetypeId
        self.confirmedNotes = confirmedNotes
        self.confirmedCards = confirmedCards
        self.expectedGeneration = expectedGeneration
    }
    var raw: MeguriCoreBindings.NotetypeRemoval {
        .init(notetypeId: notetypeId, confirmedNotes: confirmedNotes, confirmedCards: confirmedCards)
    }
}

public struct NotetypeRemoved: Sendable, Hashable {
    public let summary: MutationSummary
    public let id: Int64
    public let removed: Bool
    public let notesRemoved: UInt64
    public let cardsRemoved: UInt64
    init(_ raw: MeguriCoreBindings.NotetypeRemoved) {
        summary = MutationSummary(raw.summary)
        id = raw.id
        removed = raw.removed
        notesRemoved = raw.notesRemoved
        cardsRemoved = raw.cardsRemoved
    }
}

extension CoreNotetypeService {
    public func startNotetypeRemoval(_ request: NotetypeRemoval) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeRemoved> {
        try await startNotetypeRemoval(request, trackingSetup: { try Jobs.track($0) })
    }

    func startNotetypeRemoval(
        _ request: NotetypeRemoval,
        trackingSetup: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob
    ) async throws(BridgeError) -> CoreNotetypeReadJob<NotetypeRemoved> {
        try await CoreEventStream.shared.start()
        let expectedGeneration = request.expectedGeneration
        let request = request.raw
        let call: BridgeCall = expectedGeneration == nil ? .startNotetypeRemoval : .startNotetypeRemovalForGeneration
        let started = try await MaintenanceLane.shared.call(call) {
            if let expectedGeneration {
                return try MeguriCoreBindings.startNotetypeRemovalForGeneration(request: request, generation: expectedGeneration)
            } else {
                return try MeguriCoreBindings.startNotetypeRemoval(request: request)
            }
        }
        let job = await Jobs.trackMutationDraining(started.handle, setup: trackingSetup)
        let result = started.result
        return CoreNotetypeReadJob(job: job, resultCall: .notetypeRemovalJobResult) {
            try result.result().map(NotetypeRemoved.init)
        }
    }
}
