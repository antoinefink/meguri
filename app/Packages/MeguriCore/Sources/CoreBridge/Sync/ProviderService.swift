import Foundation
import CoreFoundation
import Instrumentation
internal import MeguriCoreBindings

public enum SyncProviderSelection: Sendable, Equatable { case localOnly, meguri }
public enum SyncProviderPhase: Sendable, Equatable { case localOnly, setupRequired, handoffRecovery, blocked }
public struct SyncProviderState: Sendable {
    public let selection: SyncProviderSelection?
    public let phase: SyncProviderPhase
    public let settings: CloudPolicyDeviceSettings?
    public let revision: UInt64
    public let removalPending: Bool
    init(_ value: MeguriCoreBindings.ProviderSnapshot) {
        selection = value.selection.map { switch $0 { case .localOnly: .localOnly; case .meguri: .meguri } }
        phase = switch value.phase { case .localOnly: .localOnly; case .setupRequired: .setupRequired; case .handoffRecovery: .handoffRecovery; case .blocked: .blocked }
        settings = value.settings.map { .init(enabled: $0.enabled, paused: $0.paused, automatic: $0.automatic, media: $0.media) }
        revision = value.revision
        removalPending = value.removalPending
    }
    public var permitsICloudStartup: Bool { false }
    public static let blocked = SyncProviderState(selection: nil, phase: .blocked, settings: nil, revision: 0)
    /// Rendering fixtures only: a synthetic value that describes no stored selection.
    public static func renderFixture(selection: SyncProviderSelection, phase: SyncProviderPhase) -> SyncProviderState {
        SyncProviderState(selection: selection, phase: phase, settings: nil, revision: 0)
    }
    private init(selection: SyncProviderSelection?, phase: SyncProviderPhase, settings: CloudPolicyDeviceSettings?, revision: UInt64) {
        self.selection = selection; self.phase = phase; self.settings = settings; self.revision = revision; removalPending = false
    }
}
public final class ProviderHandoffOwnership: Sendable {
    fileprivate let raw: MeguriCoreBindings.ProviderHandoff
    init(_ raw: MeguriCoreBindings.ProviderHandoff) { self.raw = raw }
}
public final class MeguriProviderConnectionOwnership: Sendable {
    fileprivate let raw: MeguriCoreBindings.ProviderConnection
    public let connection: MeguriProviderConnection
    init(_ raw: MeguriCoreBindings.ProviderConnection, _ info: MeguriCoreBindings.ProviderConnectionInformation) {
        self.raw = raw; connection = .init(reference: info.reference, revision: info.revision, endpoint: info.endpoint)
    }
}

/// The original Host owns this facade. No public method grants Meguri normal,
/// bootstrap or media authority, or turns on its automatic synchronization.
public protocol MeguriProviderConnectionService: Sendable {
    func configureMeguri(endpoint: String) async throws -> MeguriProviderConnectionOwnership
    func configuredMeguri() async throws -> MeguriProviderConnectionOwnership?
    func authenticate(_ connection: MeguriProviderConnectionOwnership, username: String, password: String) async throws -> MeguriProviderCredentials
}
public actor CoreProviderService: MeguriProviderConnectionService {
    private let raw: MeguriCoreBindings.ProviderOwner
    private init(_ raw: MeguriCoreBindings.ProviderOwner) { self.raw = raw }
    public static func attached() async throws -> CoreProviderService {
        let raw = try await MaintenanceLane.shared.call(.providerOwner) { try providerOwner() }
        return CoreProviderService(raw)
    }
    static func strictLegacyJSON(_ value: Any?) throws -> String? {
        guard let value else { return nil }
        guard let dictionary = value as? [String: Any], ["enabled", "paused", "automatic", "media"].allSatisfy({ key in
            guard let number = dictionary[key] as? NSNumber else { return false }
            return CFGetTypeID(number) == CFBooleanGetTypeID()
        }) else { throw BridgeError.unavailable(reason: "Sync settings could not be read") }
        return String(decoding: try JSONSerialization.data(withJSONObject: dictionary), as: UTF8.self)
    }
    public static func legacyMigrationJSON(_ value: Any?) -> String? {
        do { return try strictLegacyJSON(value) } catch { return "null" }
    }
    public static func legacySettings(_ value: Any?) throws -> CloudPolicyDeviceSettings {
        guard let text = try strictLegacyJSON(value) else { return .init(enabled: false, paused: false, automatic: true, media: true) }
        let values = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Bool]
        guard let values else { throw BridgeError.unavailable(reason: "Sync settings could not be read") }
        return .init(enabled: values["enabled"] == true, paused: values["paused"] == true, automatic: values["automatic"] == true, media: values["media"] == true)
    }
    public static func migrate(directory: URL, legacyJSON: String?) async throws -> SyncProviderState {
        // Invalid legacy input is sent as a present-invalid value. Rust retains
        // its blocked migration durably before the ordinary Host can open.
        let json = legacyJSON
        let root = directory.path
        let result = try await MaintenanceLane.shared.call(.migrateProvider) {
            try migrateProvider(root: root, legacyJson: json)
        }
        return SyncProviderState(result)
    }
    public func state() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerSnapshot) { try raw.snapshot() })
    }
    /// `sync/meguri-provider` MP7 step 5: a note type structure save is
    /// committed on this device and its upload has not run. While true the
    /// status needs you with Upload…, and no round starts.
    public func structureUploadPending() async throws -> Bool {
        let raw = raw
        return try await MaintenanceLane.shared.call(.providerStructureUploadPending) { try raw.structureUploadPending() }
    }
    public func refresh() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerRefresh) { try raw.refresh() })
    }
    public func saveSettings(_ settings: CloudPolicyDeviceSettings) async throws -> SyncProviderState {
        let raw = raw
        let value = MeguriCoreBindings.ProviderSettings(enabled: settings.enabled, paused: settings.paused, automatic: settings.automatic, media: settings.media)
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerSaveSettings) { try raw.saveSettings(settings: value) })
    }
    public func beginHandoff(to destination: SyncProviderSelection, removeCredentials: Bool = false) async throws -> ProviderHandoffOwnership {
        let raw = raw
        let target: MeguriCoreBindings.ProviderSelection = switch destination { case .localOnly: .localOnly; case .meguri: .meguri }
        return ProviderHandoffOwnership(try await MaintenanceLane.shared.call(.providerBeginHandoff) { try raw.beginHandoff(destination: target, removeCredentials: removeCredentials) })
    }
    /// Call only after the existing original owner has actually joined. There
    /// is deliberately no persisted Boolean or caller-created drain DTO.
    public func finishHandoff(_ handoff: ProviderHandoffOwnership) async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerFinishHandoff) { try raw.finishHandoff(handoff: handoff.raw) })
    }
    /// Explicit restart recovery after this process's current native owners
    /// have joined. Core validates current ownership and retains the original
    /// intent; this never asserts that a previous process drained.
    public func recoverHandoffToLocalOnly() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerRecoverHandoffToLocalOnly) { try raw.recoverHandoffToLocalOnly() })
    }
    public func retryFailedPublication() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerRetryFailedPublication) { try raw.retryFailedPublication() })
    }
    public func configureMeguri(endpoint: String) async throws -> MeguriProviderConnectionOwnership {
        let raw = raw
        let connection = try await MaintenanceLane.shared.call(.providerConfigureMeguri) { try raw.configureMeguri(endpoint: endpoint) }
        let info = try await MaintenanceLane.shared.call(.providerConnectionInformation) { try connection.information() }
        return MeguriProviderConnectionOwnership(connection, info)
    }
    public func configuredMeguri() async throws -> MeguriProviderConnectionOwnership? {
        let raw = raw
        guard let connection = try await MaintenanceLane.shared.call(.providerConfiguredMeguri, { try raw.configuredMeguri() }) else { return nil }
        let info = try await MaintenanceLane.shared.call(.providerConnectionInformation) { try connection.information() }
        return MeguriProviderConnectionOwnership(connection, info)
    }
    public func authenticate(_ connection: MeguriProviderConnectionOwnership, username: String, password: String) async throws -> MeguriProviderCredentials {
        let raw = connection.raw
        try await CoreEventStream.shared.start()
        let started = try await MaintenanceLane.shared.call(.providerStartLogin) { try raw.startLogin(username: username, password: password) }
        let job = try await Jobs.trackDraining(started.handle)
        if Task.isCancelled { try await job.cancel() }
        let outcome = await withTaskCancellationHandler { await job.outcome() } onCancel: { Task { try? await job.cancel() } }
        let result = try await MaintenanceLane.shared.call(.providerLoginResult) { try started.result.result() }
        guard !Task.isCancelled else { throw CancellationError() }
        guard let result else {
            if case .failed(let error) = outcome { throw error }
            throw BridgeError.unavailable(reason: "Meguri authentication did not return credentials")
        }
        let credentials: MeguriCoreBindings.ProviderCredentials
        switch result {
        case .authenticated(let value): credentials = value
        case .refused(let failure):
            switch failure {
            case .connectionChanged: throw MeguriProviderAuthenticationFailure.connectionChanged
            case .username: throw MeguriProviderAuthenticationFailure.username
            case .cancelled: throw CancellationError()
            case .credentials: throw MeguriProviderAuthenticationFailure.credentials
            case .redirectRefused: throw MeguriProviderAuthenticationFailure.redirectRefused
            case .network: throw MeguriProviderAuthenticationFailure.network
            }
        }
        let actual = MeguriProviderConnection(reference: credentials.connection.reference, revision: credentials.connection.revision, endpoint: credentials.connection.endpoint)
        guard actual == connection.connection else { throw BridgeError.unavailable(reason: "Meguri connection changed") }
        return .init(connection: actual, username: credentials.username, hostKey: credentials.hostkey)
    }
}

public enum MeguriProviderConnectionPhase: Sendable, Equatable { case unconfigured, signedOut, authenticating, setupRequired, cancelled, blocked }
public enum MeguriProviderAuthenticationFailure: Error, Sendable { case connectionChanged, username, credentials, redirectRefused, network }
/// Owns authentication, secure storage and the bootstrap/round facades.
/// Endpoint edits join every original owner before replacing authority.
public actor MeguriProviderConnectionOwner {
    private let service: any MeguriProviderConnectionService
    private let store: any MeguriProviderCredentialStore
    private var ownership: MeguriProviderConnectionOwnership?
    private var attempt: (revision: UInt64, task: Task<Void, any Error>)?
    private var configuration: (revision: UInt64, task: Task<MeguriProviderConnectionOwnership, any Error>)?
    // Read-only within the module so bounded owner tests can acknowledge entry
    // before releasing work held across the first suspension.
    private(set) var revision: UInt64 = 0
    private var bootstrap: (revision: UInt64, facade: MeguriProviderBootstrapService)?
    private var round: (revision: UInt64, ownership: MeguriProviderConnectionOwnership, facade: CoreMeguriRoundService)?
    private var bootstrapAvailable = false
    private var removing = false
    private var configurationBlocked = false
    public private(set) var phase: MeguriProviderConnectionPhase = .unconfigured
    /// The signed-in account's name, from the stored sign-in; nil while signed out.
    /// Display only: it grants nothing and is never the credential.
    public private(set) var username: String?
    public init(service: any MeguriProviderConnectionService, store: any MeguriProviderCredentialStore = DeviceMeguriProviderCredentialStore()) {
        self.service = service; self.store = store
    }
    private func isCurrent(_ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) -> Bool {
        expected == revision && ownership === owner
    }
    private func requireCurrent(_ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) throws {
        guard isCurrent(expected, owner), !Task.isCancelled else { throw CancellationError() }
    }
    /// Requires an explicit load/configure/sign-in flow. This factory neither
    /// authenticates nor starts a collection/media command.
    public func bootstrapService() throws -> MeguriProviderBootstrapService {
        guard bootstrapAvailable, !configurationBlocked, !removing, attempt == nil, configuration == nil,
              phase == .signedOut || phase == .setupRequired, let ownership else {
            throw MeguriBootstrapAdmissionFailure.connectionUnavailable
        }
        if let bootstrap {
            guard bootstrap.revision == revision else { throw MeguriBootstrapAdmissionFailure.connectionUnavailable }
            return bootstrap.facade
        }
        let expected = revision
        let facade = MeguriProviderBootstrapService(raw: ownership.raw, connection: ownership.connection) { [weak self] in
            guard let self else { throw MeguriBootstrapAdmissionFailure.retired }
            return try await self.bootstrapCredentials(expected, ownership)
        }
        bootstrap = (expected, facade)
        return facade
    }
    private func bootstrapCredentials(_ expected: UInt64, _ exact: MeguriProviderConnectionOwnership) async throws -> MeguriProviderCredentials {
        func validate() throws {
            guard isCurrent(expected, exact), bootstrapAvailable, !configurationBlocked, !removing,
                  attempt == nil, configuration == nil, phase == .signedOut || phase == .setupRequired else {
                throw MeguriBootstrapAdmissionFailure.retired
            }
        }
        try validate()
        let value = try await store.load(connection: exact.connection)
        try validate()
        guard let value else { throw MeguriBootstrapAdmissionFailure.credentialsMissing }
        guard value.connection == exact.connection else { throw MeguriProviderCredentialFailure.wrongConnection }
        return value
    }
    /// A stable connection is enough to construct this facade. Only Core's
    /// actual start admission can prove selection and a completed ready binding.
    public func roundService() throws -> CoreMeguriRoundService {
        guard bootstrapAvailable, !configurationBlocked, !removing, attempt == nil, configuration == nil,
              phase == .signedOut || phase == .setupRequired, let ownership else {
            throw MeguriRoundAdmissionFailure.connectionUnavailable
        }
        if let round {
            guard round.revision == revision, round.ownership === ownership else {
                throw MeguriRoundAdmissionFailure.connectionUnavailable
            }
            return round.facade
        }
        let expected = revision
        let facade = CoreMeguriRoundService(raw: ownership.raw, connection: ownership.connection) { [weak self] in
            guard let self else { throw MeguriRoundAdmissionFailure.retired }
            return try await self.roundCredentials(expected, ownership)
        }
        round = (expected, ownership, facade)
        return facade
    }
    private func roundCredentials(_ expected: UInt64, _ exact: MeguriProviderConnectionOwnership) async throws -> MeguriProviderCredentials {
        func validate() throws {
            guard isCurrent(expected, exact), bootstrapAvailable, !configurationBlocked, !removing,
                  attempt == nil, configuration == nil, phase == .signedOut || phase == .setupRequired else {
                throw MeguriRoundAdmissionFailure.retired
            }
        }
        try validate()
        let value = try await store.load(connection: exact.connection)
        try validate()
        guard let value else { throw MeguriRoundAdmissionFailure.credentialsMissing }
        guard value.connection == exact.connection else { throw MeguriProviderCredentialFailure.wrongConnection }
        return value
    }
    private func drainRound(_ original: (revision: UInt64, ownership: MeguriProviderConnectionOwnership, facade: CoreMeguriRoundService)?) async {
        // Transitions invalidate revision/availability before joining. The
        // credential closure only reads/validates; it never joins this facade.
        // Actor suspension lets its post-read validation reenter and fail,
        // allowing the original round task (and held read) to finish the join.
        await original?.facade.retireAndDrain()
        if let original, round?.facade === original.facade { round = nil }
    }
    private func drainBootstrap(_ original: (revision: UInt64, facade: MeguriProviderBootstrapService)?) async {
        await original?.facade.retireAndDrain()
        if let original, bootstrap?.facade === original.facade { bootstrap = nil }
    }
    // Draining does not claim an intent. Concurrent callers can join the same
    // original task, but only its own slot can be cleared after the join.
    private func drainAttempt(_ original: (revision: UInt64, task: Task<Void, any Error>)?, _ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) async {
        original?.task.cancel()
        _ = try? await original?.task.value
        if let original, attempt?.revision == original.revision { attempt = nil }
        if isCurrent(expected, owner), original != nil, phase == .authenticating { phase = .cancelled }
    }
    // Cancellation cannot retract synchronous FFI effects. Join the actual
    // service result, leaving its caller to report success or typed failure.
    // A concurrent join may already have installed a successor in this slot.
    private func drainConfiguration(_ original: (revision: UInt64, task: Task<MeguriProviderConnectionOwnership, any Error>)?) async {
        _ = await original?.task.result
        if let original, configuration?.revision == original.revision { configuration = nil }
    }
    public func cancelAndDrain() async {
        revision &+= 1
        bootstrapAvailable = false
        let originalBootstrap = bootstrap
        let originalRound = round
        let originalAttempt = attempt
        let expected = revision, current = ownership
        let originalConfiguration = configuration
        await drainRound(originalRound)
        await drainBootstrap(originalBootstrap)
        await drainAttempt(originalAttempt, expected, current)
        await drainConfiguration(originalConfiguration)
    }
    public func configure(endpoint: String) async throws {
        guard !removing else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        revision &+= 1
        bootstrapAvailable = false
        let originalBootstrap = bootstrap
        let originalRound = round
        let originalAttempt = attempt
        let expected = revision
        var current = ownership
        let originalConfiguration = configuration
        configurationBlocked = true
        phase = .blocked
        do {
            await drainRound(originalRound)
            await drainBootstrap(originalBootstrap)
            await drainAttempt(originalAttempt, expected, current)
            await drainConfiguration(originalConfiguration)
            try requireCurrent(expected, current)
            if current == nil {
                let configured = try await service.configuredMeguri()
                try requireCurrent(expected, current)
                current = configured; ownership = configured
            }
            if let current {
                try await store.delete(connection: current.connection)
                try requireCurrent(expected, current)
            }
            let service = service
            let task = Task { try await service.configureMeguri(endpoint: endpoint) }
            configuration = (expected, task)
            let result = await task.result
            if configuration?.revision == expected { configuration = nil }
            let configured = try result.get()
            try requireCurrent(expected, current)
            ownership = configured
            configurationBlocked = false
            username = nil
            phase = .signedOut
            bootstrapAvailable = true
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
    public func load() async throws {
        guard !configurationBlocked, !removing else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        revision &+= 1
        bootstrapAvailable = false
        let originalBootstrap = bootstrap
        let originalRound = round
        let originalAttempt = attempt
        let expected = revision
        var current = ownership
        do {
            await drainRound(originalRound)
            await drainBootstrap(originalBootstrap)
            await drainAttempt(originalAttempt, expected, current)
            try requireCurrent(expected, current)
            if current == nil {
                let configured = try await service.configuredMeguri()
                try requireCurrent(expected, current)
                current = configured; ownership = configured
            }
            guard let current else { return }
            let credentials = try await store.load(connection: current.connection)
            try requireCurrent(expected, current)
            if let credentials, credentials.connection != current.connection { throw MeguriProviderCredentialFailure.wrongConnection }
            username = credentials?.username
            phase = credentials == nil ? .signedOut : .setupRequired
            bootstrapAvailable = true
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
    public func signIn(username: String, password: String) async throws {
        guard !configurationBlocked, !removing, let ownership, attempt == nil else { throw BridgeError.unavailable(reason: "Configure a Meguri connection first") }
        revision &+= 1
        bootstrapAvailable = false
        let originalBootstrap = bootstrap
        let originalRound = round
        let expected = revision
        await drainRound(originalRound)
        await drainBootstrap(originalBootstrap)
        try requireCurrent(expected, ownership)
        do { _ = try await store.load(connection: ownership.connection) }
        catch { if isCurrent(expected, ownership) { phase = .blocked }; throw error }
        try requireCurrent(expected, ownership)
        phase = .authenticating
        let service = service, store = store
        let task = Task {
            let credentials = try await service.authenticate(ownership, username: username, password: password)
            try requireCurrent(expected, ownership)
            try await store.save(credentials)
            return credentials.username
        }
        // Cancelling the attempt must reach the request itself, not only this wrapper.
        attempt = (expected, Task {
            try await withTaskCancellationHandler { _ = try await task.value } onCancel: { task.cancel() }
        })
        do {
            let saved = try await task.value
            try requireCurrent(expected, ownership)
            self.username = saved
            attempt = nil; phase = .setupRequired; bootstrapAvailable = true
        } catch {
            if isCurrent(expected, ownership) { attempt = nil; phase = .blocked }
            throw error
        }
    }
    /// The enclosing collection owner stops and joins both provider owners
    /// before invoking deletion. A failed deletion never publishes signed out.
    public func deleteCredentialsAfterDrain() async throws {
        guard !removing else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        revision &+= 1
        bootstrapAvailable = false
        let originalBootstrap = bootstrap
        let originalRound = round
        let originalAttempt = attempt
        let expected = revision
        var current = ownership
        let originalConfiguration = configuration
        removing = true
        defer { removing = false }
        do {
            await drainRound(originalRound)
            await drainBootstrap(originalBootstrap)
            await drainAttempt(originalAttempt, expected, current)
            await drainConfiguration(originalConfiguration)
            try requireCurrent(expected, current)
            if current == nil {
                let configured = try await service.configuredMeguri()
                try requireCurrent(expected, current)
                current = configured; ownership = configured
            }
            guard let current else { return }
            try await store.delete(connection: current.connection)
            try requireCurrent(expected, current)
            username = nil
            phase = .signedOut
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
}
