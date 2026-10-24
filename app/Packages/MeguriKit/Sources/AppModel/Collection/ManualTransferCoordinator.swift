import CoreBridge
import Foundation
import Observation

/// Settings owns explicit intentions; the core owns all transfer decisions.
/// Constructing this model never starts work. Loading is offline only.
@Observable @MainActor
public final class ManualTransferCoordinator {
    /// Exact core ErrorKind::id allowlist (meguri-sync/src/error.rs), including
    /// feature-gated kinds. Diagnostic data only; never authorizes recovery.
    public enum FailureCategory: String, CaseIterable, Sendable {
        case authFailed = "auth_failed"
        case conflict
        case clientTooOld = "client_too_old"
        case serverError = "server_error"
        case dbCheckRequired = "db_check_required"
        case proxyAuth = "proxy_auth"
        case unexpectedStatus = "unexpected_status"
        case offline
        case network
        case timeout
        case tls
        case redirectRefused = "redirect_refused"
        case badResponse = "bad_response"
        case invalidEndpoint = "invalid_endpoint"
        case invalidInput = "invalid_input"
        case proxyUnusable = "proxy_unusable"
        case endpointRefused = "endpoint_refused"
        case serverMessage = "server_message"
        case clockIncorrect = "clock_incorrect"
        case uploadTooLarge = "upload_too_large"
        case sanityCheckFailed = "sanity_check_failed"
        case notetypeSchemaChanged = "notetype_schema_changed"
        case missingNotetype = "missing_notetype"
        case downloadInvalid = "download_invalid"
        case mediaResyncFailed = "media_resync_failed"
        case diskFull = "disk_full"
        case localStorage = "local_storage"
        case sessionActive = "session_active"
        case exclusiveBusy = "exclusive_busy"
        case cancelled
        case interrupted
        case `internal`
    }
    public enum Stage: Equatable {
        case ready
        case testingConnection
        case authenticating
        case buildingDecision
        case awaitingConfirmation
        case applyingDecision
        case readingRecords
        case inspectingCandidate
        case writingCopy
        case preparingMedia
        case awaitingMediaConsent
        case transferringMedia
    }
    public private(set) var stage: Stage = .ready {
        didSet {
            if stage != oldValue {
                if busy { clearFailureDiagnostics() }
                recordConnectionEvent("Observed stage: " + stage.description)
            }
        }
    }
    public private(set) var endpoint: String
    public var endpointDraft: String {
        didSet { if endpointDraft != oldValue { connectionTestResult = nil; clearFailureDiagnostics() } }
    }
    public private(set) var connectionTestResult: String?
    private let reachability: @Sendable (String) async -> EndpointReachabilityResult
    private var connectionTestTask: Task<EndpointReachabilityResult, Never>?
    public struct ConnectionEvent: Identifiable {
        public let id = UUID()
        public let instant: Date
        public let detail: String
    }
    public private(set) var connectionEvents: [ConnectionEvent] = []
    private func recordConnectionEvent(_ detail: String) {
        connectionEvents.append(.init(instant: owner?.configuration.clock.now() ?? Date(), detail: detail))
        if connectionEvents.count > 30 { connectionEvents.removeFirst(connectionEvents.count - 30) }
    }
    public var username = "" {
        didSet { if username != oldValue { clearFailureDiagnostics() } }
    }
    public var password = ""
    public private(set) var authenticated = false
    public private(set) var direction: ManualTransferDirection = .import
    public private(set) var preview: PreparedManualTransfer?
    public private(set) var mediaPreview: PreparedManualMedia?
    public let scope: ManualTransferScope = .collectionAndMedia
    public private(set) var mediaProgress: ManualMediaSnapshot?
    private var mediaUsername: String?
    public private(set) var records: [ManualTransferHistory] = []
    public private(set) var result: ManualTransferRecord?
    public private(set) var redirectDiagnostic: ManualTransferRedirectDiagnostic?
    public private(set) var failureCategory: FailureCategory?
    public private(set) var failureRequestMethod: ManualTransferRequestMethod?
    public private(set) var message: String? {
        didSet { if let message, message != oldValue { recordConnectionEvent(message) } }
    }
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
    public var busy: Bool { stage != .ready && stage != .awaitingConfirmation && stage != .awaitingMediaConsent }
    public var canLogin: Bool {
        !busy && (preview == nil || recoveryPreview)
            && !endpointDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !password.isEmpty
    }
    public var canPrepare: Bool { !busy && preview == nil && mediaPreview == nil && authenticated && owner?.manualAllowed == true }
    public var canConfirm: Bool { !busy && preview != nil && authenticated && previewUsername == credentials?.username && endpoint == preview?.record.configuredEndpoint && owner?.manualAllowed == true }

    public init(owner: ReviewCollection?, service: any ManualTransferService, keychain: any ManualTransferCredentialStore = DeviceManualTransferCredentialStore(), defaults: UserDefaults? = .standard, reachability: @escaping @Sendable (String) async -> EndpointReachabilityResult = EndpointReachability.check) {
        self.reachability = reachability
        self.owner = owner; self.service = service; self.keychain = keychain; self.defaults = defaults
        let restoredEndpoint = defaults?.string(forKey: "manualTransfer.endpoint") ?? ""
        endpoint = restoredEndpoint
        endpointDraft = restoredEndpoint
    }
    /// Invoked on Settings presentation. Neither history nor Keychain loading
    /// opens a collection, probes a server, starts a transfer or resumes one.
    public func loadOffline() async {
        guard !loaded, !busy else { return }
        loaded = true; stage = .readingRecords
        defer { stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready); finishWaiters() }
        do {
            credentials = try await keychain.load(endpoint: endpoint)
            authenticated = credentials != nil; username = credentials?.username ?? ""
        } catch { message = "Saved server login is unavailable. Local reviews and Collection Files remain available." }
        await refreshHistory()
    }
    public var canTestConnection: Bool {
        !busy && preview == nil && mediaPreview == nil
            && (owner?.navigation.sheet == nil || owner?.navigation.sheet == .manualTransfer)
            && !confirmationPresented && !copyDestinationPresented && owner?.entry.pickerPresented != true
            && owner?.entry.busy != true && owner?.collectionExport.busy != true
            && !endpointDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    public func testConnection() async {
        guard canTestConnection else { return }
        let draft = endpointDraft
        revision &+= 1
        let expected = revision
        stage = .testingConnection; connectionTestResult = nil
        recordConnectionEvent("Connection test started. No credentials are sent.")
        defer { connectionTestTask = nil; stage = .ready; finishWaiters() }
        let normalized: String
        do { normalized = try await service.normalizeEndpoint(draft) }
        catch {
            guard expected == revision, endpointDraft == draft, !Task.isCancelled else { return }
            let detail = "Address validation failed. Use an eligible self-hosted HTTP or HTTPS address without credentials, query or fragment. AnkiWeb cannot be tested here."
            connectionTestResult = detail
            recordConnectionEvent(detail)
            return
        }
        guard expected == revision, endpointDraft == draft, !Task.isCancelled else { return }
        connectionTestResult = "Address validated. Checking for an HTTP response…"
        let task = Task { await reachability(normalized) }
        connectionTestTask = task
        let result = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
        guard expected == revision, endpointDraft == draft, !Task.isCancelled else { return }
        connectionTestResult = result.description
        recordConnectionEvent(result.description)
    }
    public func select(_ direction: ManualTransferDirection) { guard !busy, preview == nil, mediaPreview == nil else { return }; self.direction = direction; result = nil }
    public func applyEndpoint() async {
        if stage == .transferringMedia { await cancelAndDrain() }
        guard !busy, preview == nil else { return }
        revision &+= 1
        stage = .readingRecords
        defer { stage = .ready; finishWaiters() }
        do {
            let normalized = try await service.normalizeEndpoint(endpointDraft)
            if normalized != endpoint {
                try await service.invalidateMediaOwner(operation: ManualTransferOperation())
                mediaPreview = nil; mediaUsername = nil
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
        if stage == .transferringMedia { await cancelAndDrain() }
        guard !busy, preview == nil else { return }
        clearFailureDiagnostics()
        do { try await service.invalidateMediaOwner(operation: ManualTransferOperation()) }
        catch { message = "Couldn’t finish stopping media. Try logging out again."; return }
        mediaPreview = nil; mediaUsername = nil
        credentials = nil; authenticated = false; password = ""; username = ""; result = nil
        revision &+= 1
        stage = .readingRecords
        defer { stage = .ready; finishWaiters() }
        do { try await keychain.clear(); message = nil }
        catch { message = "Couldn’t remove the device login. Unlock this device and try again." }
    }
    public func login() async {
        guard canLogin else { return }
        // Bind this explicit attempt before the first suspension. Draft edits
        // cannot redirect a request or replace the credentials it authenticates.
        let suppliedEndpoint = endpointDraft
        let suppliedUsername = username
        let suppliedPassword = password; password = ""
        revision &+= 1
        stage = .authenticating; message = nil
        let operation = ManualTransferOperation(); self.operation = operation
        let expected = revision
        func current() -> Bool { revision == expected && operation.cancellation == nil && !Task.isCancelled }
        defer { self.operation = nil; stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready); password = ""; finishWaiters() }
        let normalized: String
        do {
            normalized = try await service.normalizeEndpoint(suppliedEndpoint)
        } catch {
            guard current() else { return }
            message = "Couldn’t save this endpoint. Enter an http or https server URL with an optional path prefix, without credentials, a query or a fragment."
            return
        }
        guard current() else { return }
        if let preview {
            guard recoveryPreview, normalized == endpoint, normalized == preview.record.configuredEndpoint else {
                message = "Login must use the endpoint recorded for this recovery preview. Close the preview before changing servers."
                return
            }
        }
        do {
            try await service.invalidateMediaOwner(operation: operation)
            guard current() else { return }
            mediaPreview = nil; mediaUsername = nil
            credentials = nil; authenticated = false
            try await keychain.clear()
            guard current() else { return }
            if normalized != endpoint {
                result = nil; nextEligibleMilliseconds = nil
                endpoint = normalized; defaults?.set(normalized, forKey: "manualTransfer.endpoint")
            }
            endpointDraft = normalized
            let value = try await service.login(endpoint: normalized, username: suppliedUsername, password: suppliedPassword, operation: operation)
            guard current() else { return }
            try await keychain.save(value, endpoint: normalized)
            // Cancellation can arrive during Keychain persistence too. Keep
            // ownership through cleanup and report its failure even when stale.
            guard current() else {
                do { try await keychain.clear() }
                catch {
                    message = "Couldn’t remove the cancelled device login. It may return after restart. Unlock this device and retry Log In to replace it."
                }
                return
            }
            credentials = value; authenticated = true; username = value.username
            if recoveryPreview { previewUsername = value.username }
            message = "Logged in. Choose a direction, then prepare its preview."
            // Deliberately no prepare, meta probe or transfer here.
        } catch {
            guard current() else { return }
            await report(error, operation: operation)
            if endpointDraft != normalized || username != suppliedUsername { clearFailureDiagnostics() }
        }
    }
    public func prepare() async {
        guard canPrepare, let owner, let credentials else { return }
        clearFailureDiagnostics()
        do { try owner.beginManualActivation() } catch { message = "Finish the current review or file operation first."; return }
        revision &+= 1
        defer { finishWaiters() }
        stage = .buildingDecision; message = nil; result = nil
        let operation = ManualTransferOperation(); self.operation = operation
        let expected = revision
        do {
            let prepared = try await service.prepare(endpoint: endpoint, credentials: credentials, direction: direction, scope: scope, operation: operation, progress: receiver(expected))
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
        self.operation = nil; progress = nil; stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready)
        await refreshHistory()
    }
    public func confirm() async {
        guard canConfirm, let owner, let credentials, let prepared = preview else { return }
        clearFailureDiagnostics()
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
            message = value.collectionComplete
                ? (value.mediaSelected ? "Collection complete. Selected media is waiting for a separate decision; the operation is partial." : "Collection transfer finished. Media was excluded and left unchanged.")
                : "Collection transfer needs attention. Retained copies are available below."
        } catch {
            activated = (error as? ManualTransferFailure)?.candidateActive == true
            await report(error, operation: operation)
        }
        // Events may precede result delivery. Adopt the actual activated host,
        // even after cancellation/failure, using the existing Files owner once.
        activated = activated || owner.generation != before
        let adopted = await owner.finishEntryActivation(candidateActive: activated)
        if let cause = operation.cancellation { recordCancellation(cause, id: prepared.record.id) }
        #if !os(macOS)
        owner.navigation.show(.settings)
        #endif
        self.operation = nil; progress = nil; stage = .ready
        await refreshHistory()
        if !activated { await owner.releaseEmptyManualHost() }
        // The collection worker has drained and Files ownership has adopted the
        // activated host (including activated errors) before media admission.
        if adopted, operation.cancellation == nil, let entry = records.first(where: { $0.id == prepared.record.id }), entry.record.canResumeMedia {
            await prepareMedia(entry)
        }
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
        stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready)
        await refreshHistory()
    }
    /// Explicit media-only Resume, also offered after collection adoption.
    /// This offline preview cannot send, replace or upload the collection.
    public func prepareMedia(_ entry: ManualTransferHistory) async {
        guard !busy, preview == nil, entry.record.canResumeMedia, let credentials else { return }
        revision &+= 1; stage = .preparingMedia; message = nil
        let operation = ManualTransferOperation(); self.operation = operation
        defer { self.operation = nil; stage = mediaPreview == nil ? .ready : .awaitingMediaConsent; finishWaiters() }
        do {
            let prepared = try await service.prepareMedia(entry, endpoint: endpoint, credentials: credentials, operation: operation)
            if operation.cancellation == nil { mediaPreview = prepared; mediaUsername = credentials.username }
        } catch { await report(error, operation: operation) }
    }
    public var canRunMedia: Bool {
        !busy && authenticated && mediaPreview != nil && mediaUsername == credentials?.username && endpoint == mediaPreview?.record.configuredEndpoint
    }
    public func startMedia() async {
        guard canRunMedia, let prepared = mediaPreview, let credentials else { return }
        revision &+= 1; let expected = revision
        mediaPreview = nil; stage = .transferringMedia; message = nil; mediaProgress = nil
        let operation = ManualTransferOperation(); self.operation = operation
        // No entry activation/opening ownership spans this separate job. Review
        // stays available while media uses its journal and pending placeholders.
        do {
            let value = try await service.runMedia(prepared, endpoint: endpoint, credentials: credentials, operation: operation) { [weak self] value in
                Task { @MainActor in
                    guard let self, self.revision == expected, self.stage == .transferringMedia else { return }
                    self.mediaProgress = value
                }
            }
            result = value; nextEligibleMilliseconds = value.nextEligibleMilliseconds
            if value.phase == .complete { message = "Collection and supported media complete. Excluded files are listed in history." }
            else { message = "Collection complete. Media stopped before completion; the operation is partial. Saved changes remain. Resume media explicitly from history." }
        } catch { await report(error, operation: operation) }
        if let cause = operation.cancellation { recordCancellation(cause, id: prepared.record.id) }
        await refreshHistory()
        self.operation = nil; stage = .ready; progress = nil; finishWaiters()
    }
    public func cancelPreparedMedia() async {
        guard !busy, let prepared = mediaPreview else { return }
        mediaPreview = nil; mediaUsername = nil
        stage = .readingRecords; message = nil
        defer { stage = .ready; finishWaiters() }
        do {
            let partial = try await service.cancelPreparedMedia(prepared, operation: ManualTransferOperation())
            result = partial
            message = "Collection complete. Selected media remains for later, so this operation is partial. Saved changes remain. Resume media explicitly from history."
        } catch { await report(error, operation: nil) }
        await refreshHistory()
    }
    public func cancelAndDrain() async {
        password = ""
        if stage == .testingConnection { revision &+= 1; connectionTestTask?.cancel() }
        await operation?.cancel(.system)
        if busy { await withCheckedContinuation { waiters.append($0) } }
        // An unstarted category decision has no network effects. Its durable
        // operation remains partial and requires a fresh explicit decision.
        mediaPreview = nil; mediaUsername = nil
        if stage == .awaitingMediaConsent { stage = .ready }
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
    public func cancel() async {
        if stage == .testingConnection {
            revision &+= 1; connectionTestTask?.cancel()
            connectionTestResult = "Connection test cancelled."
            recordConnectionEvent("Connection test cancelled.")
        }
        await operation?.cancel(.user); password = ""
    }
    public func clearPassword() { password = "" }
    public func refreshOfflineHistory() async {
        guard !busy else { return }
        revision &+= 1; stage = .readingRecords; await refreshHistory(); stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready); finishWaiters()
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
        defer { self.operation = nil; stage = preview != nil ? .awaitingConfirmation : (mediaPreview != nil ? .awaitingMediaConsent : .ready); finishWaiters() }
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
    private func clearFailureDiagnostics() {
        redirectDiagnostic = nil
        failureCategory = nil
        failureRequestMethod = nil
    }
    private func report(_ error: any Error, operation: ManualTransferOperation?) async {
        clearFailureDiagnostics()
        if let failure = error as? ManualTransferFailure {
            failureCategory = failure.serverKind.flatMap(FailureCategory.init(rawValue:))
            failureRequestMethod = failure.requestMethod
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
            case .server:
                redirectDiagnostic = failure.redirect
                switch failure.serverKind {
                case "auth_failed": message = "The server rejected the login. Check your username and password, then log in again."
                case "offline": message = "The server could not be reached. Check your network connection and server address."
                case "network": message = "A network problem interrupted the request. Check your connection and server availability."
                case "tls": message = "A secure connection could not be made. Check that the server supports TLS 1.2 or later with a certificate this device trusts."
                case "timeout": message = "The request timed out. Check your connection and server availability."
                case "redirect_refused": message = "The server redirected the request in an unsupported way. Check the server address."
                case "proxy_auth": message = "The proxy requires authentication. Check your proxy login."
                case "proxy_unusable": message = "The system proxy could not be used. Check your proxy settings."
                case "invalid_endpoint": message = "The server address is invalid. Check the address and its path prefix."
                case "conflict": message = "Another device is syncing. Wait for it to finish before preparing a new decision."
                case "server_error": message = "The server reported a problem. Check server availability before preparing a new decision."
                case "unexpected_status": message = "The server returned an unexpected error status. Check the server address and configuration."
                case "bad_response": message = "The server response could not be read. Check the server address and compatibility."
                case "server_message": message = "The server declined the request. Check the server configuration before preparing a new decision."
                case "client_too_old": message = "The server requires a newer client. Update Meguri before preparing a new decision."
                case "clock_incorrect": message = "The device and server clocks differ. Check the date, time and time zone on both."
                case "endpoint_refused": message = "This build permits local test servers. Remote use requires a personal preview build."
                default: message = "The server request failed. Check the endpoint and login, then inspect the recorded stages."
                }
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
    public enum RenderState: Equatable { case empty, configured, error, loading, mediaConsent, mediaProgress, mediaPartial, mediaComplete, mediaUnknown }
    /// Builds synthetic Settings examples. Selected-media effects stay in memory;
    /// no fixture owns a collection or provides live transfer evidence.
    public static func renderFixture(_ state: RenderState) -> ManualTransferCoordinator {
        let mediaState: ManualTransferMediaRenderFixture.State?
        switch state {
        case .mediaConsent: mediaState = .consent
        case .mediaProgress: mediaState = .progress
        case .mediaPartial: mediaState = .partial
        case .mediaComplete: mediaState = .complete
        case .mediaUnknown: mediaState = .unknown
        default: mediaState = nil
        }
        let model = ManualTransferCoordinator(
            owner: nil, service: UnavailableManualTransferService(mediaState: mediaState),
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
        case .mediaConsent, .mediaProgress, .mediaPartial, .mediaComplete, .mediaUnknown:
            guard let mediaState else { return model }
            // Inert synthetic identity satisfies the existing presentation checks.
            // The fixture service never forwards it to a production adapter.
            let identity = ManualTransferCredentials(username: "Fixture study account", hostKey: "render-only-inert")
            model.credentials = identity; model.authenticated = true; model.username = identity.username
            model.direction = .export
            model.records = [ManualTransferMediaRenderFixture.history(mediaState)]
            model.result = model.records.first?.record
            model.message = "Rendering fixture: simulated collection/media state. No server work or acceptance."
            if state == .mediaConsent || state == .mediaProgress {
                model.mediaPreview = ManualTransferMediaRenderFixture.preview(mediaState)
                model.mediaUsername = identity.username; model.stage = .awaitingMediaConsent
            }
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
/// Fixture-only service. All non-media operations remain unavailable. Media
/// actions update synthetic values, never network, Keychain, files or a host.
private actor UnavailableManualTransferService: ManualTransferService {
    private var mediaState: ManualTransferMediaRenderFixture.State?
    init(mediaState: ManualTransferMediaRenderFixture.State? = nil) { self.mediaState = mediaState }
    func prepareMedia(_ entry: ManualTransferHistory, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation) throws -> PreparedManualMedia {
        guard let mediaState else { throw BridgeError.unavailable(reason: "Rendering fixture") }
        return ManualTransferMediaRenderFixture.preview(mediaState)
    }
    func runMedia(_ preview: PreparedManualMedia, endpoint: String, credentials: ManualTransferCredentials, operation: ManualTransferOperation, progress: @escaping @Sendable (ManualMediaSnapshot) -> Void) async throws -> ManualTransferRecord {
        guard mediaState != nil else { throw BridgeError.unavailable(reason: "Rendering fixture") }
        mediaState = .progress
        progress(ManualTransferMediaRenderFixture.progress)
        // Hold this deterministic progress frame until the real Cancel control
        // cancels the coordinator's token. No batches or server effects occur.
        while operation.cancellation == nil { try await Task.sleep(for: .milliseconds(50)) }
        mediaState = .partial
        return ManualTransferMediaRenderFixture.record(.partial)
    }
    func cancelPreparedMedia(_ preview: PreparedManualMedia, operation: ManualTransferOperation) throws -> ManualTransferRecord {
        guard mediaState != nil else { throw BridgeError.unavailable(reason: "Rendering fixture") }
        mediaState = .partial
        return ManualTransferMediaRenderFixture.record(.partial)
    }
    func normalizeEndpoint(_ text: String) throws -> String { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func login(endpoint: String, username: String, password: String, operation: ManualTransferOperation) throws -> ManualTransferCredentials { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func history(operation: ManualTransferOperation) throws -> [ManualTransferHistory] {
        guard let mediaState else { throw BridgeError.unavailable(reason: "Rendering fixture") }
        return [ManualTransferMediaRenderFixture.history(mediaState)]
    }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func prepare(endpoint: String, credentials: ManualTransferCredentials, direction: ManualTransferDirection, scope: ManualTransferScope, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func confirm(endpoint: String, credentials: ManualTransferCredentials, preview: PreparedManualTransfer, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> ManualTransferRecord { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func recover(_ entry: ManualTransferHistory, operation: ManualTransferOperation, progress: @escaping @Sendable (CoreProgress) -> Void) throws -> PreparedManualTransfer { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func cancelPreparation(_ entry: ManualTransferHistory, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func discard(_ preview: PreparedManualTransfer, interrupted: Bool, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
    func saveCopy(_ entry: ManualTransferHistory, index: UInt32, to: URL, operation: ManualTransferOperation) throws { throw BridgeError.unavailable(reason: "Rendering fixture") }
}

public extension ManualTransferCoordinator.Stage {
    var description: String {
        switch self {
        case .ready: "Idle"
        case .testingConnection: "Testing connection"
        case .authenticating: "Logging in"
        case .buildingDecision: "Preparing replacement preview"
        case .awaitingConfirmation: "Awaiting replacement decision"
        case .applyingDecision: "Applying replacement"
        case .readingRecords: "Reading offline history"
        case .inspectingCandidate: "Inspecting local recovery"
        case .writingCopy: "Saving retained copy"
        case .preparingMedia: "Preparing media decision"
        case .awaitingMediaConsent: "Awaiting media decision"
        case .transferringMedia: "Transferring media"
        }
    }
}
