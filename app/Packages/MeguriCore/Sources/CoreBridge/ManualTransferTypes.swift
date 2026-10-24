import Foundation
import Synchronization
internal import MeguriCoreBindings

public enum ManualTransferDirection: String, Sendable, Equatable {
    case `import`, export
    init(_ raw: MeguriCoreBindings.TransferDirection) {
        switch raw {
        case .import: self = .import
        case .export: self = .export
        }
    }
}
public enum ManualTransferScope: String, Sendable, Equatable {
    case collectionOnly, collectionAndMedia
}
public enum ManualMediaStage: String, Sendable, Equatable {
    case awaitingConsent, declined, running, cancelled, interrupted, failed, complete
    init(_ raw: MeguriCoreBindings.TransferMediaStage) {
        switch raw {
        case .awaitingConsent: self = .awaitingConsent
        case .declined: self = .declined
        case .running: self = .running
        case .cancelled: self = .cancelled
        case .interrupted: self = .interrupted
        case .failed: self = .failed
        case .complete: self = .complete
        }
    }
}
public enum ManualMediaPhase: String, Sendable, Equatable {
    case checking, listing, downloading, uploading, confirming, resyncing
    init(_ raw: MeguriCoreBindings.TransferMediaPhase) {
        switch raw {
        case .checking: self = .checking
        case .listing: self = .listing
        case .downloading: self = .downloading
        case .uploading: self = .uploading
        case .confirming: self = .confirming
        case .resyncing: self = .resyncing
        }
    }
}
public struct ManualMediaCounters: Sendable, Equatable {
    public let uploadedFiles: UInt64, uploadedDeletions: UInt64, downloadedFiles: UInt64, downloadedDeletions: UInt64
    public let checked: UInt64, bytesDownloaded: UInt64, bytesUploaded: UInt64, awaiting: UInt64
    public let resynced: Bool, oversized: [String], empty: [String]
    init(_ raw: MeguriCoreBindings.TransferMediaCounters) {
        uploadedFiles = raw.uploadedFiles; uploadedDeletions = raw.uploadedDeletions
        downloadedFiles = raw.downloadedFiles; downloadedDeletions = raw.downloadedDeletions
        checked = raw.checked; bytesDownloaded = raw.bytesDownloaded; bytesUploaded = raw.bytesUploaded
        awaiting = raw.awaiting; resynced = raw.resynced; oversized = raw.oversized; empty = raw.empty
    }
}
public struct ManualMediaOutcome: Sendable, Equatable {
    public let stage: ManualMediaStage, counters: ManualMediaCounters, remoteOutcomeUnknown: Bool, exclusionsVerified: Bool, failureKind: String?
    init(_ raw: MeguriCoreBindings.TransferMediaOutcome) {
        stage = .init(raw.stage); counters = .init(raw.counters)
        remoteOutcomeUnknown = raw.remoteOutcomeUnknown; exclusionsVerified = raw.exclusionsVerified; failureKind = raw.failureKind
    }
}
public struct ManualMediaSnapshot: Sendable, Equatable {
    public let phase: ManualMediaPhase, counters: ManualMediaCounters
    public let listed: UInt64, downloadFiles: UInt64, downloadReceived: UInt64, publishedFiles: UInt64, filesLeft: UInt64
    public let estimateSeconds: Double?
    init(_ raw: MeguriCoreBindings.TransferMediaSnapshot) {
        phase = .init(raw.phase); counters = .init(raw.counters); listed = raw.listed
        downloadFiles = raw.downloadFiles; downloadReceived = raw.downloadReceived
        publishedFiles = raw.publishedFiles; filesLeft = raw.filesLeft; estimateSeconds = raw.estimateSeconds
    }
}
public final class PreparedManualMedia: Sendable {
    let raw: MeguriCoreBindings.ManualMediaPreview?
    public let record: ManualTransferRecord
    init(raw: MeguriCoreBindings.ManualMediaPreview, record: ManualTransferRecord) { self.raw = raw; self.record = record }
    // Internal fixture value carries no backend authority. Production adapters
    // refuse it before starting any work; no foreign id can be reconstructed.
    init(record: ManualTransferRecord) { self.raw = nil; self.record = record }
}
public enum ManualTransferPhase: String, Sendable, Equatable {
    case preparing, preview, confirmedIntent, transferring, remoteAccepted, activating, collectionComplete, mediaIncomplete, superseded, complete, cancelled, failed, needsDecision
    init(_ raw: MeguriCoreBindings.TransferPhase) {
        switch raw {
        case .preparing: self = .preparing
        case .preview: self = .preview
        case .confirmedIntent: self = .confirmedIntent
        case .transferring: self = .transferring
        case .remoteAccepted: self = .remoteAccepted
        case .activating: self = .activating
        case .collectionComplete: self = .collectionComplete
        case .mediaIncomplete: self = .mediaIncomplete
        case .superseded: self = .superseded
        case .complete: self = .complete
        case .cancelled: self = .cancelled
        case .failed: self = .failed
        case .needsDecision: self = .needsDecision
        }
    }
}
public enum ManualTransferAcceptance: String, Sendable, Equatable {
    case notSent, possiblyAccepted, exactOk, refused
    init(_ raw: MeguriCoreBindings.TransferAcceptance) {
        switch raw {
        case .notSent: self = .notSent
        case .possiblyAccepted: self = .possiblyAccepted
        case .exactOk: self = .exactOk
        case .refused: self = .refused
        }
    }
}
public enum ManualTransferBoundary: String, Sendable, Equatable {
    case localChanged, remoteChanged, identityChanged, artifactChanged, remoteCompletionUnknown, activationInterrupted, mediaProviderUnavailable, preparationInterrupted, cancelled, interrupted, storage, server, invalidDownload
    init(_ raw: MeguriCoreBindings.TransferBoundary) {
        switch raw {
        case .localChanged: self = .localChanged
        case .remoteChanged: self = .remoteChanged
        case .identityChanged: self = .identityChanged
        case .artifactChanged: self = .artifactChanged
        case .remoteCompletionUnknown: self = .remoteCompletionUnknown
        case .activationInterrupted: self = .activationInterrupted
        case .mediaProviderUnavailable: self = .mediaProviderUnavailable
        case .preparationInterrupted: self = .preparationInterrupted
        case .cancelled: self = .cancelled
        case .interrupted: self = .interrupted
        case .storage: self = .storage
        case .server: self = .server
        case .invalidDownload: self = .invalidDownload
        }
    }
}
public enum ManualTransferIssue: String, Sendable, Equatable {
    case busy, stale, artifact, cancelled, interrupted, sessionActive, unavailable, diskFull, safetyBackup, invalidDownload, storage, server, needsDecision, mediaIncomplete, cooldown
    init(_ raw: MeguriCoreBindings.TransferIssue) {
        switch raw {
        case .busy: self = .busy
        case .stale: self = .stale
        case .artifact: self = .artifact
        case .cancelled: self = .cancelled
        case .interrupted: self = .interrupted
        case .sessionActive: self = .sessionActive
        case .unavailable: self = .unavailable
        case .diskFull: self = .diskFull
        case .safetyBackup: self = .safetyBackup
        case .invalidDownload: self = .invalidDownload
        case .storage: self = .storage
        case .server: self = .server
        case .needsDecision: self = .needsDecision
        case .mediaIncomplete: self = .mediaIncomplete
        case .cooldown: self = .cooldown
        }
    }
}
public enum ManualTransferRedirectReason: String, Sendable, Equatable {
    case notMeta = "not_meta", notPermanent = "not_308", missingLocation = "missing_location"
    case empty, unparseable, unsupportedScheme = "unsupported_scheme", missingHost = "missing_host"
    case credentials, queryOrFragment = "query_or_fragment", lostHttps = "https_lost"
    case host, repeated, notResendable = "not_resendable"
}
public struct ManualTransferRedirectDiagnostic: Sendable, Equatable {
    public let destination: String?
    public let reason: ManualTransferRedirectReason
    init?(_ raw: MeguriCoreBindings.TransferRedirectDiagnostic) {
        guard let reason = ManualTransferRedirectReason(rawValue: raw.reason) else { return nil }
        self.reason = reason
        if let target = raw.destination, var parts = URLComponents(string: target),
           let scheme = parts.scheme?.lowercased(), ["http", "https"].contains(scheme),
           let host = parts.host, !host.isEmpty {
            parts.user = nil; parts.password = nil; parts.query = nil; parts.fragment = nil
            destination = parts.string
        } else { destination = nil }
    }
}
public enum ManualTransferRequestMethod: String, Sendable, Equatable {
    case hostKey, meta, start, applyGraves, applyChanges, chunk, applyChunk, sanityCheck2
    case finish, abort, upload, download
    case mediaBegin = "msync/begin", mediaChanges = "msync/mediaChanges"
    case uploadChanges = "msync/uploadChanges", downloadFiles = "msync/downloadFiles"
    case mediaSanity = "msync/mediaSanity"
}
public struct ManualTransferFailure: Error, Sendable {
    public let issue: ManualTransferIssue
    public let boundary: ManualTransferBoundary?
    public let generation: UInt64?
    public let candidateActive: Bool
    public let nextEligibleMilliseconds: Int64?
    public let serverKind: String?
    public let requestMethod: ManualTransferRequestMethod?
    public let redirect: ManualTransferRedirectDiagnostic?
    init(_ raw: MeguriCoreBindings.TransferFailure) {
        issue = .init(raw.issue); boundary = raw.boundary.map(ManualTransferBoundary.init)
        generation = raw.generation; candidateActive = raw.candidateActive
        nextEligibleMilliseconds = raw.nextEligibleMs; serverKind = raw.serverKind
        requestMethod = raw.issue == .server ? raw.requestMethod.flatMap(ManualTransferRequestMethod.init(rawValue:)) : nil
        redirect = raw.issue == .server && raw.serverKind == "redirect_refused"
            ? raw.redirect.flatMap(ManualTransferRedirectDiagnostic.init) : nil
    }
}
public struct ManualTransferCounts: Sendable, Equatable {
    public let cards: UInt64, notes: UInt64, reviews: UInt64, pendingReviews: UInt64
    public let pendingRows: [UInt64]
    init(_ raw: MeguriCoreBindings.TransferCounts) {
        cards = raw.cards; notes = raw.notes; reviews = raw.reviews; pendingReviews = raw.pendingReviews; pendingRows = raw.pendingRows
    }
}
public struct ManualTransferCopy: Sendable, Equatable, Identifiable {
    public let name: String, byteLength: UInt64, sha256: [UInt8], verifiedPackage: Bool, instantMilliseconds: Int64?, server: Bool
    public var id: String { name }
    init(_ raw: MeguriCoreBindings.TransferCopy) {
        name = raw.name; byteLength = raw.length; sha256 = Array(raw.sha256); verifiedPackage = raw.verifiedPackage
        instantMilliseconds = raw.instantMs; server = raw.server
    }
}
public struct ManualTransferObservation: Sendable, Equatable {
    public let modifiedMilliseconds: Int64, schemaModifiedMilliseconds: Int64
    public let revision: Int32, mediaRevision: Int32
    init(_ raw: MeguriCoreBindings.TransferObservation) {
        modifiedMilliseconds = raw.modifiedMs; schemaModifiedMilliseconds = raw.schemaModifiedMs; revision = raw.revision; mediaRevision = raw.mediaRevision
    }
}
public struct ManualTransferRecord: Sendable, Identifiable {
    public let revision: UInt64
    public let media: ManualMediaOutcome?
    public let id: String, configuredEndpoint: String, targetEndpoint: String
    public let direction: ManualTransferDirection, phase: ManualTransferPhase, acceptance: ManualTransferAcceptance
    public let boundary: ManualTransferBoundary?, local: ManualTransferCounts?, servedCounts: ManualTransferCounts?
    public let remote: ManualTransferObservation?
    public let remoteHasNoCards: Bool?, preparedMilliseconds: Int64, generation: UInt64?
    public let collectionComplete: Bool, mediaSelected: Bool, copies: [ManualTransferCopy], nextEligibleMilliseconds: Int64?
    init(_ raw: MeguriCoreBindings.TransferRecord) {
        revision = raw.revision; media = raw.media.map(ManualMediaOutcome.init)
        id = raw.id; configuredEndpoint = raw.configuredEndpoint; targetEndpoint = raw.targetEndpoint
        direction = .init(raw.direction); phase = .init(raw.phase); acceptance = .init(raw.acceptance)
        boundary = raw.boundary.map(ManualTransferBoundary.init); local = raw.local.map(ManualTransferCounts.init)
        remote = raw.remote.map(ManualTransferObservation.init)
        servedCounts = raw.servedCounts.map(ManualTransferCounts.init); remoteHasNoCards = raw.remoteHasNoCards
        preparedMilliseconds = raw.preparedMs; generation = raw.generation; collectionComplete = raw.collectionComplete
        mediaSelected = raw.mediaSelected; copies = raw.copies.map(ManualTransferCopy.init); nextEligibleMilliseconds = raw.nextEligibleMs
    }
}
public extension ManualTransferRecord {
    var canResumeMedia: Bool { collectionComplete && mediaSelected && phase != .complete && phase != .superseded }
    var canInspectRecovery: Bool {
        !collectionComplete && acceptance != .possiblyAccepted && [.preview, .needsDecision, .remoteAccepted, .activating].contains(phase)
    }
}
/// Raw keys cross this boundary only for the device-only Keychain store.
/// Never use these values in diagnostics or an operation/recovery record.
public struct ManualTransferCredentials: Sendable {
    public let username: String
    public let hostKey: String
    public init(username: String, hostKey: String) { self.username = username; self.hostKey = hostKey }
}
public final class PreparedManualTransfer: Sendable {
    let raw: MeguriCoreBindings.ManualTransferPreview?
    public let record: ManualTransferRecord
    init(raw: MeguriCoreBindings.ManualTransferPreview, record: ManualTransferRecord) { self.raw = raw; self.record = record }
    // Internal fixture value carries no backend authority. Production adapters
    // refuse it before starting any work; no foreign id can be reconstructed.
    init(record: ManualTransferRecord) { self.raw = nil; self.record = record }
}
public final class ManualTransferHistory: Sendable, Identifiable {
    let raw: MeguriCoreBindings.ManualTransferHistoryEntry?
    public let record: ManualTransferRecord
    public var id: String { record.id }
    init(raw: MeguriCoreBindings.ManualTransferHistoryEntry, record: ManualTransferRecord) { self.raw = raw; self.record = record }
    // Internal fixture value carries no backend authority. Production adapters
    // refuse it before starting any work; no foreign id can be reconstructed.
    init(record: ManualTransferRecord) { self.raw = nil; self.record = record }
}
public enum ManualTransferCancellation: String, Sendable { case user, system }
/// Record the earliest stop request and drain its worker. Inspect the worker's
/// result to learn which effects committed; a stop request cannot undo them.
public final class ManualTransferOperation: Sendable, CollectionExportCancellationChecking {
    private struct State { var cause: ManualTransferCancellation?; var job: CoreJob?; var operationID: String?; var stop: (@Sendable (ManualTransferCancellation) async -> Void)? }
    private let state = Mutex(State())
    // Event seam for deterministic cancellation/drain checks. Delivery happens
    // after releasing state; it carries the same first cause as publication.
    let cancellations: AsyncStream<ManualTransferCancellation>
    private let cancellationContinuation: AsyncStream<ManualTransferCancellation>.Continuation
    public init() {
        (cancellations, cancellationContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    deinit { cancellationContinuation.finish() }
    public var cancellation: ManualTransferCancellation? { state.withLock { $0.cause } }
    public func cancel(_ cause: ManualTransferCancellation = .user) async {
        let (job, first, stop, recorded) = state.withLock { state in
            let first = state.cause == nil
            if first { state.cause = cause }
            return (state.job, first, state.stop, state.cause ?? cause)
        }
        if first { cancellationContinuation.yield(cause); cancellationContinuation.finish() }
        await stop?(recorded)
        try? await job?.cancel()
    }
    func attach(_ job: CoreJob, stop: (@Sendable (ManualTransferCancellation) async -> Void)? = nil) async {
        let cause = state.withLock { state in state.job = job; state.stop = stop; return state.cause }
        if let cause { await stop?(cause); try? await job.cancel() }
    }
    public var operationID: String? { state.withLock { $0.operationID } }
    func identify(_ id: String) {
        state.withLock { state in
            precondition(state.operationID == nil || state.operationID == id)
            state.operationID = id
        }
    }
    func check() throws(BridgeError) { if cancellation != nil { throw .cancelled } }
}
public struct ManualTransferInterrupted: Error, Sendable { public init() {} }

/// Synthetic display values only. Preview/history wrappers have raw == nil;
/// production adapters reject them. This cannot create a session or authority.
nonisolated public enum ManualTransferMediaRenderFixture {
    public enum State: Sendable, Equatable { case consent, progress, partial, complete, unknown }
    public static func record(_ state: State) -> ManualTransferRecord {
        let complete = state == .complete
        let stopped = state == .partial || state == .unknown
        let stage: MeguriCoreBindings.TransferMediaStage = complete ? .complete : (stopped ? .cancelled : (state == .progress ? .running : .awaitingConsent))
        return ManualTransferRecord(.init(id: "render-selected-media", configuredEndpoint: "https://study.example.test/personal/collection/",
            targetEndpoint: "https://study.example.test/personal/collection/", direction: .export,
            phase: complete ? .complete : .mediaIncomplete, acceptance: .notSent,
            boundary: stopped ? .cancelled : nil,
            local: .init(cards: 28_800, notes: 19_200, reviews: 164_320, pendingReviews: 37, pendingRows: [12, 3, 4]),
            remoteHasNoCards: nil, remote: nil, servedCounts: nil,
            preparedMs: 1_894_701_600_000, generation: nil, collectionComplete: true, mediaSelected: true, revision: 1,
            media: .init(stage: stage, counters: counters(complete: complete), remoteOutcomeUnknown: state == .unknown,
                exclusionsVerified: complete, failureKind: stopped ? "cancelled" : nil),
            copies: [], nextEligibleMs: nil))
    }
    public static func history(_ state: State) -> ManualTransferHistory { .init(record: record(state)) }
    public static func preview(_ state: State) -> PreparedManualMedia { .init(record: record(state)) }
    public static var progress: ManualMediaSnapshot {
        .init(.init(phase: .downloading, counters: counters(complete: false), listed: 28_800,
            downloadFiles: 9_600, downloadReceived: 3_200, publishedFiles: 3_200, filesLeft: 6_400, estimateSeconds: 128))
    }
    private static func counters(complete: Bool) -> MeguriCoreBindings.TransferMediaCounters {
        .init(uploadedFiles: complete ? 12_480 : 480, uploadedDeletions: 17,
            downloadedFiles: complete ? 9_600 : 3_200, downloadedDeletions: 23,
            checked: 48_000, bytesDownloaded: complete ? 987_654_321 : 234_567_890, bytesUploaded: 123_456_789,
            resynced: false,
            oversized: complete ? ["f-0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789.wav"] : [],
            empty: complete ? ["f-9876543210987654321098765432109876543210987654321098765432109876543210987654321098765432109876543210.png"] : [],
            awaiting: complete ? 0 : 6_400)
    }
}
