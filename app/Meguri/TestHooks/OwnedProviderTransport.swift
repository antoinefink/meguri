#if MEGURI_TEST_HOOKS && os(macOS)
import AppModel
import CoreFoundation
import CryptoKit
import Darwin
import Foundation
import Observation
import SwiftUI
internal import MeguriCoreBindings

extension TestHooks {
    /// Transport admission only. No provider selection, account or sync action.
    @MainActor @Observable
    final class OwnedProviderTransport {
        static let refusal = TestHookError("Owned provider transport refused")
        nonisolated static let runKey = "MEGURI_OWNED_PROVIDER_TRANSPORT_RUN_ID"
        nonisolated static let digestKey = "MEGURI_OWNED_PROVIDER_TRANSPORT_MANIFEST_SHA256"
        nonisolated static let manifestLimit = 128 * 1024
        nonisolated static let binaryLimit = 512 * 1024 * 1024

        nonisolated enum Refusal: Error { case refused }
        nonisolated enum State: Equatable, Sendable { case inert, pending, installing, ready, failed }

        /// No synthetic success transition: only the real installer can enable UI.
        struct Reservation {
            private(set) var state: State
            init(requested: Bool) { state = requested ? .pending : .inert }
            var blocksInteraction: Bool { state != .inert && state != .ready }
            mutating func reserve() -> Bool {
                guard state == .pending else { return false }
                state = .installing
                return true
            }
            mutating func fail() { if state != .inert { state = .failed } }
        }

        nonisolated struct Request: Sendable {
            let runID: String
            let manifestSHA256: String

            static func admit(environment: [String: String], ownedMode: Bool) throws -> Self? {
                let run = environment[OwnedProviderTransport.runKey]
                let digest = environment[OwnedProviderTransport.digestKey]
                if run == nil && digest == nil { return nil }
                guard ownedMode, let run, let digest, validRun(run), validDigest(digest) else {
                    throw OwnedProviderTransport.Refusal.refused
                }
                return Self(runID: run, manifestSHA256: digest)
            }

            static func validRun(_ value: String) -> Bool {
                guard (1...96).contains(value.utf8.count) else { return false }
                let bytes = Array(value.utf8)
                func alphanumeric(_ byte: UInt8) -> Bool {
                    (65...90).contains(byte) || (97...122).contains(byte) || (48...57).contains(byte)
                }
                return alphanumeric(bytes[0])
                    && bytes.allSatisfy { alphanumeric($0) || $0 == 45 || $0 == 95 }
            }

            static func validDigest(_ value: String) -> Bool {
                value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
            }
        }

        nonisolated struct Manifest: Decodable, Sendable {
            let format: Int
            let purpose: String
            let run_id: String
            let origin: String
            let ca_der_base64: String
            let ca_der_sha256: String
            let app_executable_sha256: String
            let app_debug_dylib_sha256: String

            static func parse(_ raw: Data, request: Request) throws -> Self {
                guard !raw.isEmpty, raw.count <= OwnedProviderTransport.manifestLimit,
                      let shape = try JSONSerialization.jsonObject(with: raw) as? [String: Any],
                      Set(shape.keys) == Set(["format", "purpose", "run_id", "origin", "ca_der_base64",
                          "ca_der_sha256", "app_executable_sha256", "app_debug_dylib_sha256"]),
                      let format = shape["format"] as? NSNumber,
                      CFGetTypeID(format) != CFBooleanGetTypeID(),
                      ["i", "q"].contains(String(cString: format.objCType)), format.intValue == 1 else {
                    throw Refusal.refused
                }
                let value = try JSONDecoder().decode(Self.self, from: raw)
                guard value.format == 1, value.purpose == "meguri-owned-provider-transport",
                      value.run_id == request.runID, Request.validRun(value.run_id),
                      validOriginShape(value.origin),
                      (4...87384).contains(value.ca_der_base64.utf8.count),
                      Request.validDigest(value.ca_der_sha256),
                      Request.validDigest(value.app_executable_sha256),
                      Request.validDigest(value.app_debug_dylib_sha256) else { throw Refusal.refused }
                // The out-of-band digest pins EXACT bytes, not a reserialization
                // or a self-reported claim of MG provenance.
                guard OwnedProviderTransport.digest(raw) == request.manifestSHA256 else { throw Refusal.refused }
                _ = try value.caDER()
                return value
            }

            func caDER() throws -> Data {
                guard let der = Data(base64Encoded: ca_der_base64), !der.isEmpty, der.count <= 65536,
                      der.base64EncodedString() == ca_der_base64,
                      OwnedProviderTransport.digest(der) == ca_der_sha256 else { throw Refusal.refused }
                return der
            }

            private static func validOriginShape(_ origin: String) -> Bool {
                // Only obvious syntax is checked here. Host's existing endpoint
                // guard decides local authority; pass its original text unchanged.
                guard origin.utf8.count <= 2048, origin.hasPrefix("https://"),
                      origin.utf8.allSatisfy({ (33...126).contains($0) && $0 != 37 && $0 != 92 }),
                      let url = URLComponents(string: origin), url.scheme == "https",
                      let host = url.host, !host.isEmpty, url.user == nil, url.password == nil,
                      url.query == nil, url.fragment == nil,
                      url.percentEncodedPath.isEmpty || url.percentEncodedPath == "/" else { return false }
                return url.port.map { (1...65535).contains($0) } ?? true
            }
        }

        private var reservation = Reservation(requested: false)
        private var installed = false
        var state: State { installed ? .ready : reservation.state }
        var blocksInteraction: Bool { !installed && reservation.blocksInteraction }
        @ObservationIgnored private var request: Request?
        @ObservationIgnored private weak var collection: ReviewCollection?
        @ObservationIgnored private var expectedGeneration: UInt64?
        @ObservationIgnored private var task: Task<Void, Never>?
        @ObservationIgnored private var observing = false

        func admit(_ request: Request?) {
            guard let request, self.request == nil, state == .inert else { return }
            self.request = request
            reservation = Reservation(requested: true)
        }

        func bind(to collection: ReviewCollection?, owned: OwnedProviderFixture) {
            guard request != nil, state == .pending, !observing else { return }
            guard let collection, collection.configuration.collectionLocation == owned.collectionLocation else {
                reservation.fail()
                return
            }
            self.collection = collection
            observing = true
            observe(collectionFile: owned.collectionLocation.collectionFile.path,
                installationDirectory: owned.installationDirectory.path)
        }

        private func observe(collectionFile: String, installationDirectory: String) {
            guard observing, let collection, state == .pending || state == .installing else { return }
            let snapshot = withObservationTracking {
                (collection.state, collection.lifecycle, collection.generation)
            } onChange: { [weak self] in
                Task { @MainActor [weak self] in
                    self?.observe(collectionFile: collectionFile, installationDirectory: installationDirectory)
                }
            }
            let ready = snapshot.0 == .ready && snapshot.1 == .ready(generation: snapshot.2)
            if state == .installing {
                if !ready || expectedGeneration != snapshot.2 { fail() }
                return
            }
            if !ready {
                switch snapshot.0 {
                case .unopened, .loading: break
                default: fail()
                }
                return
            }
            // Reserve, bind paths/generation and capture ONE original raw owner
            // synchronously at actual readiness, before the task can suspend.
            guard let request, reservation.reserve() else { return }
            let expected = snapshot.2
            expectedGeneration = expected
            do {
                let owner = try MeguriCoreBindings.providerOwner()
                guard let executable = Bundle.main.executableURL else { throw Self.refusal }
                task = Task { [weak self, owner] in
                    do {
                        let input = try await Self.readInput(request: request, executable: executable)
                        try Task.checkCancellation()
                        guard let self, self.isCurrent(expected) else { throw Self.refusal }
                        try await Self.install(owner: owner, collectionFile: collectionFile,
                            installationDirectory: installationDirectory, input: input)
                        try Task.checkCancellation()
                        guard self.isCurrent(expected) else { throw Self.refusal }
                        self.observing = false
                        self.expectedGeneration = nil
                        self.installed = true
                    } catch { self?.fail() }
                }
            } catch { fail() }
        }

        private func isCurrent(_ expected: UInt64) -> Bool {
            guard observing, state == .installing, let collection else { return false }
            return collection.state == .ready && collection.generation == expected
                && collection.lifecycle == .ready(generation: expected)
        }

        private func fail() {
            observing = false
            installed = false
            reservation.fail()
            task?.cancel()
        }

        /// The hook close composition joins before closing/removing owned data.
        func cancelAndJoin() async {
            guard request != nil else { return }
            fail()
            let original = task
            await original?.value
            task = nil
            collection = nil
        }

        nonisolated struct Input: Sendable { let origin: String; let der: Data }

        @concurrent nonisolated private static func install(owner: MeguriCoreBindings.ProviderOwner,
            collectionFile: String, installationDirectory: String, input: Input) async throws {
            try Task.checkCancellation()
            let collectionURL = URL(filePath: collectionFile)
            // Match Host: canonicalize the directory, then retain the collection filename.
            let canonicalCollectionFile = collectionURL.deletingLastPathComponent()
                .resolvingSymlinksInPath()
                .appendingPathComponent(collectionURL.lastPathComponent).path
            try owner.installMeguriFixtureTransportForTesting(collectionPath: canonicalCollectionFile,
                installationDirectory: installationDirectory, origin: input.origin, caDer: input.der)
        }

        @concurrent nonisolated private static func readInput(request: Request, executable: URL) async throws -> Input {
            var checkout = URL(filePath: #filePath)
            for _ in 0..<4 { checkout.deleteLastPathComponent() }
            return try admitFiles(checkout: checkout, executable: executable, request: request)
        }

        /// Internal file-admission seam for synthetic tests; production uses
        /// only #filePath and Bundle.main, never caller/env-selected paths.
        nonisolated static func admitFiles(checkout: URL, executable: URL, request: Request) throws -> Input {
            try Task.checkCancellation()
            guard Request.validRun(request.runID), Request.validDigest(request.manifestSHA256) else { throw Refusal.refused }
            let manifest = try withFile(root: checkout,
                components: ["build", "fixtures", "owned-meguri-provider", request.runID, "transport.json"],
                cap: manifestLimit, privateManifest: true) { fd, size in
                    var raw = Data()
                    try read(fd, size: size) { raw.append($0) }
                    return try Manifest.parse(raw, request: request)
                }
            let directory = executable.deletingLastPathComponent()
            let name = executable.lastPathComponent
            guard executable.isFileURL, !name.isEmpty, name != ".", name != ".." else { throw Refusal.refused }
            let main = try hashFile(root: directory, name: name)
            let debug = try hashFile(root: directory, name: name + ".debug.dylib")
            guard main == manifest.app_executable_sha256, debug == manifest.app_debug_dylib_sha256 else { throw Refusal.refused }
            return Input(origin: manifest.origin, der: try manifest.caDER())
        }

        nonisolated static func digest(_ bytes: Data) -> String {
            SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        }

        nonisolated private static func hashFile(root: URL, name: String) throws -> String {
            try withFile(root: root, components: [name], cap: binaryLimit, privateManifest: false) { fd, size in
                var hash = SHA256()
                try read(fd, size: size) { hash.update(data: $0) }
                return hash.finalize().map { String(format: "%02x", $0) }.joined()
            }
        }

        /// All untrusted components are opened relative to retained descriptors.
        /// NONBLOCK avoids hanging on a substituted FIFO before fstat refuses it.
        nonisolated private static func withFile<T>(root: URL, components: [String], cap: Int,
            privateManifest: Bool, body: (Int32, Int) throws -> T) throws -> T {
            guard root.isFileURL, !components.isEmpty,
                  components.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".."
                      && !$0.contains("/") && !$0.utf8.contains(0) }) else { throw Refusal.refused }
            let flags = O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK
            let rootFD = open(root.path, flags | O_DIRECTORY)
            guard rootFD >= 0 else { throw Refusal.refused }
            var descriptors = [rootFD]
            defer { for fd in descriptors.reversed() { Darwin.close(fd) } }
            var links: [(parent: Int32, name: String, fd: Int32, before: stat)] = []
            for (index, name) in components.enumerated() {
                try Task.checkCancellation()
                let parent = descriptors[descriptors.count - 1]
                let fd = openat(parent, name, flags | (index == components.count - 1 ? 0 : O_DIRECTORY))
                guard fd >= 0 else { throw Refusal.refused }
                descriptors.append(fd)
                let before = try status(fd)
                try refuseAlias(fd)
                links.append((parent, name, fd, before))
            }
            guard let file = links.last else { throw Refusal.refused }
            try validateFile(file.before, cap: cap, privateManifest: privateManifest)
            let result = try body(file.fd, Int(file.before.st_size))
            for link in links {
                try Task.checkCancellation()
                try requireStable(link.before, status(link.fd))
                var named = stat()
                guard fstatat(link.parent, link.name, &named, AT_SYMLINK_NOFOLLOW) == 0 else { throw Refusal.refused }
                try requireStable(link.before, named)
                try refuseAlias(link.fd)
            }
            return result
        }

        nonisolated static func validateFile(_ value: stat, cap: Int, privateManifest: Bool) throws {
            guard value.st_mode & S_IFMT == S_IFREG, value.st_size > 0, value.st_size <= off_t(cap),
                  !privateManifest || (value.st_uid == geteuid() && value.st_mode & 0o7777 == 0o600) else { throw Refusal.refused }
        }

        nonisolated private static func status(_ fd: Int32) throws -> stat {
            var value = stat()
            guard fstat(fd, &value) == 0 else { throw Refusal.refused }
            return value
        }

        nonisolated static func requireStable(_ before: stat, _ after: stat) throws {
            guard before.st_dev == after.st_dev, before.st_ino == after.st_ino,
                  before.st_mode == after.st_mode, before.st_uid == after.st_uid,
                  before.st_gid == after.st_gid, before.st_size == after.st_size,
                  before.st_nlink == after.st_nlink, before.st_flags == after.st_flags,
                  before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
                  before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
                  before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
                  before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec else { throw Refusal.refused }
        }

        nonisolated private static func refuseAlias(_ fd: Int32) throws {
            // Finder alias bit (big-endian Finder flags) and alias file type,
            // read from this descriptor, never via a path-based URL reopen.
            var finder = [UInt8](repeating: 0, count: 32)
            let count = finder.withUnsafeMutableBytes {
                fgetxattr(fd, "com.apple.FinderInfo", $0.baseAddress, $0.count, 0, 0)
            }
            if count < 0 { guard errno == ENOATTR else { throw Refusal.refused }; return }
            guard count == 32, finder[8] & 0x80 == 0,
                  Array(finder.prefix(4)) != Array("alis".utf8) else { throw Refusal.refused }
        }

        nonisolated private static func read(_ fd: Int32, size: Int, consume: (Data) -> Void) throws {
            var buffer = [UInt8](repeating: 0, count: 64 * 1024)
            var total = 0
            var interruptions = 0
            while true {
                try Task.checkCancellation()
                let amount = min(buffer.count, size - total + 1)
                let count = buffer.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, amount) }
                if count < 0 {
                    guard errno == EINTR, interruptions < 16 else { throw Refusal.refused }
                    interruptions += 1
                    continue
                }
                if count == 0 { guard total == size else { throw Refusal.refused }; return }
                guard count <= size - total else { throw Refusal.refused }
                consume(Data(buffer.prefix(count)))
                total += count
            }
        }
    }

    /// The ordinary subtree stays mounted, including its startup/open tasks.
    struct OwnedProviderTransportGate: ViewModifier {
        let transport: OwnedProviderTransport
        func body(content: Content) -> some View {
            if transport.state == .inert {
                content
            } else {
                VStack {
                    content
                        .disabled(transport.blocksInteraction)
                        .accessibilityHidden(transport.blocksInteraction)
                    switch transport.state {
                    case .ready:
                        Text("Transport installed")
                            .accessibilityIdentifier("testhooks.ownedProvider.transport.ready")
                    case .failed:
                        Text("Transport refused")
                            .accessibilityIdentifier("testhooks.ownedProvider.transport.failed")
                    default:
                        Text("Transport pending")
                            .accessibilityIdentifier("testhooks.ownedProvider.transport.pending")
                    }
                }
            }
        }
    }
}
#endif
