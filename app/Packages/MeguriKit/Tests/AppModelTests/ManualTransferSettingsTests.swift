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
        #expect(await service.calls == ["normalize", "normalize", "login", "normalize", "history"])
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
        #expect(await service.calls == ["normalize", "normalize", "login"])
    }
}
extension ManualTransferSettingsTests {
    @Test func loginEligibilityUsesEnteredFields() {
        let model = ManualTransferCoordinator(owner: nil, service: SettingsTransferService(), defaults: nil)
        model.endpointDraft = "https://example.test/prefix"
        model.password = "transient"
        #expect(!model.canLogin)
        model.username = "   "
        #expect(!model.canLogin)
        model.username = "Synthetic User"
        #expect(model.endpoint.isEmpty && model.canLogin)
        model.endpointDraft = " \n "
        #expect(!model.canLogin)
    }

    @Test func enteredEndpointLoginPersistsNormalizedIdentityAndRestoresOffline() async throws {
        let suite = "meguri.entered-login.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let normalized = "https://example.test/Anki/"
        let draft = "  HTTPS://Example.Test:443/Anki  "
        let service = SettingsTransferService(normalizedEndpoint: normalized)
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: defaults)
        model.endpointDraft = draft
        model.username = "Synthetic User"; model.password = "transient"
        model.select(.export)
        try #require(model.canLogin && model.endpoint.isEmpty)
        await model.login()
        #expect(await service.normalizationInputs == [draft])
        #expect(await service.loginRequests == [.init(endpoint: normalized, username: "Synthetic User", password: "transient")])
        #expect(await service.calls == ["normalize", "login"])
        #expect(await service.invalidations == 1)
        #expect(await keys.clears == 1)
        #expect(await keys.savedEndpoint == normalized)
        #expect(await keys.saved?.username == "Synthetic User")
        #expect(await keys.saved?.hostKey == "synthetic-host-key")
        #expect(model.endpoint == normalized && model.endpointDraft == normalized)
        #expect(model.authenticated && model.password.isEmpty && model.direction == .export && model.preview == nil)
        #expect(defaults.persistentDomain(forName: suite)?.keys.sorted() == ["manualTransfer.endpoint"])
        #expect(defaults.string(forKey: "manualTransfer.endpoint") == normalized)
        let restarted = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: defaults)
        await restarted.loadOffline()
        #expect(restarted.endpoint == normalized && restarted.endpointDraft == normalized)
        #expect(restarted.authenticated && restarted.username == "Synthetic User" && restarted.password.isEmpty)
        #expect(await service.calls == ["normalize", "login", "history"])
    }

    @Test func invalidEnteredEndpointNeverSendsCredentialsAndCanBeCorrected() async {
        let service = SettingsTransferService(rejectNormalization: true)
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil)
        let invalid = "https://example.test/prefix?query=invalid"
        model.endpointDraft = invalid
        model.username = "Synthetic User"; model.password = "transient"
        await model.login()
        #expect(await service.calls == ["normalize"])
        #expect(await service.loginRequests.isEmpty)
        #expect(await service.invalidations == 0)
        #expect(await keys.clears == 0)
        #expect(await keys.saved == nil)
        #expect(model.endpoint.isEmpty && model.endpointDraft == invalid && model.username == "Synthetic User")
        #expect(!model.authenticated && !model.busy && model.password.isEmpty && model.message != nil)
        await service.acceptNormalization()
        model.endpointDraft = "https://example.test/prefix/"; model.password = "corrected"
        #expect(model.canLogin)
        await model.login()
        #expect(model.authenticated && model.password.isEmpty && model.preview == nil)
        #expect(await service.calls == ["normalize", "normalize", "login"])
    }

    @Test(arguments: SettingsLoginPoint.allCases)
    func cancelledLoginCannotPublishLateCredentials(point: SettingsLoginPoint) async throws {
        let suite = "meguri.cancel-login.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let suspension = SettingsLoginSuspension(point)
        let normalized = "https://example.test/prefix/"
        let service = SettingsTransferService(normalizedEndpoint: normalized, suspension: suspension)
        let keys = SettingsTransferKeys(suspension: suspension)
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: defaults)
        model.endpointDraft = "https://example.test/prefix"
        model.username = "Synthetic User"; model.password = "transient"
        let attempt = Task { await model.login(); await suspension.finished() }
        #expect(await suspension.waitUntilPaused())
        #expect(model.busy && !model.canLogin)
        await model.cancel()
        model.endpointDraft = "https://other.example.test/"
        model.username = "Other User"; model.password = "other-transient"
        await model.login(); await model.applyEndpoint(); await model.logout()
        #expect(model.busy)
        await suspension.release(); await attempt.value
        #expect(!model.authenticated && !model.busy && model.password.isEmpty)
        #expect(await keys.saved == nil)
        #expect(await keys.savedEndpoint == nil)
        if point == .normalization {
            #expect(model.endpoint.isEmpty)
            #expect(defaults.string(forKey: "manualTransfer.endpoint") == nil)
            #expect(await service.calls == ["normalize"])
            #expect(await service.invalidations == 0)
        } else {
            // Explicit configuration completed before the credential request;
            // cancellation must never replace it with the later draft edits.
            #expect(model.endpoint == normalized)
            #expect(defaults.string(forKey: "manualTransfer.endpoint") == normalized)
            #expect(await service.loginRequests == [.init(endpoint: normalized, username: "Synthetic User", password: "transient")])
        }
        #expect(model.message == nil)
    }

    @Test func cancelledSavedLoginReportsFailedCleanupAndCanBeReplaced() async throws {
        let suite = "meguri.cancel-login-cleanup.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let suspension = SettingsLoginSuspension(.save)
        let normalized = "https://example.test/prefix/"
        let service = SettingsTransferService(normalizedEndpoint: normalized, suspension: suspension)
        let keys = SettingsTransferKeys(suspension: suspension, failingClearAt: 2)
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: defaults)
        model.endpointDraft = "https://example.test/prefix"
        model.username = "Synthetic User"; model.password = "transient"
        let attempt = Task { await model.login(); await suspension.finished() }
        #expect(await suspension.waitUntilPaused())
        // Save has already changed the store before cancellation is delivered.
        #expect(await keys.saved?.username == "Synthetic User")
        #expect(await keys.savedEndpoint == normalized)
        await model.cancel()
        await suspension.release(); await attempt.value
        #expect(!model.authenticated && !model.busy && model.password.isEmpty && model.preview == nil)
        #expect(await keys.clears == 2)
        #expect(await keys.saved?.username == "Synthetic User")
        #expect(await keys.saved?.hostKey == "synthetic-host-key")
        #expect(await keys.savedEndpoint == normalized)
        #expect(model.message == "Couldn’t remove the cancelled device login. It may return after restart. Unlock this device and retry Log In to replace it.")
        #expect(await service.calls == ["normalize", "login"])

        // Failed deletion really can restore the login, without a network call.
        let restarted = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: defaults)
        await restarted.loadOffline()
        #expect(restarted.authenticated && restarted.username == "Synthetic User" && restarted.password.isEmpty)
        #expect(restarted.endpoint == normalized && restarted.message == nil)
        #expect(await service.calls == ["normalize", "login", "history"])

        // The warning's existing explicit retry path clears and replaces it.
        model.username = "Replacement User"; model.password = "replacement-transient"
        #expect(model.canLogin)
        await model.login()
        #expect(model.authenticated && model.username == "Replacement User" && model.password.isEmpty)
        #expect(model.message == "Logged in. Choose a direction, then prepare its preview.")
        #expect(await keys.clears == 3)
        #expect(await keys.saved?.username == "Replacement User")
        #expect(await keys.savedEndpoint == normalized)
        #expect(await service.calls == ["normalize", "login", "history", "normalize", "login"])
    }

    @Test(arguments: SettingsLoginPoint.allCases)
    func liveLoginKeepsCapturedInputsAndBusyOwnership(point: SettingsLoginPoint) async {
        let suspension = SettingsLoginSuspension(point)
        let normalized = "https://example.test/prefix/"
        let draft = "https://example.test/prefix"
        let service = SettingsTransferService(normalizedEndpoint: normalized, suspension: suspension)
        let keys = SettingsTransferKeys(suspension: suspension)
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil)
        model.endpointDraft = draft
        model.username = "Synthetic User"; model.password = "transient"
        let attempt = Task { await model.login(); await suspension.finished() }
        #expect(await suspension.waitUntilPaused())
        model.endpointDraft = "https://other.example.test/"
        model.username = "Other User"; model.password = "other-transient"
        await model.login(); await model.applyEndpoint(); await model.logout()
        model.select(.export); model.selectScope(.collectionAndMedia)
        #expect(model.busy && !model.canLogin && model.direction == .import && model.scope == .collectionOnly)
        await suspension.release(); await attempt.value
        #expect(model.authenticated && !model.busy && model.password.isEmpty)
        #expect(model.endpoint == normalized && model.username == "Synthetic User")
        #expect(await keys.savedEndpoint == normalized)
        #expect(await keys.saved?.username == "Synthetic User")
        #expect(await service.normalizationInputs == [draft])
        #expect(await service.loginRequests == [.init(endpoint: normalized, username: "Synthetic User", password: "transient")])
        #expect(await service.calls == ["normalize", "login"])
        #expect(await service.invalidations == 1)
    }
}

private actor SettingsTransferKeys: ManualTransferCredentialStore {
    private(set) var saved: ManualTransferCredentials?
    private(set) var savedEndpoint: String?
    private(set) var clears = 0
    private let suspension: SettingsLoginSuspension?
    private let failingClearAt: Int?
    init(suspension: SettingsLoginSuspension? = nil, failingClearAt: Int? = nil) {
        self.suspension = suspension; self.failingClearAt = failingClearAt
    }
    func load(endpoint: String) -> ManualTransferCredentials? { endpoint == savedEndpoint ? saved : nil }
    func save(_ credentials: ManualTransferCredentials, endpoint: String) async {
        saved = credentials; savedEndpoint = endpoint
        await suspension?.pause(at: .save)
    }
    func clear() throws {
        clears += 1
        if clears == failingClearAt { throw BridgeError.unavailable(reason: "Scripted device login deletion failure") }
        saved = nil; savedEndpoint = nil
    }
}

nonisolated enum SettingsLoginPoint: Sendable, CaseIterable { case normalization, login, save }
/// One explicit release per attempt; no polling, clocks or external services.
private actor SettingsLoginSuspension {
    private let point: SettingsLoginPoint
    private let entered: AsyncStream<Void>
    private let enteredContinuation: AsyncStream<Void>.Continuation
    private var releaseContinuation: CheckedContinuation<Void, Never>?
    private var paused = false
    init(_ point: SettingsLoginPoint) {
        self.point = point
        (entered, enteredContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func pause(at point: SettingsLoginPoint) async {
        guard self.point == point, !paused else { return }
        paused = true
        await withCheckedContinuation { continuation in
            releaseContinuation = continuation
            enteredContinuation.yield(())
            enteredContinuation.finish()
        }
    }
    func waitUntilPaused() async -> Bool { var iterator = entered.makeAsyncIterator(); return await iterator.next() != nil }
    func finished() { enteredContinuation.finish() }
    func release() { releaseContinuation?.resume(); releaseContinuation = nil }
}
private nonisolated struct SettingsLoginRequest: Equatable, Sendable {
    let endpoint: String
    let username: String
    let password: String
}
private actor SettingsTransferService: ManualTransferService {
    private(set) var calls: [String] = []
    private(set) var normalizationInputs: [String] = []
    private(set) var loginRequests: [SettingsLoginRequest] = []
    private(set) var invalidations = 0
    private let normalizedEndpoint: String?
    private var rejectNormalization: Bool
    private let recoveryRecord: ManualTransferRecord?
    private let suspension: SettingsLoginSuspension?
    private let historyUnavailable: Bool
    private let suspendPreparation: Bool
    private let entered: AsyncStream<Void>
    private let enteredContinuation: AsyncStream<Void>.Continuation
    init(historyUnavailable: Bool = false, suspendPreparation: Bool = false,
         normalizedEndpoint: String? = nil, rejectNormalization: Bool = false,
         recoveryRecord: ManualTransferRecord? = nil, suspension: SettingsLoginSuspension? = nil) {
        self.normalizedEndpoint = normalizedEndpoint; self.rejectNormalization = rejectNormalization
        self.recoveryRecord = recoveryRecord; self.suspension = suspension
        self.historyUnavailable = historyUnavailable
        self.suspendPreparation = suspendPreparation
        (entered, enteredContinuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    func waitUntilPreparing() async {
        var iterator = entered.makeAsyncIterator()
        _ = await iterator.next()
    }
    func normalizeEndpoint(_ text: String) async throws -> String {
        calls.append("normalize"); normalizationInputs.append(text)
        await suspension?.pause(at: .normalization)
        if rejectNormalization { throw BridgeError.unavailable(reason: "Scripted invalid endpoint") }
        return normalizedEndpoint ?? text
    }
    func acceptNormalization() { rejectNormalization = false }
    func invalidateMediaOwner(operation: ManualTransferOperation) { invalidations += 1 }
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) async -> ManualTransferCredentials {
        calls.append("login"); loginRequests.append(.init(endpoint: endpoint, username: username, password: password))
        await suspension?.pause(at: .login)
        return .init(username: username, hostKey: "synthetic-host-key")
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
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer {
        calls.append("recover")
        if let recoveryRecord { return .init(record: recoveryRecord) }
        throw BridgeError.unavailable(reason: "No scripted recovery")
    }
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
    @Test func recoveryLoginCannotChangeItsBoundEndpoint() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        try #require(owner.state == .noFile)
        let record = decisionRecord()
        let service = SettingsTransferService(recoveryRecord: record)
        let keys = SettingsTransferKeys()
        let model = ManualTransferCoordinator(owner: owner, service: service, keychain: keys, defaults: nil)
        model.endpointDraft = record.configuredEndpoint; await model.applyEndpoint()
        await model.recover(.init(record: record))
        try #require(model.isRecoveryPreview && model.preview?.record.id == record.id)
        model.endpointDraft = "https://other.example.test/"
        model.username = "Synthetic User"; model.password = "transient"
        let invalidations = await service.invalidations
        let clears = await keys.clears
        await model.applyEndpoint(); await model.login()
        #expect(model.endpoint == record.configuredEndpoint)
        #expect(model.preview?.record.configuredEndpoint == record.configuredEndpoint)
        #expect(model.isRecoveryPreview && !model.authenticated && model.password.isEmpty)
        #expect(await service.loginRequests.isEmpty)
        #expect(await service.invalidations == invalidations)
        #expect(await keys.clears == clears)
        // Login at the bound endpoint still works without closing recovery.
        model.endpointDraft = record.configuredEndpoint; model.password = "transient"
        await model.login()
        #expect(model.authenticated && model.canConfirm && model.isRecoveryPreview)
        #expect(model.previewLoginLabel == "Synthetic User")
        #expect(await service.loginRequests == [.init(endpoint: record.configuredEndpoint, username: "Synthetic User", password: "transient")])
        #expect(await service.calls == ["normalize", "recover", "history", "normalize", "normalize", "login"])
        await model.discardPreview()
        #expect(await owner.close())
    }

    @Test func recoveryCannotRebindASuspendedLogin() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        await owner.start()
        try #require(owner.state == .noFile)
        let suspension = SettingsLoginSuspension(.normalization)
        let record = decisionRecord()
        let service = SettingsTransferService(recoveryRecord: record, suspension: suspension)
        let model = ManualTransferCoordinator(owner: owner, service: service, keychain: SettingsTransferKeys(), defaults: nil)
        model.endpointDraft = "https://other.example.test/"
        model.username = "Synthetic User"; model.password = "transient"
        let attempt = Task { await model.login(); await suspension.finished() }
        #expect(await suspension.waitUntilPaused())
        await model.recover(.init(record: record))
        #expect(model.busy && model.preview == nil)
        await suspension.release(); await attempt.value
        #expect(model.authenticated && model.endpoint == "https://other.example.test/")
        #expect(await service.calls == ["normalize", "login"])
        #expect(await owner.close())
    }

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
        #expect(await service.calls == ["history", "normalize", "normalize", "login", "prepare", "history"])
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

extension ManualTransferSettingsTests {
    @Test func connectionTestNormalizesDraftWithoutSavingOrAuthenticating() async {
        let service = SettingsTransferService(normalizedEndpoint: "http://example.test/prefix/")
        let keys = SettingsTransferKeys()
        let probe = SettingsReachabilityProbe()
        let model = ManualTransferCoordinator(owner: nil, service: service, keychain: keys, defaults: nil,
            reachability: { await probe.check($0) })
        let draft = "  HTTP://Example.Test:80/prefix "
        model.endpointDraft = draft
        model.username = "Synthetic User"; model.password = "not-for-the-test"
        let work = Task { await model.testConnection() }
        await probe.waitUntilStarted()
        #expect(model.busy && !model.canTestConnection && !model.canLogin && !model.canPrepare)
        await probe.finish(.response(401)); await work.value
        #expect(await probe.endpoints == ["http://example.test/prefix/"])
        #expect(await service.calls == ["normalize"])
        #expect(await service.normalizationInputs == [draft])
        #expect(await service.invalidations == 0)
        #expect(await keys.saved == nil)
        #expect(await keys.clears == 0)
        #expect(model.endpoint.isEmpty && model.endpointDraft == draft)
        #expect(!model.authenticated && model.password == "not-for-the-test" && model.preview == nil)
        #expect(model.connectionTestResult == EndpointReachabilityResult.response(401).description)
        #expect(!model.busy)
    }

    @Test func rejectedEndpointEligibilityNeverReachesNetworkProbe() async {
        let service = SettingsTransferService(rejectNormalization: true)
        let probe = SettingsReachabilityProbe()
        let model = ManualTransferCoordinator(owner: nil, service: service, defaults: nil,
            reachability: { await probe.check($0) })
        model.endpointDraft = "https://ankiweb.net/"
        await model.testConnection()
        #expect(await service.calls == ["normalize"])
        #expect(await probe.endpoints.isEmpty)
        #expect(model.connectionTestResult?.contains("Address validation failed") == true)
        #expect(model.endpoint.isEmpty && !model.authenticated && !model.busy)
    }

    @Test(arguments: [false, true])
    func staleConnectionCompletionCannotPublishOrChangeAccount(cancel: Bool) async {
        let service = SettingsTransferService()
        let probe = SettingsReachabilityProbe()
        let model = ManualTransferCoordinator(owner: nil, service: service, defaults: nil,
            reachability: { await probe.check($0) })
        model.endpointDraft = "https://example.test/old/"
        let work = Task { await model.testConnection() }
        await probe.waitUntilStarted()
        if cancel { await model.cancel() }
        model.endpointDraft = "https://example.test/new/"
        model.username = "Synthetic User"; model.password = "transient"
        await model.login(); await model.applyEndpoint(); await model.logout(); await model.prepare()
        await probe.finish(.response(200)); await work.value
        #expect(await service.calls == ["normalize"])
        #expect(model.connectionTestResult == nil && model.endpoint.isEmpty && !model.authenticated)
        #expect(!model.busy && model.preview == nil)
        #expect(!model.connectionEvents.contains { $0.detail.contains("HTTP response received") })
    }
}

private actor SettingsReachabilityProbe {
    private(set) var endpoints: [String] = []
    private let starts: AsyncStream<Void>
    private let started: AsyncStream<Void>.Continuation
    private var pending: CheckedContinuation<EndpointReachabilityResult, Never>?
    init() { (starts, started) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1)) }
    func check(_ endpoint: String) async -> EndpointReachabilityResult {
        endpoints.append(endpoint)
        return await withCheckedContinuation { continuation in
            pending = continuation; started.yield(())
        }
    }
    func waitUntilStarted() async { var iterator = starts.makeAsyncIterator(); _ = await iterator.next() }
    func finish(_ result: EndpointReachabilityResult) { pending?.resume(returning: result); pending = nil }
}
