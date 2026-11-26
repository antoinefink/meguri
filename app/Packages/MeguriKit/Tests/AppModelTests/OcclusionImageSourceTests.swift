import CoreBridge
import Darwin
import Foundation
import Network
import Testing
import UniformTypeIdentifiers
@testable import AppModel
@testable import Editor

/// A real loopback HTTP response, including a body that can remain in flight.
/// Every URL in these tests is selected by the test through the actual picker.
@MainActor
private final class OcclusionHTTPImage {
    private let listener: NWListener
    private var connections: [NWConnection] = []
    private let body: Data
    private let status: Int
    private let held: Bool
    private var ready: CheckedContinuation<Void, any Error>?
    private let arrivals = AsyncStream<Void>.makeStream()
    private let disconnections = AsyncStream<Void>.makeStream()
    private(set) var url: URL!

    init(body: Data, status: Int = 200, held: Bool = false) throws {
        self.body = body; self.status = status; self.held = held
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: .any)
        listener = try NWListener(using: parameters)
    }
    func start() async throws {
        listener.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in
                guard let self else { return }
                switch state {
                case .ready:
                    self.url = URL(string: "http://127.0.0.1:\(self.listener.port!.rawValue)/selected.svg")!
                    let continuation = self.ready; self.ready = nil; continuation?.resume()
                case .failed(let error):
                    let continuation = self.ready; self.ready = nil; continuation?.resume(throwing: error)
                default: break
                }
            }
        }
        listener.newConnectionHandler = { [weak self] connection in
            Task { @MainActor in
                guard let self else { connection.cancel(); return }
                self.connections.append(connection)
                connection.start(queue: DispatchQueue(label: "Meguri.test.occlusion-image"))
                self.receive(connection, header: Data())
            }
        }
        try await withCheckedThrowingContinuation { continuation in
            ready = continuation
            listener.start(queue: DispatchQueue(label: "Meguri.test.occlusion-image-listener"))
        }
    }
    private func receive(_ connection: NWConnection, header: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 16384) { [weak self] data, _, complete, error in
            Task { @MainActor in
                guard let self else { return }
                var header = header
                if let data { header.append(data) }
                guard header.range(of: Data("\r\n\r\n".utf8)) != nil else {
                    if !complete && error == nil { self.receive(connection, header: header) }
                    return
                }
                self.arrivals.continuation.yield(())
                let count = self.held ? self.body.count + 1000 : self.body.count
                var response = Data("HTTP/1.1 \(self.status) Fixture\r\nContent-Type: image/svg+xml\r\nContent-Length: \(count)\r\nConnection: close\r\n\r\n".utf8)
                response.append(self.body)
                let held = self.held
                connection.send(content: response, completion: .contentProcessed { _ in
                    if !held { connection.cancel() }
                })
                if self.held {
                    let disconnected = self.disconnections.continuation
                    connection.receive(minimumIncompleteLength: 1, maximumLength: 1) { _, _, complete, error in
                        if complete || error != nil { disconnected.yield(()) }
                    }
                }
            }
        }
    }
    func waitForRequest() async throws { try await next(arrivals.stream) }
    func waitForDisconnect() async throws { try await next(disconnections.stream) }
    private func next(_ stream: AsyncStream<Void>) async throws {
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask { for await _ in stream { return }; throw CancellationError() }
            group.addTask { try await Task.sleep(for: .seconds(5)); throw CocoaError(.fileReadUnknown) }
            defer { group.cancelAll() }
            _ = try await group.next()
        }
    }
    func stop() {
        listener.cancel(); connections.forEach { $0.cancel() }
        arrivals.continuation.finish(); disconnections.continuation.finish()
    }
}

extension ReviewCollectionStartupTests {
    @MainActor private func checkOrderedOcclusionDrops(host: FieldEditorHost, root: URL) async throws {
        let session = host.occlusionImage
        let roles = try #require(host.draft.occlusionRoles)
        let png = try OcclusionFieldFlowTests.bitmap()
        let damaged = root.appending(path: "damaged.png")
        let good = root.appending(path: "ordered.png")
        try Data("damaged PNG".utf8).write(to: damaged)
        try png.write(to: good)
        let before = try await CoreMediaService().trackedNames().names
        let untouched = NSItemProvider()
        untouched.registerDataRepresentation(forTypeIdentifier: UTType.url.identifier, visibility: .all) { _ in
            Issue.record("A usable earlier file must prevent loading a later URL provider")
            return nil
        }
        // Unsupported, unreadable and undecodable files must all fall through.
        let urls = [root.appending(path: "not-an-image.pdf"), root.appending(path: "missing.png"), damaged, good]
        #expect(session.chooseProvider(urls.map { NSItemProvider(object: $0 as NSURL) } + [untouched], host: host))
        let deadline = ContinuousClock.now + .seconds(5)
        while session.busy && ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        try #require(!session.busy)
        #expect(session.error == nil && session.image?.name == "ordered.png")
        #expect(session.image?.size == CGSize(width: 4, height: 2))
        #expect(OcclusionImageField.name(in: host.draft.values[roles.image]) == "ordered.png")
        let after = try await CoreMediaService().trackedNames()
        #expect(Set(after.names.map(\.name)).subtracting(before.map(\.name)) == ["ordered.png"])
        #expect(try MediaFolder.read("ordered.png", in: after.root) == png)
    }

    @Test @MainActor func occlusionImageSourcesUseRealFetchAndFenceCancellationAndDraftChanges() async throws {
        let path = try #require(realpath(FileManager.default.temporaryDirectory.path, nil))
        let root = URL(filePath: String(cString: path)).appending(path: UUID().uuidString)
        free(path)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory, backupsDirectory: root.appending(path: "backups"))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(directory: directory, configuration: nil, defaults: nil), providerDirectory: root.appending(path: "transfers"))
        do {
            await owner.start(); await owner.startEmpty()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            _ = try #require(await owner.prepareAddNoteEditor())
            await owner.addNoteFlow.selectNotetype(AddNoteFlow.imageOcclusionChoice)
            let host = try #require(owner.addNoteFlow.runtime as? FieldEditorHost)
            let roles = try #require(host.draft.occlusionRoles)
            let session = host.occlusionImage
            try await checkOrderedOcclusionDrops(host: host, root: root)
            let svg = Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"12\"><rect width=\"24\" height=\"12\" fill=\"red\"/></svg>".utf8)
            let good = try OcclusionHTTPImage(body: svg)
            defer { good.stop() }
            try await good.start()
            // A real dropped URL representation enters the production loader.
            #expect(session.chooseProvider([NSItemProvider(object: good.url as NSURL)], host: host))
            try await good.waitForRequest()
            await session.completion?.value
            #expect(session.image?.size == CGSize(width: 24, height: 12))
            let field = host.draft.values[roles.image]
            let name = try #require(OcclusionImageField.name(in: field))
            let media = try await CoreMediaService().trackedNames()
            #expect(try MediaFolder.read(name, in: media.root) == svg)
            await session.load(host: host) // Existing cards read local media only.

            for (status, body) in [(404, Data()), (200, Data("damaged SVG".utf8))] {
                let bad = try OcclusionHTTPImage(body: body, status: status)
                defer { bad.stop() }
                try await bad.start()
                session.chooseRemoteURL(bad.url, host: host)
                await session.completion?.value
                if status == 404 { #expect(session.fetchFailure?.contains("The server answered with status 404.") == true) }
                else { #expect(session.error != nil && session.fetchFailure == nil) }
                #expect(host.draft.values[roles.image] == field)
                #expect(try await CoreMediaService().trackedNames().names == media.names)
            }
            for changeDraft in [false, true] {
                let slow = try OcclusionHTTPImage(body: Data("<svg".utf8), held: true)
                defer { slow.stop() }
                try await slow.start()
                session.chooseRemoteURL(slow.url, host: host)
                let completion = session.completion
                try await slow.waitForRequest()
                await #expect(throws: (any Error).self) { try await host.flush(hold: true) }
                #expect(session.busy && session.gettingMedia && host.acceptsHostInput)
                if changeDraft { host.setHostField("", at: roles.image) }
                else { session.cancel() }
                await completion?.value
                try await slow.waitForDisconnect()
                #expect(host.draft.values[roles.image] == (changeDraft ? "" : field))
                #expect(try await CoreMediaService().trackedNames().names == media.names)
                #expect(session.fetchFailure == nil)
            }
            host.occlusionPhotoMaximumDimension = 2
            session.chooseBitmap(try OcclusionFieldFlowTests.bitmap(), host: host, photo: true)
            await #expect(throws: (any Error).self) { try await host.flush(hold: true) }
            #expect(session.busy && host.acceptsHostInput)
            await session.completion?.value
            #expect(session.image?.size == CGSize(width: 2, height: 1))
            host.occlusionPhotoMaximumDimension = nil
            session.chooseBitmap(try OcclusionFieldFlowTests.bitmap(), host: host, photo: true)
            await session.completion?.value
            #expect(session.image?.size == CGSize(width: 4, height: 2))
            // A missing installed file is cached for this appearance, then
            // retried when a later explicit reload requests another attempt.
            let laterName = "arrives-later.svg"
            host.setHostField(OcclusionImageField.html(name: laterName), at: roles.image)
            await session.load(host: host)
            #expect(session.image == nil && session.error != nil)
            let failedKey = host.occlusionImageLoadKey
            #expect(try await CoreMediaService().addMedia(name: laterName, bytes: Array(svg)) == laterName)
            await session.load(host: host)
            #expect(session.image == nil && host.occlusionImageLoadKey == failedKey)
            try await host.reload(fields: [roles.image])
            #expect(host.occlusionImageLoadKey != failedKey)
            await session.load(host: host)
            #expect(session.image?.size == CGSize(width: 24, height: 12))
            owner.addNoteFlow.generationEnded(); owner.editNoteFlow.generationEnded()
            #expect(await owner.close())
        } catch {
            owner.addNoteFlow.generationEnded(); owner.editNoteFlow.generationEnded()
            _ = await owner.close(); throw error
        }
    }
}
