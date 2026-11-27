import Foundation
import Testing
import Instrumentation
import Synchronization
@testable import CoreBridge

@Suite struct MeguriProviderCredentialTests {
    @Test func successfulReadWithMalformedPayloadIsAnError() throws {
        let connection = MeguriProviderConnection(reference: "synthetic-connection", revision: 1, endpoint: "https://synthetic.example/")
        for payload in [["password": "synthetic-never-adopt"], ["endpoint": connection.endpoint, "reference": connection.reference,
            "revision": "1", "username": "synthetic.user", "hostKey": "synthetic-key", "extra": "not-allowed"]] {
            let malformed = try PropertyListSerialization.data(fromPropertyList: payload, format: .binary, options: 0)
            #expect(throws: MeguriProviderCredentialFailure.malformedPayload) {
                try DeviceMeguriProviderCredentialStore.decode(malformed, connection: connection)
            }
        }
    }
    @Test func storedCredentialsCannotBeAdoptedByALaterConnectionRevision() throws {
        let connection = MeguriProviderConnection(reference: "synthetic-connection", revision: 2, endpoint: "https://synthetic.example/")
        let data = try PropertyListSerialization.data(fromPropertyList: ["endpoint": connection.endpoint, "reference": connection.reference,
            "revision": "1", "username": "synthetic.user", "hostKey": "synthetic-key"], format: .binary, options: 0)
        #expect(throws: MeguriProviderCredentialFailure.wrongConnection) {
            try DeviceMeguriProviderCredentialStore.decode(data, connection: connection)
        }
    }
}

private actor FailingProviderStore: MeguriProviderCredentialStore {
    enum Failure: Error { case storage }
    var failure: String
    var value: MeguriProviderCredentials?
    init(_ failure: String) { self.failure = failure }
    func setFailure(_ value: String) { failure = value }
    func load(connection: MeguriProviderConnection) throws -> MeguriProviderCredentials? {
        if failure == "load" { throw Failure.storage }; return value
    }
    func save(_ credentials: MeguriProviderCredentials) throws {
        if failure == "save" { throw Failure.storage }; value = credentials
    }
    func delete(connection: MeguriProviderConnection) throws {
        if failure == "delete" { throw Failure.storage }; value = nil
    }
}
// Substitute the authentication reply only. Connection ownership is still
// issued by the real Core facade; no binding/ready fact is fabricated.
private actor ProviderAuthenticationReply: MeguriProviderConnectionService {
    let core: CoreProviderService
    var authentications = 0
    init(_ core: CoreProviderService) { self.core = core }
    func configureMeguri(endpoint: String) async throws -> MeguriProviderConnectionOwnership { try await core.configureMeguri(endpoint: endpoint) }
    func configuredMeguri() async throws -> MeguriProviderConnectionOwnership? { try await core.configuredMeguri() }
    func authenticate(_ connection: MeguriProviderConnectionOwnership, username: String, password: String) async throws -> MeguriProviderCredentials {
        let actual = try await core.configuredMeguri()
        guard actual?.connection == connection.connection else { throw MeguriProviderAuthenticationFailure.connectionChanged }
        authentications += 1
        return .init(connection: connection.connection, username: "synthetic.user", hostKey: "synthetic-hostkey")
    }
}

extension EventTests {
    @Test @MainActor func credentialStorageFailuresCloseTheProductionConnectionOwner() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-provider-store-\(UUID().uuidString)")
        let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget"),
            manualTransferDirectory: root.appending(path: "installation"))
        try FileManager.default.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let collection = CoreCollectionService()
        do {
            try await collection.open(location, randomSeed: 151)
            let core = try await CoreProviderService.attached()
            let reply = ProviderAuthenticationReply(core)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            try await owner.configure(endpoint: "https://synthetic.example/")
            await store.setFailure("load")
            await #expect(throws: FailingProviderStore.Failure.self) { try await owner.signIn(username: "Synthetic.User", password: "synthetic-password") }
            #expect(await owner.phase == .blocked)
            #expect(await reply.authentications == 0)
            await store.setFailure("save")
            await #expect(throws: FailingProviderStore.Failure.self) { try await owner.signIn(username: "Synthetic.User", password: "synthetic-password") }
            #expect(await owner.phase == .blocked)
            #expect(await reply.authentications == 1)
            await store.setFailure("")
            try await owner.signIn(username: "Synthetic.User", password: "synthetic-password")
            #expect(await owner.phase == .setupRequired)
            await store.setFailure("delete")
            await #expect(throws: FailingProviderStore.Failure.self) { try await owner.deleteCredentialsAfterDrain() }
            #expect(await owner.phase == .blocked)
            #expect(await store.value != nil)
            let state = try await core.state()
            #expect(state.selection == .localOnly && state.phase == .localOnly)
            try await collection.close()
        } catch { try? await collection.close(); throw error }
    }
}

// Hold scheduling before Core or delivery after Core, never connection
// authority: all handles still come from the original Core facade above.
private actor HeldProviderReplies: MeguriProviderConnectionService {
    let core: CoreProviderService
    let reply: ProviderAuthenticationReply
    let authenticationGate: DeferredTestGate?
    var configurationRequests: [String] = []
    var configurations: [String: MeguriProviderConnectionOwnership] = [:]
    var authentications: [MeguriProviderConnection] = []
    var heldConfigurations: [String: (gate: DeferredTestGate, beforeCore: Bool, fails: Bool)] = [:]
    init(_ core: CoreProviderService, authenticationGate: DeferredTestGate? = nil) {
        self.core = core; reply = ProviderAuthenticationReply(core); self.authenticationGate = authenticationGate
    }
    func holdConfiguration(_ endpoint: String, at gate: DeferredTestGate, beforeCore: Bool = false, fails: Bool = false) {
        heldConfigurations[endpoint] = (gate, beforeCore, fails)
    }
    func configureMeguri(endpoint: String) async throws -> MeguriProviderConnectionOwnership {
        configurationRequests.append(endpoint)
        let held = heldConfigurations[endpoint]
        if let held, held.beforeCore { await held.gate.enter() }
        let connection = try await core.configureMeguri(endpoint: endpoint)
        configurations[endpoint] = connection
        if let held {
            if !held.beforeCore { await held.gate.enter() }
            if held.fails { throw FailingProviderStore.Failure.storage }
        }
        return connection
    }
    func configuredMeguri() async throws -> MeguriProviderConnectionOwnership? { try await core.configuredMeguri() }
    func authenticate(_ connection: MeguriProviderConnectionOwnership, username: String, password: String) async throws -> MeguriProviderCredentials {
        authentications.append(connection.connection)
        let credentials = try await reply.authenticate(connection, username: username, password: password)
        if let authenticationGate { await authenticationGate.enter() }
        return credentials
    }
}

private actor HeldProviderReadStore: MeguriProviderCredentialStore {
    var nextRead: (DeferredTestGate, Bool)?
    var value: MeguriProviderCredentials?
    func holdRead(at gate: DeferredTestGate, fails: Bool) { nextRead = (gate, fails) }
    func load(connection: MeguriProviderConnection) async throws -> MeguriProviderCredentials? {
        if let (gate, fails) = nextRead {
            nextRead = nil
            await gate.enter()
            if fails { throw FailingProviderStore.Failure.storage }
            return .init(connection: connection, username: "synthetic.user", hostKey: "synthetic-hostkey")
        }
        return value
    }
    func save(_ credentials: MeguriProviderCredentials) { value = credentials }
    func delete(connection: MeguriProviderConnection) { value = nil }
}

// Use the existing 20-second event deadline, yielding rather than sleeping.
// Revision observation only acknowledges entry; assertions below exercise
// the production owner's phase, admission, storage and real handle dispatch.
@MainActor private func awaitProviderCondition(_ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(20)
    while ContinuousClock.now < deadline {
        if await condition() { return }
        await Task.yield()
    }
    throw BridgeError.unavailable(reason: "Synthetic provider rendezvous did not arrive")
}
@MainActor private func awaitProviderGate(_ gate: DeferredTestGate) async throws {
    try await awaitProviderCondition { await gate.pendingWaiterCount() > 0 }
}
@MainActor private func releaseProviderTasks(_ gates: [DeferredTestGate], _ tasks: [Task<Void, any Error>]) async {
    for task in tasks { task.cancel() }
    for gate in gates { await gate.open() }
    for task in tasks { _ = await task.result }
}
@MainActor private func withProviderOwnerCollection(_ body: (CoreProviderService) async throws -> Void) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-provider-intent-\(UUID().uuidString)")
    let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget"),
        manualTransferDirectory: root.appending(path: "installation"))
    try FileManager.default.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    try await MaintenanceLane.shared.bootstrapCollection(location)
    let collection = CoreCollectionService()
    do {
        try await collection.open(location, randomSeed: 151)
        let core = try await CoreProviderService.attached()
        try await body(core)
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}

extension EventTests {
    @Test @MainActor func endpointEditsJoiningOneHeldAuthenticationKeepLatestIntentClosed() async throws {
        try await withProviderOwnerCollection { core in
            let authentication = DeferredTestGate(), configuration = DeferredTestGate()
            let reply = HeldProviderReplies(core, authenticationGate: authentication)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            let initial = "https://initial.synthetic.example/", older = "https://older.synthetic.example/", latest = "https://latest.synthetic.example/"
            try await owner.configure(endpoint: initial)
            await reply.holdConfiguration(latest, at: configuration)
            var tasks: [Task<Void, any Error>] = []
            do {
                let login = Task { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                tasks.append(login)
                try await awaitProviderGate(authentication)
                let base = await owner.revision
                let first = Task { try await owner.configure(endpoint: older) }
                tasks.append(first)
                try await awaitProviderCondition { await owner.revision == base + 1 }
                let second = Task { try await owner.configure(endpoint: latest) }
                tasks.append(second)
                try await awaitProviderCondition { await owner.revision == base + 2 }
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                #expect(await reply.configurations.count == 1)
                #expect(await reply.authentications.count == 1)
                await authentication.open()
                try await awaitProviderGate(configuration)
                await #expect(throws: CancellationError.self) { try await login.value }
                await #expect(throws: CancellationError.self) { try await first.value }
                #expect(await reply.configurations[older] == nil)
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                #expect(await reply.authentications.count == 1)
                #expect(await store.value == nil)
                await configuration.open()
                try await second.value
                #expect(await owner.phase == .signedOut)
                try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
                let current = try #require(await reply.configurations[latest])
                #expect(await reply.authentications.map(\.endpoint) == [initial, latest])
                #expect(await store.value?.connection == current.connection)
                #expect(await owner.phase == .setupRequired)
            } catch { await releaseProviderTasks([authentication, configuration], tasks); throw error }
        }
    }

    @Test(arguments: [false, true], [false, true])
    @MainActor func configurationDeliveryCannotPublishAnOlderIntent(beforeCore: Bool, olderFails: Bool) async throws {
        try await withProviderOwnerCollection { core in
            let olderGate = DeferredTestGate(), latestGate = DeferredTestGate()
            let reply = HeldProviderReplies(core)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            let initial = "https://initial.synthetic.example/", older = "https://older.synthetic.example/", latest = "https://latest.synthetic.example/"
            try await owner.configure(endpoint: initial)
            await reply.holdConfiguration(older, at: olderGate, beforeCore: beforeCore, fails: olderFails)
            await reply.holdConfiguration(latest, at: latestGate)
            var tasks: [Task<Void, any Error>] = []
            do {
                let first = Task { try await owner.configure(endpoint: older) }
                tasks.append(first)
                try await awaitProviderGate(olderGate)
                let base = await owner.revision
                let second = Task { try await owner.configure(endpoint: latest) }
                tasks.append(second)
                try await awaitProviderCondition { await owner.revision == base + 1 }
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                #expect(await reply.authentications.isEmpty)
                #expect(await reply.configurationRequests == [initial, older])
                #expect(await latestGate.pendingWaiterCount() == 0)
                #expect(try await core.configuredMeguri()?.connection.endpoint == (beforeCore ? initial : older))
                await olderGate.open()
                if olderFails { await #expect(throws: FailingProviderStore.Failure.self) { try await first.value } }
                else { await #expect(throws: CancellationError.self) { try await first.value } }
                try await awaitProviderGate(latestGate)
                #expect(await reply.configurationRequests == [initial, older, latest])
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                #expect(await reply.authentications.isEmpty)
                #expect(await store.value == nil)
                await latestGate.open()
                try await second.value
                #expect(await owner.phase == .signedOut)
                try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
                let current = try #require(await reply.configurations[latest])
                #expect(try await core.configuredMeguri()?.connection == current.connection)
                #expect(current.connection.endpoint == latest)
                #expect(await reply.authentications == [current.connection])
                #expect(await store.value?.connection == current.connection)
                #expect(await owner.phase == .setupRequired)
            } catch { await releaseProviderTasks([olderGate, latestGate], tasks); throw error }
        }
    }

    @Test(arguments: [false, true], [false, true])
    @MainActor func configurationDrainCannotReleaseASuccessorSlot(beforeCore: Bool, removing: Bool) async throws {
        try await withProviderOwnerCollection { core in
            let olderGate = DeferredTestGate(), latestGate = DeferredTestGate()
            let reply = HeldProviderReplies(core)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            let initial = "https://initial.synthetic.example/", older = "https://older.synthetic.example/"
            let superseded = "https://superseded.synthetic.example/", latest = "https://latest.synthetic.example/", final = "https://final.synthetic.example/"
            try await owner.configure(endpoint: initial)
            await reply.holdConfiguration(older, at: olderGate, beforeCore: beforeCore)
            await reply.holdConfiguration(latest, at: latestGate)
            var tasks: [Task<Void, any Error>] = []
            do {
                let first = Task { try await owner.configure(endpoint: older) }
                tasks.append(first)
                try await awaitProviderGate(olderGate)
                let base = await owner.revision
                let second = Task { try await owner.configure(endpoint: superseded) }
                tasks.append(second)
                try await awaitProviderCondition { await owner.revision == base + 1 }
                let draining = Task {
                    if removing { try await owner.deleteCredentialsAfterDrain() }
                    else { await owner.cancelAndDrain() }
                }
                tasks.append(draining)
                try await awaitProviderCondition { await owner.revision == base + 2 }
                #expect(await owner.phase == .blocked)
                #expect(await reply.configurationRequests == [initial, older])
                if removing {
                    await #expect(throws: MeguriProviderAuthenticationFailure.self) { try await owner.configure(endpoint: latest) }
                }
                // Caller cancellation cannot retract the already dispatched Core work.
                first.cancel()
                var successor: Task<Void, any Error>?
                if !removing {
                    let editing = Task { try await owner.configure(endpoint: latest) }
                    tasks.append(editing); successor = editing
                    try await awaitProviderCondition { await owner.revision == base + 3 }
                    #expect(await reply.configurationRequests == [initial, older])
                }
                await olderGate.open()
                await #expect(throws: CancellationError.self) { try await first.value }
                await #expect(throws: CancellationError.self) { try await second.value }
                try await draining.value
                if removing {
                    #expect(await owner.phase == .signedOut)
                    #expect(await store.value == nil)
                    let editing = Task { try await owner.configure(endpoint: latest) }
                    tasks.append(editing); successor = editing
                }
                try await awaitProviderGate(latestGate)
                #expect(await reply.configurationRequests == [initial, older, latest])
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                let nextBase = await owner.revision
                let last = Task { try await owner.configure(endpoint: final) }
                tasks.append(last)
                try await awaitProviderCondition { await owner.revision == nextBase + 1 }
                #expect(await reply.configurationRequests == [initial, older, latest])
                #expect(try await core.configuredMeguri()?.connection.endpoint == latest)
                await latestGate.open()
                let editing = try #require(successor)
                await #expect(throws: CancellationError.self) { try await editing.value }
                try await last.value
                #expect(await owner.phase == .signedOut)
                let current = try #require(await reply.configurations[final])
                #expect(try await core.configuredMeguri()?.connection == current.connection)
                #expect(current.connection.endpoint == final)
                try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
                #expect(await reply.authentications == [current.connection])
                #expect(await store.value?.connection == current.connection)
                #expect(await owner.phase == .setupRequired)
            } catch { await releaseProviderTasks([olderGate, latestGate], tasks); throw error }
        }
    }

    @Test(arguments: [false, true], [false, true])
    @MainActor func credentialReadCannotPublishAfterAnEndpointEdit(fails: Bool, signingIn: Bool) async throws {
        try await withProviderOwnerCollection { core in
            let read = DeferredTestGate(), configuration = DeferredTestGate()
            let reply = HeldProviderReplies(core)
            let store = HeldProviderReadStore()
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            let latest = "https://latest.synthetic.example/"
            try await owner.configure(endpoint: "https://initial.synthetic.example/")
            await store.holdRead(at: read, fails: fails)
            await reply.holdConfiguration(latest, at: configuration)
            var tasks: [Task<Void, any Error>] = []
            do {
                let loading = Task {
                    if signingIn { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                    else { try await owner.load() }
                }
                tasks.append(loading)
                try await awaitProviderGate(read)
                let editing = Task { try await owner.configure(endpoint: latest) }
                tasks.append(editing)
                try await awaitProviderGate(configuration)
                #expect(await owner.phase == .blocked)
                await #expect(throws: BridgeError.self) { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                await configuration.open()
                try await editing.value
                #expect(await owner.phase == .signedOut)
                await read.open()
                if fails { await #expect(throws: FailingProviderStore.Failure.self) { try await loading.value } }
                else { await #expect(throws: CancellationError.self) { try await loading.value } }
                #expect(await owner.phase == .signedOut)
                #expect(await reply.authentications.isEmpty)
                #expect(await store.value == nil)
                try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
                let current = try #require(await reply.configurations[latest])
                #expect(await reply.authentications == [current.connection])
                #expect(await store.value?.connection == current.connection)
                #expect(await owner.phase == .setupRequired)
            } catch { await releaseProviderTasks([read, configuration], tasks); throw error }
        }
    }
}

extension EventTests {
    @Test(arguments: [false, true])
    @MainActor func credentialReadCannotUndoCredentialRemoval(fails: Bool) async throws {
        try await withProviderOwnerCollection { core in
            let read = DeferredTestGate()
            let reply = HeldProviderReplies(core)
            let store = HeldProviderReadStore()
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            try await owner.configure(endpoint: "https://initial.synthetic.example/")
            await store.holdRead(at: read, fails: fails)
            let loading = Task { try await owner.load() }
            do {
                try await awaitProviderGate(read)
                try await owner.deleteCredentialsAfterDrain()
                #expect(await owner.phase == .signedOut)
                await read.open()
                if fails { await #expect(throws: FailingProviderStore.Failure.self) { try await loading.value } }
                else { await #expect(throws: CancellationError.self) { try await loading.value } }
                #expect(await owner.phase == .signedOut)
                #expect(await store.value == nil)
                #expect(await reply.authentications.isEmpty)
            } catch { await releaseProviderTasks([read], [loading]); throw error }
        }
    }
}


extension EventTests {
    #if DEBUG
    @Test @MainActor func ProviderBootstrapAuthenticationAndFactoryNeverTransfer() async throws {
        try await withProviderOwnerCollection { core in
            let reply = ProviderAuthenticationReply(core)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                try await owner.configure(endpoint: "https://bootstrap.synthetic.example/")
                let signedOut = try await owner.bootstrapService()
                try await owner.load()
                await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await signedOut.readiness() }
                let beforeLogin = try await owner.bootstrapService()
                try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
                await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await beforeLogin.prepare(.useDeviceCollection) }
                let afterLogin = try await owner.bootstrapService()
                #expect(afterLogin !== beforeLogin)
                #expect(await owner.phase == .setupRequired)
                #expect(await reply.authentications == 1)
                guard case .notSelected = try await afterLogin.readiness() else { Issue.record("Authentication granted bootstrap readiness"); return }
                let forbidden: Set<BridgeCall> = [.providerBootstrapPrepare, .providerBootstrapConfirm,
                    .providerBootstrapMediaPrepare, .providerBootstrapMedia, .providerBootstrapComplete]
                #expect(calls.withLock { Set($0).isDisjoint(with: forbidden) })
                await owner.cancelAndDrain()
            }
        }
    }
    #endif

    #if DEBUG
    @Test @MainActor func ProviderBootstrapCredentialReadFailureStaysActionable() async throws {
        try await withProviderOwnerCollection { core in
            let reply = ProviderAuthenticationReply(core)
            let store = FailingProviderStore("")
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            try await owner.configure(endpoint: "https://bootstrap.synthetic.example/")
            try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
            let facade = try await owner.bootstrapService()
            await store.setFailure("load")
            let calls = Mutex<[BridgeCall]>([])
            await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                await #expect(throws: FailingProviderStore.Failure.self) { _ = try await facade.prepare(.useServerCollection) }
            }
            #expect(!calls.withLock { $0.contains(.providerBootstrapPrepare) })
            #expect(await store.value != nil)
            await owner.cancelAndDrain()
        }
    }
    #endif

    #if DEBUG
    @Test @MainActor func ProviderBootstrapSignInDrainsHeldReadBeforeAuthenticating() async throws {
        try await withProviderOwnerCollection { core in
            let read = DeferredTestGate()
            let reply = ProviderAuthenticationReply(core)
            let store = HeldProviderReadStore()
            let owner = MeguriProviderConnectionOwner(service: reply, store: store)
            try await owner.configure(endpoint: "https://bootstrap.synthetic.example/")
            try await owner.signIn(username: "synthetic.user", password: "synthetic-password")
            let original = try #require(await store.value)
            let facade = try await owner.bootstrapService()
            await store.holdRead(at: read, fails: false)
            let calls = Mutex<[BridgeCall]>([])
            try await MaintenanceCallObservation.$capture.withValue({ call in calls.withLock { $0.append(call) } }) {
                let preparing = Task { _ = try await facade.prepare(.useServerCollection) }
                var signingIn: Task<Void, any Error>?
                do {
                    try await awaitProviderGate(read)
                    let base = await owner.revision
                    let login = Task { try await owner.signIn(username: "synthetic.user", password: "synthetic-password") }
                    signingIn = login
                    try await awaitProviderCondition { await owner.revision == base + 1 }
                    #expect(await reply.authentications == 1)
                    #expect(await store.value == original)
                    await read.open()
                    await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { try await preparing.value }
                    try await login.value
                    #expect(await reply.authentications == 2)
                    #expect(!calls.withLock { $0.contains(.providerBootstrapPrepare) })
                    await #expect(throws: MeguriBootstrapAdmissionFailure.retired) { _ = try await facade.prepare(.useDeviceCollection) }
                    let current = try await owner.bootstrapService()
                    #expect(current !== facade)
                    await owner.cancelAndDrain()
                } catch {
                    await releaseProviderTasks([read], [preparing] + (signingIn.map { [$0] } ?? []))
                    await owner.cancelAndDrain()
                    throw error
                }
            }
        }
    }
    #endif
}
