import CoreBridge
import Foundation
import Observation

/// Settings owns explicit intentions; the core owns all transfer decisions.
/// Constructing this model never starts work. Loading is offline only.
@Observable @MainActor
public final class ManualTransferCoordinator {
    public enum Stage: Equatable {
        case ready
        case authenticating
        case buildingDecision
        case awaitingConfirmation
        case applyingDecision
        case readingRecords
        case inspectingCandidate
        case writingCopy
    }
    public private(set) var stage: Stage = .ready
    public private(set) var endpoint: String
    public var endpointDraft: String
    public var username = ""
    public var password = ""
    public private(set) var authenticated = false
    public private(set) var direction: ManualTransferDirection = .import
    public private(set) var preview: PreparedManualTransfer?
    public private(set) var records: [ManualTransferHistory] = []
    public private(set) var result: ManualTransferRecord?
    public private(set) var message: String?
    public private(set) var progress: CoreProgress?
    public private(set) var nextEligibleMilliseconds: Int64?
    public private(set) var loaded = false
    public var confirmationPresented = false
    public var copyDestinationPresented = false
    private var selectedCopy: (ManualTransferHistory, Int)?
    public var isRecoveryPreview: Bool { recoveryPreview }
    public var previewLoginLabel: String { previewUsername ?? "Login required" }
    public func chooseCopy(_ entry: ManualTransferHistory, index: Int) {
        guard !busy else { return }
        selectedCopy = (entry, index); copyDestinationPresented = true
    }
    public func copyDestination(_ result: Result<[URL], any Error>) async {
        copyDestinationPresented = false
        guard let selection = selectedCopy else { return }
        selectedCopy = nil
        if case .success(let urls) = result, let folder = urls.first { await saveCopy(selection.0, index: selection.1, folder: folder) }
    }
    private var credentials: ManualTransferCredentials?
    private weak var owner: ReviewCollection?
    private let service: any ManualTransferService
    private let keychain: any ManualTransferCredentialStore
    private let defaults: UserDefaults?
    private var operation: ManualTransferOperation?
    private var revision: UInt64 = 0
    private var previewUsername: String?
    private var recoveryPreview = false
    private var cancellationCauses: [String: ManualTransferCancellation] = [:]
    private var waiters: [CheckedContinuation<Void, Never>] = []
    public var busy: Bool { stage != .ready && stage != .awaitingConfirmation }
    public var canPrepare: Bool { !busy && preview == nil && authenticated && owner?.manualAllowed == true }
    public var canConfirm: Bool { !busy && preview != nil && authenticated && previewUsername == credentials?.username && endpoint == preview?.record.configuredEndpoint && owner?.manualAllowed == true }

    public init(owner: ReviewCollection?, service: any ManualTransferService, keychain: any ManualTransferCredentialStore = DeviceManualTransferCredentialStore(), defaults: UserDefaults? = .standard) {
        self.owner = owner; self.service = service; self.keychain = keychain; self.defaults = defaults
        endpoint = defaults?.string(forKey: "manualTransfer.endpoint") ?? ""
        endpointDraft = endpoint
    }
    /// Invoked on Settings presentation. Neither history nor Keychain loading
    /// opens a collection, probes a server, starts a transfer or resumes one.
    public func loadOffline() async {
        guard !loaded, !busy else { return }
        loaded = true; stage = .readingRecords
        defer { stage = preview == nil ? .ready : .awaitingConfirmation; finishWaiters() }
        do {
            credentials = try await keychain.load(endpoint: endpoint)
            authenticated = credentials != nil; username = credentials?.username ?? ""
        } catch { message = "Saved server login is unavailable. Local reviews and Collection Files remain available." }
        await refreshHistory()
    }
    public func select(_ direction: ManualTransferDirection) { guard !busy, preview == nil else { return }; self.direction = direction; result = nil }
    public func applyEndpoint() async {
        guard !busy, preview == nil else { return }
        revision &+= 1
        stage = .readingRecords
        defer { stage = .ready; finishWaiters() }
        do {
            let normalized = try await service.normalizeEndpoint(endpointDraft)
            if normalized != endpoint {
                // Clear in memory before awaiting persistence. A Keychain failure
                // leaves setup unavailable and never permits reuse of the old key.
                credentials = nil; authenticated = false; password = ""; username = ""; result = nil
                try await keychain.clear()
                nextEligibleMilliseconds = nil
                endpoint = normalized; defaults?.set(normalized, forKey: "manualTransfer.endpoint")
            }
            endpointDraft = normalized; message = nil
        } catch { message = "Couldn’t save this endpoint. Enter an http or https server URL with an optional path prefix, without credentials, a query or a fragment." }
    }
    public func logout() async {
        guard !busy, preview == nil else { return }
        credentials = nil; authenticated = false; password = ""; username = ""; result = nil
        revision &+= 1
        stage = .readingRecords
        defer { stage = .ready; finishWaiters() }
        do { try await keychain.clear(); message = nil }
        catch { message = "Couldn’t remove the device login. Unlock this device and try again." }
    }
    public func login() async {
        guard !busy, (preview == nil || recoveryPreview), !endpoint.isEmpty, !username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !password.isEmpty else { return }
        revision &+= 1
        stage = .authenticating; message = nil
        let suppliedPassword = password; password = ""
        let operation = ManualTransferOperation(); self.operation = operation
        let expected = revision
        defer { self.operation = nil; stage = preview == nil ? .ready : .awaitingConfirmation; password = ""; finishWaiters() }
        do {
            credentials = nil; authenticated = false
            try await keychain.clear()
            let value = try await service.login(endpoint: endpoint, username: username, password: suppliedPassword, operation: operation)
            guard revision == expected, operation.cancellation == nil else { return }
            try await keychain.save(value, endpoint: endpoint)
            credentials = value; authenticated = true; username = value.username
            if recoveryPreview { previewUsername = value.username }
            message = "Logged in. Choose a direction, then prepare its preview."
            // Deliberately no prepare, meta probe or transfer here.
        } catch { await report(error, operation: operation) }
    }
    public func prepare() async {
        guard canPrepare, let owner, let credentials else { return }
        do { try owner.beginManualActivation() } catch { message = "Finish the current review or file operation first."; return }
        revision &+= 1
        defer { finishWaiters() }
        stage = .buildingDecision; message = nil; result = nil
        let operation = ManualTransferOperation(); self.operation = operation
        let expected = revision
        do {
            let prepared = try await service.prepare(endpoint: endpoint, credentials: credentials, direction: direction, operation: operation, progress: receiver(expected))
            if operation.cancellation != nil {
                try await service.discard(prepared, interrupted: operation.cancellation == .system, operation: ManualTransferOperation())
            } else if expected == revision {
                preview = prepared; previewUsername = credentials.username; recoveryPreview = false
            }
        } catch { await report(error, operation: operation) }
        // This worker's durable id is returned even when it produces no preview.
        // Optional history is presentation data, never an ownership mechanism.
        if let id = operation.operationID, let cause = operation.cancellation { recordCancellation(cause, id: id) }
        await owner.finishEntryActivation(candidateActive: false)
        if preview == nil { await owner.releaseEmptyManualHost() }
        self.operation = nil; progress = nil; stage = preview == nil ? .ready : .awaitingConfirmation
        await refreshHistory()
    }
    public func confirm() async {
        guard canConfirm, let owner, let credentials, let prepared = preview else { return }
        do { try owner.beginManualActivation() } catch { message = "Finish the current review or file operation first."; return }
        let before = owner.generation
        revision &+= 1
        defer { finishWaiters() }
        stage = .applyingDecision; preview = nil; message = nil
        let operation = ManualTransferOperation(); self.operation = operation
        var activated = false
        do {
            let value = try await service.confirm(endpoint: endpoint, credentials: credentials, preview: prepared, operation: operation, progress: receiver(revision))
            result = value; activated = value.generation != nil
            message = value.collectionComplete ? "Collection transfer finished. Media was not transferred." : "Collection transfer needs attention. Retained copies are available below."
        } catch {
            activated = (error as? ManualTransferFailure)?.candidateActive == true
            await report(error, operation: operation)
        }
        // Events may precede result delivery. Adopt the actual activated host,
        // even after cancellation/failure, using the existing Files owner once.
        activated = activated || owner.generation != before
        await owner.finishEntryActivation(candidateActive: activated)
        if let cause = operation.cancellation { recordCancellation(cause, id: prepared.record.id) }
        owner.navigation.show(.settings)
        self.operation = nil; progress = nil; stage = .ready
        await refreshHistory()
        if !activated { await owner.releaseEmptyManualHost() }
    }
    public func discardPreview() async {
        guard !busy, let preview else { return }
        stage = .readingRecords
        if recoveryPreview {
            message = "Recovery preview closed. The retained operation still requires a decision; inspect it again in local history."
        } else {
            do { try await service.discard(preview, interrupted: false, operation: ManualTransferOperation()); recordCancellation(.user, id: preview.record.id); message = "Preparation cancelled. Retained copies remain available." }
            catch { await report(error, operation: nil) }
        }
        self.preview = nil; previewUsername = nil; recoveryPreview = false; stage = .ready
        finishWaiters()
        await owner?.releaseEmptyManualHost()
        await refreshHistory()
    }
    public func cancelPreparation(_ entry: ManualTransferHistory) async {
        guard !busy, preview == nil else { return }
        revision &+= 1; stage = .readingRecords
        defer { stage = .ready; finishWaiters() }
        do { try await service.cancelPreparation(entry, operation: ManualTransferOperation()); recordCancellation(.user, id: entry.id); message = "Unfinished preparation cancelled. All retained copies remain available." }
        catch { await report(error, operation: nil) }
        await refreshHistory()
        await owner?.releaseEmptyManualHost()
    }
    public func recover(_ entry: ManualTransferHistory) async {
        guard let collection = owner, !busy, preview == nil, collection.manualAllowed else { return }
        do {
            try collection.beginManualActivation()
        } catch { return }

        revision &+= 1
        let inspection = ManualTransferOperation()
        operation = inspection
        stage = .inspectingCandidate
        message = nil
        defer { finishWaiters() }

        do {
            preview = try await service.recover(entry, operation: inspection, progress: receiver(revision))
            recoveryPreview = true
            previewUsername = credentials?.username
            message = "Check the saved candidate before confirming its use on this device. Recovery does not upload it. Confirmation may require login at the endpoint recorded for this operation."
        } catch {
            await report(error, operation: inspection)
        }

        await collection.finishEntryActivation(candidateActive: false)
        if preview == nil { await collection.releaseEmptyManualHost() }
        operation = nil
        progress = nil
        stage = preview == nil ? .ready : .awaitingConfirmation
        await refreshHistory()
    }
    public func cancelAndDrain() async {
        password = ""
        await operation?.cancel(.system)
        if busy { await withCheckedContinuation { waiters.append($0) } }
        if let preview {
            stage = .readingRecords
            if !recoveryPreview {
                do {
                    try await service.discard(preview, interrupted: true, operation: ManualTransferOperation())
                    recordCancellation(.system, id: preview.record.id)
                } catch { /* The backend retains recovery and acceptance evidence. */ }
            }
            self.preview = nil; previewUsername = nil; recoveryPreview = false; stage = .ready
            await owner?.releaseEmptyManualHost()
            finishWaiters()
        }
    }
    private func finishWaiters() {
        let pending = waiters; waiters.removeAll()
        for waiter in pending { waiter.resume() }
    }
    public func cancel() async { await operation?.cancel(.user); password = "" }
    public func clearPassword() { password = "" }
    public func refreshOfflineHistory() async {
        guard !busy else { return }
        revision &+= 1; stage = .readingRecords; await refreshHistory(); stage = preview == nil ? .ready : .awaitingConfirmation; finishWaiters()
    }
    private func refreshHistory() async {
        do {
            records = try await service.history(operation: ManualTransferOperation())
            if let receipt = records.first(where: { $0.record.id == result?.id }) { result = receipt.record }
        } catch { if message == nil { message = "Local transfer history is unavailable. Reviews and Collection Files remain available." } }
    }
    public func saveCopy(_ entry: ManualTransferHistory, index: Int, folder: URL) async {
        guard !busy, entry.record.copies.indices.contains(index) else { return }
        revision &+= 1
        stage = .writingCopy; message = nil
        let operation = ManualTransferOperation(); self.operation = operation
        defer { self.operation = nil; stage = preview == nil ? .ready : .awaitingConfirmation; finishWaiters() }
        let copy = entry.record.copies[index]
        let target = folder.appendingPathComponent("Meguri-\(entry.id)-\(copy.name)")
        let scoped = folder.startAccessingSecurityScopedResource()
        defer { if scoped { folder.stopAccessingSecurityScopedResource() } }
        do { try await service.saveCopy(entry, index: UInt32(index), to: target, operation: operation); message = "A copy was saved. The protected original is retained." }
        catch { message = "The copy could not be saved, or completion is uncertain. Check the chosen folder before retrying. The protected original is retained." }
    }
    public func cancellationCause(for id: String) -> ManualTransferCancellation? {
        cancellationCauses[id] ?? defaults?.string(forKey: "manualTransfer.cancellationCause.\(id)").flatMap(ManualTransferCancellation.init(rawValue:))
    }
    public func cancellationDescription(for record: ManualTransferRecord) -> String {
        if record.boundary == .interrupted { return "Interrupted by the system." }
        switch cancellationCause(for: record.id) {
        case .user: return "Cancelled by you before completion."
        case .system: return "Interrupted by the system."
        case nil: return "Stopped before completion. The reason was not recorded."
        }
    }
    private func recordCancellation(_ cause: ManualTransferCancellation, id: String) {
        guard cancellationCause(for: id) == nil else { return }
        cancellationCauses[id] = cause
        // Native lifecycle metadata only: backend operation id and first cause.
        // Never changes acceptance, recovery authority, history or cooldown.
        defaults?.set(cause.rawValue, forKey: "manualTransfer.cancellationCause.\(id)")
    }
    private func receiver(_ expected: UInt64) -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in Task { @MainActor in
            guard let self, self.revision == expected, self.busy else { return }
            if self.progress?.cancellable == false && value.cancellable { return }
            self.progress = value
        } }
    }
    private func report(_ error: any Error, operation: ManualTransferOperation?) async {
        if let failure = error as? ManualTransferFailure {
            nextEligibleMilliseconds = failure.nextEligibleMilliseconds
            if failure.serverKind == "auth_failed" {
                credentials = nil; authenticated = false; password = ""
                try? await keychain.clear()
            }
            switch failure.issue {
            case .cooldown: message = "This login is cooling down after a completed transfer. Reviews and Collection Files remain available."
            case .stale, .artifact: message = "The source, endpoint or account changed. Cancel this preview and prepare a new decision."
            case .needsDecision:
                message = failure.boundary == .remoteCompletionUnknown
                    ? "The server may have received the upload. Its completion is unknown. Another server transfer is blocked; no retry or clear is available. Retained copies and local reviews remain available."
                    : "Local activation needs a new recovery decision. Retained copies remain available."
            case .mediaIncomplete: message = "The collection stage finished, but selected media did not finish. This operation is incomplete."
            case .cancelled:
                switch operation?.cancellation {
                case .user: message = "Cancelled by you. Recorded changes and retained copies remain available."
                case .system: message = "Interrupted by the system. Inspect the recorded stages before continuing."
                case nil: message = "The operation stopped before completion. Its cancellation reason was not recorded; inspect the saved stages."
                }
            case .interrupted: message = "Interrupted by the system. Inspect the recorded stages before continuing."
            case .sessionActive: message = "Finish the current review before preparing a server transfer."
            case .safetyBackup: message = "The required safety copy could not be completed. Check storage and prepare again."
            case .diskFull: message = "There isn’t enough free space to safely prepare this transfer."
            case .invalidDownload: message = "The server collection could not be validated. It was not activated."
            case .busy: message = "A transfer or file operation needs attention. Inspect local history below."
            case .server: message = "The server request failed. Check the endpoint and login, then inspect the recorded stages."
            default: message = "The transfer could not finish. Inspect the recorded stages and retained copies below."
            }
        } else if error is ManualTransferInterrupted { message = "The operation was interrupted by the system. Inspect local history before continuing." }
        else if (error as? BridgeError) == .cancelled {
            switch operation?.cancellation {
            case .user: message = "Cancelled by you."
            case .system: message = "Interrupted by the system."
            case nil: message = "The operation stopped. Its cancellation reason was not recorded."
            }
        }
        else { message = "The operation could not finish. Local reviews and Collection Files remain available." }
    }
}

extension ManualTransferCoordinator {
    public enum RenderState { case empty, configured, error, loading }
    /// Builds Settings examples for screenshots with unavailable service and
    /// credential stores. Live transfer evidence must come from a VM run.
    public static func renderFixture(_ state: RenderState) -> ManualTransferCoordinator {
        let model = ManualTransferCoordinator(
            owner: nil, service: UnavailableManualTransferService(),
            keychain: RenderingTransferCredentials(), defaults: nil)
        model.loaded = true
        switch state {
        case .empty:
            return model
        case .error:
            model.message = "Upload completion is unknown. Further server transfers are blocked. Local reviews and Collection Files remain available."
        case .loading:
            model.stage = .buildingDecision
        case .configured:
            model.authenticated = true
            model.username = "Study account"
        }
        model.endpoint = "https://study.example.test/personal/collection/"
        model.endpointDraft = model.endpoint
        return model
    }
}
private actor RenderingTransferCredentials: ManualTransferCredentialStore {
    func load(endpoint: String) -> ManualTransferCredentials? { nil }
    func save(_ credentials: ManualTransferCredentials, endpoint: String) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func clear() {}
}
private actor UnavailableManualTransferService: ManualTransferService {
    func normalizeEndpoint(_ text: String) throws -> String { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) throws -> ManualTransferCredentials { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func history(operation: ManualTransferOperation) throws -> [ManualTransferHistory] { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> ManualTransferRecord { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to: URL, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
}
