import Foundation
import Instrumentation
internal import MeguriCoreBindings

public protocol CollectionClock: Sendable {
    func nowMilliseconds() -> Int64
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32
    func zoneIdentifier() -> String
}
public struct SystemCollectionClock: CollectionClock {
    public init() {}
    public func nowMilliseconds() -> Int64 { Int64(Date().timeIntervalSince1970 * 1_000) }
    public func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 {
        Int32(TimeZone.current.secondsFromGMT(for: Date(timeIntervalSince1970: Double(atMilliseconds) / 1_000)))
    }
    public func zoneIdentifier() -> String { TimeZone.current.identifier }
}
final class CollectionClockRelay: MeguriCoreBindings.HostClock {
    let clock: any CollectionClock
    init(_ clock: any CollectionClock) { self.clock = clock }
    func nowMs() -> Int64 { clock.nowMilliseconds() }
    func offsetFromUtc(instantMs: Int64) -> Int32 {
        clock.utcOffsetSeconds(atMilliseconds: instantMs)
    }
    func zoneId() -> String { clock.zoneIdentifier() }
}

public struct CollectionLocation: Sendable, Hashable {
    public let collectionDirectory: URL
    public let backupsDirectory: URL
    public let manualTransferDirectory: URL
    public let appGroupSnapshot: URL
    public init(collectionDirectory: URL, backupsDirectory: URL, appGroupSnapshot: URL, manualTransferDirectory: URL = URL.applicationSupportDirectory.appending(path: "ManualTransfers", directoryHint: .isDirectory)) {
        self.collectionDirectory = collectionDirectory
        self.backupsDirectory = backupsDirectory
        self.appGroupSnapshot = appGroupSnapshot
        self.manualTransferDirectory = manualTransferDirectory
    }
    var raw: MeguriCoreBindings.CollectionLocation {
        .init(collectionDirectory: collectionDirectory.path, backupsDirectory: backupsDirectory.path,
              appGroupSnapshot: appGroupSnapshot.path, manualTransferDirectory: manualTransferDirectory.path)
    }
}

public protocol CollectionService: Sendable {
    func localPublicationInformation(generation: UInt64) async throws(BridgeError) -> LocalPublicationInformation
    func open(_ location: CollectionLocation, randomSeed: UInt64) async throws(BridgeError)
    func close() async throws(BridgeError)
    func lifecycle() async throws(BridgeError) -> CollectionLifecycle
    func waitAvailable(timeoutMilliseconds: UInt32) async throws(BridgeError) -> UInt64
}

public actor CoreCollectionService: CollectionService {
    private let clock: CollectionClockRelay
    private let diskSpace: DiskSpaceRelay
    public init(clock: any CollectionClock = SystemCollectionClock(), diskSpace: any DiskSpaceProviding = DiskSpaceProvider()) {
        self.clock = CollectionClockRelay(clock)
        self.diskSpace = DiskSpaceRelay(diskSpace)
    }
    public func localPublicationInformation(generation: UInt64) async throws(BridgeError) -> LocalPublicationInformation {
        try await MaintenanceLane.shared.call(.localPublicationInformation) {
            LocalPublicationInformation(try MeguriCoreBindings.localPublicationInformation(generation: generation))
        }
    }
    /// Use startOpen when the caller displays progress and explicit Cancel.
    public func startOpen(_ location: CollectionLocation, randomSeed: UInt64) async throws(BridgeError) -> CoreJob {
        let clock = clock
        let diskSpace = diskSpace
        return try await Jobs.start(.startOpen) {
            try MeguriCoreBindings.startOpen(location: location.raw, clock: clock, diskSpace: diskSpace, randomSeed: randomSeed)
        }
    }
    public func open(_ location: CollectionLocation, randomSeed: UInt64) async throws(BridgeError) {
        let job = try await startOpen(location, randomSeed: randomSeed)
        try await job.value()
    }
    /// Retains the original process Host without bootstrapping an absent file.
    public func prepareCloudConnectionHost(_ location: CollectionLocation, randomSeed: UInt64) async throws(BridgeError) -> UInt64 {
        let clock = clock, diskSpace = diskSpace
        return try await MaintenanceLane.shared.call(.prepareCloudConnectionHost) {
            try MeguriCoreBindings.prepareCloudConnectionHost(location: location.raw, clock: clock,
                diskSpace: diskSpace, randomSeed: randomSeed)
        }
    }
    public func close() async throws(BridgeError) {
        try await ControlLane.shared.call(.closeCollection) { try closeCollection() }
    }
    public func lifecycle() async throws(BridgeError) -> CollectionLifecycle {
        try await ControlLane.shared.call(.collectionLifecycle) { CollectionLifecycle(try collectionLifecycle()) }
    }
    public func waitAvailable(timeoutMilliseconds: UInt32) async throws(BridgeError) -> UInt64 {
        try await waitAvailable(timeoutMilliseconds: timeoutMilliseconds, using: {
            try waitCollectionAvailable(timeoutMs: 0)
        })
    }

    // Keep each probe on ControlLane; suspend between probes on this service.
    // The provider seam also lets tests observe the lane and refusal handling.
    func waitAvailable(
        timeoutMilliseconds: UInt32, using probe: @Sendable () throws -> UInt64
    ) async throws(BridgeError) -> UInt64 {
        let timer = ContinuousClock()
        let deadline = timer.now + .milliseconds(Int64(timeoutMilliseconds))
        while true {
            guard !Task.isCancelled else { throw .cancelled }
            do throws(BridgeError) {
                let generation = try await ControlLane.shared.call(.waitCollectionAvailable, probe)
                guard !Task.isCancelled else { throw BridgeError.cancelled }
                return generation
            } catch {
                guard !Task.isCancelled else { throw .cancelled }
                guard case .unavailable = error else { throw error }
                let now = timer.now
                guard now < deadline else { throw error }
                do {
                    try await timer.sleep(until: min(deadline, now + .milliseconds(10)))
                } catch {
                    if error is CancellationError { throw .cancelled }
                    throw BridgeError(error)
                }
            }
        }
    }
}

/// Scripted terminal results for previews; exhaustion is an explicit error.
public actor FakeCollectionService: CollectionService {
    public enum Call: Sendable, Hashable {
        case open(CollectionLocation, seed: UInt64), close, lifecycle, waitAvailable(UInt32), localPublicationInformation(UInt64)
    }
    public private(set) var calls: [Call] = []
    private var opens: [Result<Void, BridgeError>]
    private var closes: [Result<Void, BridgeError>]
    private var states: [Result<CollectionLifecycle, BridgeError>]
    private var waits: [Result<UInt64, BridgeError>]
    private var information: [Result<LocalPublicationInformation, BridgeError>]
    public init(opens: [Result<Void, BridgeError>] = [], closes: [Result<Void, BridgeError>] = [],
                states: [Result<CollectionLifecycle, BridgeError>] = [], waits: [Result<UInt64, BridgeError>] = [],
                information: [Result<LocalPublicationInformation, BridgeError>] = []) {
        self.opens = opens; self.closes = closes; self.states = states; self.waits = waits
        self.information = information
    }
    public func localPublicationInformation(generation: UInt64) throws(BridgeError) -> LocalPublicationInformation {
        calls.append(.localPublicationInformation(generation))
        guard !information.isEmpty else { throw .unavailable(reason: "No scripted local publication information") }
        return try information.removeFirst().get()
    }
    public func open(_ location: CollectionLocation, randomSeed: UInt64) throws(BridgeError) {
        calls.append(.open(location, seed: randomSeed))
        guard !opens.isEmpty else { throw .unavailable(reason: "No scripted open result") }
        try opens.removeFirst().get()
    }
    public func close() throws(BridgeError) {
        calls.append(.close)
        guard !closes.isEmpty else { throw .unavailable(reason: "No scripted close result") }
        try closes.removeFirst().get()
    }
    public func lifecycle() throws(BridgeError) -> CollectionLifecycle {
        calls.append(.lifecycle)
        guard !states.isEmpty else { throw .unavailable(reason: "No scripted lifecycle result") }
        return try states.removeFirst().get()
    }
    public func waitAvailable(timeoutMilliseconds: UInt32) throws(BridgeError) -> UInt64 {
        calls.append(.waitAvailable(timeoutMilliseconds))
        guard !waits.isEmpty else { throw .unavailable(reason: "No scripted wait result") }
        return try waits.removeFirst().get()
    }
}
