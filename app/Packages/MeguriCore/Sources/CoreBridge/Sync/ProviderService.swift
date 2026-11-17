import Foundation
import CoreFoundation
import Instrumentation
internal import MeguriCoreBindings

public enum SyncProviderSelection: Sendable, Equatable { case localOnly, iCloud, meguri }
public enum SyncProviderPhase: Sendable, Equatable { case localOnly, setupRequired, iCloud, handoffRecovery, blocked }
public struct SyncProviderState: Sendable {
    public let selection: SyncProviderSelection?
    public let phase: SyncProviderPhase
    public let settings: CloudPolicyDeviceSettings?
    public let revision: UInt64
    public let removalPending: Bool
    init(_ value: MeguriCoreBindings.ProviderSnapshot) {
        selection = value.selection.map { switch $0 { case .localOnly: .localOnly; case .iCloud: .iCloud; case .meguri: .meguri } }
        phase = switch value.phase { case .localOnly: .localOnly; case .setupRequired: .setupRequired; case .iCloud: .iCloud; case .handoffRecovery: .handoffRecovery; case .blocked: .blocked }
        settings = value.settings.map { .init(enabled: $0.enabled, paused: $0.paused, automatic: $0.automatic, media: $0.media) }
        revision = value.revision
        removalPending = value.removalPending
    }
    public var permitsICloudStartup: Bool { selection == .iCloud && phase != .blocked && phase != .handoffRecovery && settings?.enabled == true && settings?.paused == false }
    public static let blocked = SyncProviderState(selection: nil, phase: .blocked, settings: nil, revision: 0)
    private init(selection: SyncProviderSelection?, phase: SyncProviderPhase, settings: CloudPolicyDeviceSettings?, revision: UInt64) {
        self.selection = selection; self.phase = phase; self.settings = settings; self.revision = revision; removalPending = false
    }
}
public final class ProviderDiscoveryOwnership: Sendable {
    private let raw: MeguriCoreBindings.ProviderDiscovery
    init(_ raw: MeguriCoreBindings.ProviderDiscovery) { self.raw = raw }
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
        }) else { throw BridgeError.unavailable(reason: "iCloud device settings require recovery") }
        return String(decoding: try JSONSerialization.data(withJSONObject: dictionary), as: UTF8.self)
    }
    public static func legacyMigrationJSON(_ value: Any?) -> String? {
        do { return try strictLegacyJSON(value) } catch { return "null" }
    }
    public static func legacySettings(_ value: Any?) throws -> CloudPolicyDeviceSettings {
        guard let text = try strictLegacyJSON(value) else { return .init(enabled: false, paused: false, automatic: true, media: true) }
        let values = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Bool]
        guard let values else { throw BridgeError.unavailable(reason: "iCloud device settings require recovery") }
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
    public func refresh() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerRefresh) { try raw.refresh() })
    }
    public func saveSettings(_ settings: CloudPolicyDeviceSettings) async throws -> SyncProviderState {
        let raw = raw
        let value = MeguriCoreBindings.ProviderSettings(enabled: settings.enabled, paused: settings.paused, automatic: settings.automatic, media: settings.media)
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerSaveSettings) { try raw.saveSettings(settings: value) })
    }
    public func beginICloudDiscovery() async throws -> ProviderDiscoveryOwnership {
        let raw = raw
        return ProviderDiscoveryOwnership(try await MaintenanceLane.shared.call(.providerBeginDiscovery) { try raw.beginIcloudDiscovery() })
    }
    public func acceptICloudConnection() async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerAcceptICloud) { try raw.acceptIcloudConnection() })
    }
    public func beginHandoff(to destination: SyncProviderSelection, removeCredentials: Bool = false) async throws -> ProviderHandoffOwnership {
        let raw = raw
        let target: MeguriCoreBindings.ProviderSelection = switch destination { case .localOnly: .localOnly; case .iCloud: .iCloud; case .meguri: .meguri }
        return ProviderHandoffOwnership(try await MaintenanceLane.shared.call(.providerBeginHandoff) { try raw.beginHandoff(destination: target, removeCredentials: removeCredentials) })
    }
    /// Call only after the existing original owner has actually joined. There
    /// is deliberately no persisted Boolean or caller-created drain DTO.
    public func finishHandoff(_ handoff: ProviderHandoffOwnership) async throws -> SyncProviderState {
        let raw = raw
        return SyncProviderState(try await MaintenanceLane.shared.call(.providerFinishHandoff) { try raw.finishHandoff(handoff: handoff.raw) })
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
/// Authentication and secure storage only. Retains the real Core connection
/// handle across every suspension; endpoint edits join the original attempt.
public actor MeguriProviderConnectionOwner {
    private let service: any MeguriProviderConnectionService
    private let store: any MeguriProviderCredentialStore
    private var ownership: MeguriProviderConnectionOwnership?
    private var attempt: (revision: UInt64, task: Task<Void, any Error>)?
    private var configuration: (revision: UInt64, task: Task<MeguriProviderConnectionOwnership, any Error>)?
    // Read-only within the module so bounded owner tests can acknowledge entry
    // before releasing work held across the first suspension.
    private(set) var revision: UInt64 = 0
    private var removing = false
    private var configurationBlocked = false
    public private(set) var phase: MeguriProviderConnectionPhase = .unconfigured
    public init(service: any MeguriProviderConnectionService, store: any MeguriProviderCredentialStore = DeviceMeguriProviderCredentialStore()) {
        self.service = service; self.store = store
    }
    private func isCurrent(_ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) -> Bool {
        expected == revision && ownership === owner
    }
    private func requireCurrent(_ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) throws {
        guard isCurrent(expected, owner), !Task.isCancelled else { throw CancellationError() }
    }
    // Draining does not claim an intent. Concurrent callers can join the same
    // original task, but only its own slot can be cleared after the join.
    private func drainAttempt(_ expected: UInt64, _ owner: MeguriProviderConnectionOwnership?) async {
        let original = attempt
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
        let expected = revision, current = ownership
        let originalConfiguration = configuration
        await drainAttempt(expected, current)
        await drainConfiguration(originalConfiguration)
    }
    public func configure(endpoint: String) async throws {
        guard !removing else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        revision &+= 1
        let expected = revision
        var current = ownership
        let originalConfiguration = configuration
        configurationBlocked = true
        phase = .blocked
        do {
            await drainAttempt(expected, current)
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
            phase = .signedOut
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
    public func load() async throws {
        guard !configurationBlocked, !removing else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        revision &+= 1
        let expected = revision
        var current = ownership
        do {
            await drainAttempt(expected, current)
            try requireCurrent(expected, current)
            if current == nil {
                let configured = try await service.configuredMeguri()
                try requireCurrent(expected, current)
                current = configured; ownership = configured
            }
            guard let current else { return }
            let credentials = try await store.load(connection: current.connection)
            try requireCurrent(expected, current)
            phase = credentials == nil ? .signedOut : .setupRequired
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
    public func signIn(username: String, password: String) async throws {
        guard !configurationBlocked, !removing, let ownership, attempt == nil else { throw BridgeError.unavailable(reason: "Configure a Meguri connection first") }
        revision &+= 1
        let expected = revision
        do { _ = try await store.load(connection: ownership.connection) }
        catch { if isCurrent(expected, ownership) { phase = .blocked }; throw error }
        try requireCurrent(expected, ownership)
        phase = .authenticating
        let service = service, store = store
        let task = Task {
            let credentials = try await service.authenticate(ownership, username: username, password: password)
            try requireCurrent(expected, ownership)
            try await store.save(credentials)
        }
        attempt = (expected, task)
        do {
            try await task.value
            try requireCurrent(expected, ownership)
            attempt = nil; phase = .setupRequired
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
        let expected = revision
        var current = ownership
        let originalConfiguration = configuration
        removing = true
        defer { removing = false }
        do {
            await drainAttempt(expected, current)
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
            phase = .signedOut
        } catch { if isCurrent(expected, current) { phase = .blocked }; throw error }
    }
}
