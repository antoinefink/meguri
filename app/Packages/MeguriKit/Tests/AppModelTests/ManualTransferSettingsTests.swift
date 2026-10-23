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
        mediaSelected: false,
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
