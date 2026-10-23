@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

@Suite @MainActor struct ManualTransferSettingsTests {
    @Test func offlineLoadingNeverLogsInOrTransfers() async {
        let service = SettingsTransferService()
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil)
        #expect(await service.calls.isEmpty)
        await model.loadOffline(); await model.loadOffline()
        #expect(await service.calls == ["history"])
        #expect(model.preview == nil && model.stage == .ready)
    }
    @Test func loginClearsPasswordAndReturnsWithoutPreparation() async {
        let service = SettingsTransferService()
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil)
        model.endpointDraft = "http://127.0.0.1:28711/prefix/"
        await model.applyEndpoint()
        model.select(.export)
        model.username = "Synthetic User"; model.password = "ephemeral-only"
        await model.login()
        #expect(model.password.isEmpty && model.authenticated && model.direction == .export)
        #expect(model.preview == nil)
        #expect(await service.calls == ["normalize", "login"])
        #expect(await keys.saved?.username == "Synthetic User")
        #expect(await keys.saved?.hostKey == "synthetic-host-key")
    }
    @Test func endpointAndLogoutClearLoginButPreserveOfflineHistory() async {
        let service = SettingsTransferService()
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil)
        model.endpointDraft = "http://127.0.0.1:28711/prefix/"; await model.applyEndpoint()
        model.username = "Synthetic User"; model.password = "ephemeral"; await model.login()
        model.endpointDraft = "http://127.0.0.1:28712/other/"; await model.applyEndpoint()
        #expect(!model.authenticated && model.username.isEmpty && model.password.isEmpty)
        #expect(await keys.saved == nil)
        await model.logout(); await model.refreshOfflineHistory()
        #expect(await service.calls == ["normalize", "login", "normalize", "history"])
    }
    @Test func defaultsContainOnlyEndpointAndLoginCancellationDoesNotPrepare() async throws {
        let suite = "meguri.manual-transfer.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let service = SettingsTransferService()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: SettingsTransferKeys(), defaults: defaults)
        model.endpointDraft = "http://127.0.0.1:28711/"; await model.applyEndpoint()
        model.username = "Synthetic User"; model.password = "never-persist-me"; await model.login()
        #expect(defaults.persistentDomain(forName: suite)?.keys.sorted() == ["manualTransfer.endpoint"])
        model.password = "clear-on-background"; await model.cancelAndDrain()
        #expect(model.password.isEmpty)
        #expect(await service.calls == ["normalize", "login"])
    }
}
private actor SettingsTransferKeys: ManualTransferCredentialStore {
    private(set) var saved: ManualTransferCredentials?
    func load(endpoint: String) -> ManualTransferCredentials? { saved }
    func save(_ credentials: ManualTransferCredentials, endpoint: String) { saved = credentials }
    func clear() { saved = nil }
}
private actor SettingsTransferService: ManualTransferService {
    private(set) var calls: [String] = []
    private let historyUnavailable: Bool
    private let suspendPreparation: Bool
    private let entered: AsyncStream<Void>
    private let enteredContinuation: AsyncStream<Void>.Continuation
    init(historyUnavailable: Bool = false, suspendPreparation: Bool = false) {
        self.historyUnavailable = historyUnavailable
        self.suspendPreparation = suspendPreparation
        (entered, enteredContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func waitUntilPreparing() async {
        var iterator = entered.makeAsyncIterator()
        _ = await iterator.next()
    }
    func normalizeEndpoint(_ text: String) -> String { calls.append("normalize"); return text }
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) -> ManualTransferCredentials {
        calls.append("login"); return .init(username: username, hostKey: "synthetic-host-key")
    }
    func history(operation: ManualTransferOperation) throws -> [ManualTransferHistory] {
        calls.append("history")
        if historyUnavailable { throw BridgeError.unavailable(reason: "Scripted offline history failure") }
        return []
    }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> PreparedManualTransfer {
        calls.append("prepare")
        guard suspendPreparation else { throw BridgeError.unavailable(reason: "No scripted preparation") }
        // The host regression verifies the real worker-to-policy mapping. This
        // service scripts its delivered id, independently of optional history.
        operation.identify("scripted-policy-operation")
        enteredContinuation.yield(())
        enteredContinuation.finish()
        var iterator = operation.cancellations.makeAsyncIterator()
        _ = await iterator.next()
        throw BridgeError.cancelled
    }
    func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> ManualTransferRecord { calls.append("confirm"); throw BridgeError.unavailable(reason: "No scripted confirmation") }
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { calls.append("recover"); throw BridgeError.unavailable(reason: "No scripted recovery") }
    func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) { calls.append("cancelPreparation") }
    func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) { calls.append("discard") }
    func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to: URL, operation: ManualTransferOperation) { calls.append("saveCopy") }
}


private func decisionRecord(phase: ManualTransferPhase = .preview, acceptance: ManualTransferAcceptance = .notSent) -> ManualTransferRecord {
    // Inferred binding values remain confined to a testable CoreBridge mapping.
    ManualTransferRecord(.init(id: "scripted-policy-operation", configuredEndpoint: "https://example.test/prefix/",
        targetEndpoint: "https://example.test/approved/", direction: .export,
        phase: phase == .cancelled ? .cancelled : .preview,
        acceptance: acceptance == .exactOk ? .exactOk : .notSent, boundary: nil,
        local: .init(cards: 11, notes: 6, reviews: 8, pendingReviews: 2, pendingRows: [1, 2, 3, 4, 5, 6, 7, 8]),
        remoteHasNoCards: true,
        remote: .init(modifiedMs: 1_900_000_001_234, schemaModifiedMs: 1_899_000_005_678, revision: 19, mediaRevision: 4),
        servedCounts: nil, preparedMs: 1_900_000_010_000, generation: nil, collectionComplete: false,
        mediaSelected: false, revision: 0, media: nil,
        copies: [.init(name: "local.colpkg", length: 10, sha256: Data(repeating: 0x11, count: 32), verifiedPackage: true, instantMs: nil, server: false),
                 .init(name: "served.raw", length: 20, sha256: Data(repeating: 0x22, count: 32), verifiedPackage: false, instantMs: 1_900_000_003_456, server: true)],
        nextEligibleMs: nil))
}

extension ManualTransferSettingsTests {
    @Test func finalDecisionIncludesPostCopyObservationsAndBothCopyIdentities() throws {
        let record = decisionRecord()
        let decision = record.decisionFacts
        let values = Dictionary(uniqueKeysWithValues: decision.map { ($0.id, $0.value) })
        let exactValues = [
            "endpoint": record.configuredEndpoint,
            "target": record.targetEndpoint,
            "local": "11 cards, 6 notes, 8 saved reviews.",
            "pending": "2 reviews; 36 other changed records.",
            "remoteBaseline": "Observed after retaining the server copy.",
            "copy.0.name": "local.colpkg",
            "copy.1.name": "served.raw",
            "copy.0.kind": "Verified collection package",
            "copy.0.identity": String(repeating: "11", count: 32),
            "copy.1.identity": String(repeating: "22", count: 32),
            "copy.0.time": "Creation time was not recorded."
        ]
        for (id, expected) in exactValues {
            #expect(try #require(values[id]) == expected)
        }

        let display = DateFormatter()
        display.dateStyle = .medium
        display.timeStyle = .short
        let observedDates: [(String, Date)] = [
            ("remoteModified", Date(timeIntervalSince1970: 1_900_000_001.234)),
            ("remoteSchema", Date(timeIntervalSince1970: 1_899_000_005.678)),
            ("copy.1.time", Date(timeIntervalSince1970: 1_900_000_003.456))
        ]
        for (id, instant) in observedDates {
            #expect(try #require(values[id]) == display.string(from: instant))
        }

        let changeMarker = try #require(values["remoteRevision"])
        #expect(changeMarker.starts(with: "19."))
        #expect(changeMarker.contains("does not guarantee"))
        #expect(try #require(values["remoteMediaRevision"]).starts(with: "4."))
        let requiredDisclosures = [
            "serverCounts": "unknown",
            "copy.1.kind": "Unverified raw file",
            "scope": "Collection only"
        ]
        for (id, text) in requiredDisclosures {
            #expect(try #require(values[id]).contains(text))
        }
        for fact in decision {
            #expect(!fact.label.contains("usn"))
            #expect(!fact.label.contains("scm"))
        }
    }
    @Test func missingCancellationMetadataIsNotPresentedAsUserIntent() {
        let model = ManualTransferCoordinator(owner: nil, service: SettingsTransferService(), defaults: nil)
        #expect(model.cancellationDescription(for: decisionRecord(phase: .cancelled)) == "Stopped before completion. The reason was not recorded.")
    }
}

extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func historyFailureDuringLifecycleCancellationKeepsFirstCauseAcrossRestart(userFirst: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        try #require(owner.state == .noFile)
        let suite = "meguri.manual-attribution.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let service = SettingsTransferService(historyUnavailable: true, suspendPreparation: true)
        let model = ManualTransferCoordinator(owner: owner, service: service, keychain: SettingsTransferKeys(), defaults: defaults)
        await model.loadOffline() // History fails before any preparation.
        model.endpointDraft = "http://127.0.0.1:28711/prefix/"
        await model.applyEndpoint()
        model.username = "Synthetic User"; model.password = "transient"
        await model.login()
        try #require(model.canPrepare)
        let preparation = Task { await model.prepare() }
        await service.waitUntilPreparing()
        if userFirst { await model.cancel() }
        await model.cancelAndDrain()
        await preparation.value
        let expected: ManualTransferCancellation = userFirst ? .user : .system
        #expect(model.cancellationCause(for: "scripted-policy-operation") == expected)
        // The refresh following cancellation fails too. No pair of successful
        // history reads is available to correlate this operation.
        #expect(await service.calls == ["history", "normalize", "login", "prepare", "history"])
        let restarted = ManualTransferCoordinator(owner: nil, service: service, defaults: defaults)
        #expect(restarted.cancellationCause(for: "scripted-policy-operation") == expected)
        #expect(restarted.cancellationDescription(for: decisionRecord(phase: .cancelled)) == (userFirst ? "Cancelled by you before completion." : "Interrupted by the system."))
        #expect(model.preview == nil && !model.busy && owner.entryAllowed)
        #expect(await owner.close())
    }
}

nonisolated private func mediaDecisionRecord(generation: UInt64, completeCollection: Bool = true, stopped: Bool = false) -> ManualTransferRecord {
    ManualTransferRecord(.init(id: "synthetic-selected-media", configuredEndpoint: "http://127.0.0.1:28711/prefix/",
        targetEndpoint: "http://127.0.0.1:28711/prefix/", direction: .export,
        phase: completeCollection ? .mediaIncomplete : .preview, acceptance: completeCollection ? .exactOk : .notSent,
        boundary: stopped ? .cancelled : nil, local: nil, remoteHasNoCards: nil, remote: nil, servedCounts: nil,
        preparedMs: 1_894_701_600_000, generation: generation, collectionComplete: completeCollection,
        mediaSelected: true, revision: 4,
        media: .init(stage: stopped ? .cancelled : .awaitingConsent,
            counters: .init(uploadedFiles: 0, uploadedDeletions: 0, downloadedFiles: 0, downloadedDeletions: 0,
                checked: 0, bytesDownloaded: 0, bytesUploaded: 0, resynced: false, oversized: [], empty: [], awaiting: 0),
            remoteOutcomeUnknown: stopped, exclusionsVerified: false, failureKind: stopped ? "cancelled" : nil),
        copies: [], nextEligibleMs: nil))
}
private actor SelectedMediaSettingsService: ManualTransferService {
    private(set) var calls: [String] = []
    private(set) var consentAfterAdoption = false
    private(set) var scopeReceived: ManualTransferScope?
    private var record: ManualTransferRecord
    private let activationError: Bool
    private let adopted: @Sendable () async -> Bool
    private let entered: AsyncStream<Void>
    private let enteredContinuation: AsyncStream<Void>.Continuation
    init(generation: UInt64, activationError: Bool, adopted: @escaping @Sendable () async -> Bool) {
        record = mediaDecisionRecord(generation: generation, completeCollection: false)
        self.activationError = activationError; self.adopted = adopted
        (entered, enteredContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func waitForMedia() async { var iterator = entered.makeAsyncIterator(); _ = await iterator.next() }
    func normalizeEndpoint(_ text: String) -> String { text }
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) -> ManualTransferCredentials { .init(username: username, hostKey: "synthetic") }
    func history(operation: ManualTransferOperation) -> [ManualTransferHistory] { calls.append("history"); return record.collectionComplete ? [.init(record: record)] : [] }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.internal("Scope required") }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, scope: ManualTransferScope, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) -> PreparedManualTransfer {
        calls.append("prepare"); scopeReceived = scope; return .init(record: record)
    }
    func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> ManualTransferRecord {
        calls.append("confirm"); record = mediaDecisionRecord(generation: record.generation ?? 0)
        if activationError { throw ManualTransferFailure(.init(issue: .needsDecision, boundary: .activationInterrupted,
            generation: record.generation, candidateActive: true, nextEligibleMs: nil, serverKind: nil)) }
        return record
    }
    func prepareMedia(_ entry: ManualTransferHistory, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation) async -> PreparedManualMedia {
        calls.append("mediaPreview"); consentAfterAdoption = await adopted(); return .init(record: entry.record)
    }
    func runMedia(_ preview: PreparedManualMedia, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation, progress: @escaping @Sendable (ManualMediaSnapshot) -> Void) async -> ManualTransferRecord {
        calls.append("media"); enteredContinuation.yield(()); enteredContinuation.finish()
        var iterator = operation.cancellations.makeAsyncIterator(); _ = await iterator.next()
        calls.append("mediaDrained"); record = mediaDecisionRecord(generation: record.generation ?? 0, stopped: true); return record
    }
    func cancelPreparedMedia(_ preview: PreparedManualMedia, operation: ManualTransferOperation) -> ManualTransferRecord {
        calls.append("cancelPreparedMedia")
        return preview.record
    }
    func invalidateMediaOwner(operation: ManualTransferOperation) { calls.append("invalidate") }
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.internal("Must not recover collection") }
    func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) {}
    func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) {}
    func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to: URL, operation: ManualTransferOperation) {}
}
extension ReviewCollectionStartupTests {
    @Test(arguments: [false, true])
    func selectedMediaWaitsForAdoptionAndExplicitStartAndLogoutDrains(activationError: Bool) async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        let directory = root.appending(path: "Collection")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start(); try #require(owner.state == .ready)
        let service = SelectedMediaSettingsService(generation: owner.generation, activationError: activationError) {
            await MainActor.run { owner.state == .ready && owner.manualAllowed }
        }
        let model = ManualTransferCoordinator(owner: owner, service: service, keychain: SettingsTransferKeys(), defaults: nil)
        await model.loadOffline()
        model.endpointDraft = "http://127.0.0.1:28711/prefix/"; await model.applyEndpoint()
        model.username = "Synthetic User"; model.password = "transient"; await model.login()
        #expect(model.scope == .collectionOnly)
        model.selectScope(.collectionAndMedia); await model.prepare(); await model.confirm()
        #expect(await service.scopeReceived == .collectionAndMedia)
        #expect(await service.consentAfterAdoption)
        #expect(model.mediaPreview != nil && model.result?.nextEligibleMilliseconds == nil)
        #expect(await service.calls.filter { $0 == "media" }.isEmpty)
        await model.refreshOfflineHistory()
        #expect(model.stage == .awaitingMediaConsent && model.mediaPreview != nil)
        #expect(await service.calls.filter { $0 == "media" }.isEmpty)
        let media = Task { await model.startMedia() }
        await service.waitForMedia()
        #expect(owner.manualAllowed && owner.entryAllowed, "media must not hold entry activation")
        await model.cancel() // First user cause survives logout's system drain.
        await model.logout(); await media.value
        let calls = await service.calls
        let drained = try #require(calls.firstIndex(of: "mediaDrained"))
        let invalidated = try #require(calls.lastIndex(of: "invalidate"))
        #expect(drained < invalidated)
        #expect(model.cancellationCause(for: "synthetic-selected-media") == .user)
        #expect(model.result == nil && !model.authenticated && !model.busy)
        #expect(calls.filter { $0 == "confirm" }.count == 1)
        #expect(await owner.close())
    }
}


extension ManualTransferSettingsTests {
    @Test func mediaCompletionAndOldCheckedCountsDoNotHideHistoricalUncertaintyOrUnverifiedExclusions() {
        for verified in [false, true] {
            let record = ManualTransferRecord(.init(id: "synthetic-media-history", configuredEndpoint: "http://127.0.0.1:28711/prefix/",
                targetEndpoint: "http://127.0.0.1:28711/prefix/", direction: .export,
                phase: .complete, acceptance: .exactOk, boundary: nil, local: nil, remoteHasNoCards: nil,
                remote: nil, servedCounts: nil, preparedMs: 1_894_701_600_000, generation: 4,
                collectionComplete: true, mediaSelected: true, revision: 12,
                media: .init(stage: .complete,
                    counters: .init(uploadedFiles: 2, uploadedDeletions: 0, downloadedFiles: 0, downloadedDeletions: 0,
                        checked: 9, bytesDownloaded: 0, bytesUploaded: 12, resynced: false,
                        oversized: ["large.bin"], empty: ["empty.bin"], awaiting: 0),
                    remoteOutcomeUnknown: true, exclusionsVerified: verified, failureKind: nil),
                copies: [], nextEligibleMs: 1_894_705_200_000))
            let values = Dictionary(uniqueKeysWithValues: record.decisionFacts.map { ($0.id, $0.value) })
            #expect(values["media.stage"] == "Supported media complete.")
            #expect(values["media.unknown"] != nil)
            #expect((values["media.counts"] == nil) == verified)
            #expect((values["media.large.0"] == "large.bin") == verified)
            #expect((values["media.empty.0"] == "empty.bin") == verified)
            #expect(values["next"] != nil)
        }
    }
}
