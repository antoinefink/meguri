import Darwin
import Foundation
import Testing

// Generated observations below are deliberately fake-only. No Bundle/main,
// simulator, SDK, signing, account or network execution is used by this suite.
@MainActor @Suite(.serialized) struct AgentRunningProductTests {
    private typealias Observation = AgentRunningProductObservation
    private typealias Copy = AgentProductReceiptBinding.Copy
    private static let cpu: UInt32 = 0x0100000c
    private static let commit = String(repeating: "a", count: 40)
    private static let hostRoot = "/generated-checkout"
    private static let buildRun = "20320405T060708Z-2", installRun = "20320405T060708Z-3"
    private static let app = "Meguri.app", executable = "Contents/MacOS/Meguri"
    private static let agent = app + "/Contents/PlugIns/MeguriSnapshotTests.xctest"
    private static let agentExecutable = "Contents/MacOS/MeguriSnapshotTests"
    private static let debug = "Meguri.app/Contents/MacOS/Meguri.debug.dylib"
    private static let widgetDebugs = ["MeguriWidget.appex/Contents/MacOS/MeguriWidget.debug.dylib",
                                     "Meguri.app/Contents/PlugIns/MeguriWidget.appex/Contents/MacOS/MeguriWidget.debug.dylib"]

    private static func word(_ value: UInt32, big: Bool = false) -> Data {
        Data((0..<4).map { UInt8(truncatingIfNeeded: value >> UInt32((big ? 3 - $0 : $0) * 8)) })
    }
    private static func image(_ byte: UInt8, subtype: UInt32 = 0) -> Data {
        let words: [UInt32] = [0xfeedfacf, cpu, subtype, 2, 1, 24, 0, 0, 0x1b, 24]
        return words.reduce(Data()) { $0 + word($1) } + Data(repeating: byte, count: 16)
    }
    private static func fat(_ thin: Data, subtype: UInt32 = 0) -> Data {
        let words: [UInt32] = [0xcafebabe, 1, cpu, subtype, 32, UInt32(thin.count), 3]
        return words.reduce(Data()) { $0 + word($1, big: true) } + Data(repeating: 0, count: 4) + thin
    }
    private static func json(_ value: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    }
    private static func ref(_ path: String, _ bytes: Data) -> [String: Any] {
        ["path": path, "sha256": AgentJSON.hash(bytes), "bytes": bytes.count]
    }
    private static func artifact(_ path: String, _ kind: String = "json") -> [String: Any] { ["path": path, "kind": kind] }
    private static func receipt(_ job: String, run: String, kind: String, artifacts: [[String: Any]],
                                key: String, reference: [String: Any]) throws -> Data {
        try json(["schema_version": 1, "job": job, "run_id": run, "status": "pass", "exit_code": 0,
            "placement": "host", "service": NSNull(), "dry_run": false,
            "args": job == "build-app" ? ["--platform", kind == "mac" ? "mac" : "ios-sim", "--test-plan", "CloudAgent"] : [],
            "root": hostRoot, "started_at": "2032-04-05T06:07:08Z", "finished_at": "2032-04-05T06:08:08Z",
            "duration_ms": 60000, "commit": ["sha": commit, "dirty": false], "message": "generated unit fixture",
            "summary": ["total": 1, "passed": 1, "failed": 0, "skipped": 0], "failures": [], "user_steps": [],
            "artifacts": artifacts, "details": [key: reference]])
    }

    @MainActor private final class Fixture {
        let directory: URL, product: URL, mainURL: URL, physicalMainURL: URL, agentURL: URL, debugURL: URL
        let originalHeader: Data, receipts: AgentProductReceiptBinding, observed: Observation
        let mainBytes: Data, agentBytes: Data, debugBytes: Data
        init(kind: String = "mac", internalLink: Bool = false, widgets: Bool = false) throws {
            guard let temp = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(temp) }
            let directory = URL(fileURLWithPath: String(cString: temp), isDirectory: true)
                .appendingPathComponent("meguri-running-product-" + UUID().uuidString, isDirectory: true)
            self.directory = directory
            let product = directory.appendingPathComponent("live/Meguri.app", isDirectory: true)
            self.product = product
            let mainURL = product.appendingPathComponent(Self.executable)
            self.mainURL = mainURL
            let physicalMainURL = internalLink ? product.appendingPathComponent("Contents/MacOS/Versions/A/Meguri") : mainURL
            self.physicalMainURL = physicalMainURL
            let agentURL = product.appendingPathComponent("Contents/PlugIns/MeguriSnapshotTests.xctest/" + Self.agentExecutable)
            self.agentURL = agentURL
            let debugURL = product.appendingPathComponent("Contents/MacOS/Meguri.debug.dylib")
            self.debugURL = debugURL
            let mainBytes = Self.image(1), agentBytes = Self.image(2), debugBytes = Self.fat(Self.image(3, subtype: 2), subtype: 2)
            self.mainBytes = mainBytes; self.agentBytes = agentBytes; self.debugBytes = debugBytes
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o755])
            var entries: [String: Any] = [:]
            func recordParents(_ key: String) {
                let parts = key.split(separator: "/")
                for count in 1..<parts.count { entries[parts.prefix(count).joined(separator: "/")] = ["type": "directory", "mode": 0o755] }
            }
            func write(_ key: String, _ data: Data, mode: Int = 0o755) throws {
                recordParents(key)
                entries[key] = ["type": "file", "sha256": AgentJSON.hash(data), "bytes": data.count, "mode": mode]
                // Only locally generated paths are used to create these files.
                let url = directory.appendingPathComponent("live/" + key)
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true,
                                                        attributes: [.posixPermissions: 0o755])
                try data.write(to: url)
                guard chmod(url.path, mode_t(mode)) == 0 else { throw AgentFailure.storage }
            }
            let physicalMain = internalLink ? Self.app + "/Contents/MacOS/Versions/A/Meguri" : Self.app + "/" + Self.executable
            try write(physicalMain, mainBytes)
            if internalLink {
                entries[Self.app + "/" + Self.executable] = ["type": "symlink", "target": "Versions/A/Meguri"]
                guard symlink("Versions/A/Meguri", mainURL.path) == 0 else { throw AgentFailure.storage }
            }
            try write(Self.agent + "/" + Self.agentExecutable, agentBytes)
            try write(Self.debug, debugBytes, mode: 0o644) // dyld images need not carry execute permission
            if widgets {
                for key in AgentRunningProductTests.widgetDebugs { try write(key, Self.image(4), mode: 0o644) }
            }
            let xctestrun = Data("generated-xctestrun".utf8)
            try write("CloudAgent.xctestrun", xctestrun, mode: 0o644)
            let buildPath = Self.hostRoot + "/build/mg/build-app/" + Self.buildRun + "/agent-build.json"
            let deploymentPath = Self.hostRoot + "/build/mg/cloudkit-agent-install/" + Self.installRun
            let productsPath = Self.hostRoot + "/build/products"
            let build = try Self.json(["protocol_version": 1, "job": "build-app", "run_id": Self.buildRun,
                "source_commit": Self.commit, "scheme": "Meguri", "configuration": "Debug", "test_plan": "CloudAgent",
                "platform": kind == "mac" ? "mac" : "ios-sim", "signing": "team", "app_build": "app-sample-7",
                "app": ["path": Self.app, "bundle_id": "generated.host", "executable": Self.executable, "executable_sha256": AgentJSON.hash(mainBytes)],
                "agent": ["path": Self.agent, "bundle_id": "generated.agent", "executable": Self.agentExecutable, "executable_sha256": AgentJSON.hash(agentBytes)],
                "xctestrun": Self.ref("CloudAgent.xctestrun", xctestrun), "products": ["path": productsPath, "entries": entries]])
            let buildReceipt = try Self.receipt("build-app", run: Self.buildRun, kind: kind,
                artifacts: [Self.artifact(buildPath), Self.artifact(productsPath, "directory")], key: "agent_build", reference: Self.ref(buildPath, build))
            var header = try JSONSerialization.jsonObject(with: CloudKitAgentProtocolTests.header) as! [String: Any]
            header["device_kind"] = kind
            header["executable_sha256"] = AgentJSON.hash(mainBytes)
            header["build_receipt"] = ["job": "build-app", "run_id": Self.buildRun]
            header["build_receipt_sha256"] = AgentJSON.hash(buildReceipt)
            header["deployment_receipt"] = ["job": "cloudkit-agent-install", "run_id": Self.installRun]
            let originalHeader = try Self.json(header)
            self.originalHeader = originalHeader
            let delivery = Data("generated-independent-artifact-not-client-proof".utf8)
            let deliveredPath = deploymentPath + "/delivered-manifest.json"
            let runPath = deploymentPath + "/CloudAgent-derived.xctestrun"
            let installationPath = deploymentPath + "/installation.json"
            let deployment = try Self.json(["protocol_version": 1, "job": "cloudkit-agent-install", "run_id": Self.installRun,
                "source_commit": Self.commit, "parent_run": header["run_id"]!, "device": header["device"]!, "device_kind": kind,
                "os_build": header["os_build"]!, "header_sha256": AgentJSON.hash(originalHeader),
                "build_receipt": header["build_receipt"]!, "build_receipt_sha256": AgentJSON.hash(buildReceipt),
                "build_manifest": Self.ref(buildPath, build), "delivered_manifest": Self.ref(deliveredPath, delivery),
                "xctestrun": Self.ref(runPath, xctestrun),
                "installation": kind == "mac" ? NSNull() : Self.ref(installationPath, delivery)])
            var artifacts = [Self.artifact(deploymentPath + "/agent-deployment.json"), Self.artifact(deploymentPath + "/run.json"),
                             Self.artifact(deliveredPath), Self.artifact(runPath, "xctestrun")]
            if kind != "mac" { artifacts.append(Self.artifact(installationPath)) }
            let deploymentReceipt = try Self.receipt("cloudkit-agent-install", run: Self.installRun, kind: kind, artifacts: artifacts,
                key: "agent_deployment", reference: Self.ref(deploymentPath + "/agent-deployment.json", deployment))
            let evidence = directory.appendingPathComponent("product-evidence", isDirectory: true)
            try FileManager.default.createDirectory(at: evidence, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o755])
            let copies: [Copy: Data] = [.buildReceipt: buildReceipt, .buildManifest: build,
                                        .deploymentReceipt: deploymentReceipt, .deploymentManifest: deployment]
            for copy in Copy.allCases {
                let url = evidence.appendingPathComponent(copy.rawValue)
                try copies[copy]!.write(to: url)
                guard chmod(url.path, 0o644) == 0 else { throw AgentFailure.storage }
            }
            receipts = try AgentProductReceiptBinding(admittedRunDirectory: directory, originalHeaderBytes: originalHeader)
            let host = Observation.BundleIdentity(root: product.path, canonicalRoot: product.path, bundleID: "generated.host", executable: mainURL.path)
            let agentRoot = agentURL.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().path
            let agent = Observation.BundleIdentity(root: agentRoot, canonicalRoot: agentRoot, bundleID: "generated.agent", executable: agentURL.path)
            observed = .init(host: host, agent: agent, appBuild: "app-sample-7", executablePath: mainURL.path, anchorImagePath: agentURL.path,
                images: [.init(path: physicalMainURL.path, identity: try Self.identity(1)),
                         .init(path: agentURL.path, identity: try Self.identity(2)), .init(path: debugURL.path, identity: try Self.identity(3, subtype: 2))],
                os: kind == "mac" ? .mac(build: "os-sample-7") : .simulator(runtimeBuild: "os-sample-7"))
        }
        private static func identity(_ byte: UInt8, subtype: UInt32 = 0) throws -> AgentRunningImageIdentity {
            try AgentRunningMachO.identity(AgentRunningProductTests.image(byte, subtype: subtype))
        }
        // Refer to enclosing fixture constants explicitly (nested types have no
        // inherited static members).
        private static var app: String { AgentRunningProductTests.app }
        private static var executable: String { AgentRunningProductTests.executable }
        private static var agent: String { AgentRunningProductTests.agent }
        private static var agentExecutable: String { AgentRunningProductTests.agentExecutable }
        private static var debug: String { AgentRunningProductTests.debug }
        private static var commit: String { AgentRunningProductTests.commit }
        private static var hostRoot: String { AgentRunningProductTests.hostRoot }
        private static var buildRun: String { AgentRunningProductTests.buildRun }
        private static var installRun: String { AgentRunningProductTests.installRun }
        private static func image(_ byte: UInt8, subtype: UInt32 = 0) -> Data { AgentRunningProductTests.image(byte, subtype: subtype) }
        private static func fat(_ bytes: Data, subtype: UInt32) -> Data { AgentRunningProductTests.fat(bytes, subtype: subtype) }
        private static func json(_ object: [String: Any]) throws -> Data { try AgentRunningProductTests.json(object) }
        private static func ref(_ path: String, _ bytes: Data) -> [String: Any] { AgentRunningProductTests.ref(path, bytes) }
        private static func artifact(_ path: String, _ kind: String = "json") -> [String: Any] { AgentRunningProductTests.artifact(path, kind) }
        private static func receipt(_ job: String, run: String, kind: String, artifacts: [[String: Any]], key: String,
                                    reference: [String: Any]) throws -> Data {
            try AgentRunningProductTests.receipt(job, run: run, kind: kind, artifacts: artifacts, key: key, reference: reference)
        }
        func candidate(hostID: String? = nil, agentID: String? = nil, appBuild: String? = nil,
                       executablePath: String? = nil, anchor: String? = nil, hostExecutable: String? = nil,
                       agentRoot: String? = nil, images: [AgentRunningImage]? = nil, os: Observation.OS? = nil) -> Observation {
            let host = Observation.BundleIdentity(root: observed.host.root, canonicalRoot: observed.host.canonicalRoot,
                bundleID: hostID ?? observed.host.bundleID, executable: hostExecutable ?? observed.host.executable)
            let agent = Observation.BundleIdentity(root: agentRoot ?? observed.agent.root, canonicalRoot: agentRoot ?? observed.agent.canonicalRoot,
                bundleID: agentID ?? observed.agent.bundleID, executable: observed.agent.executable)
            return .init(host: host, agent: agent, appBuild: appBuild ?? observed.appBuild,
                         executablePath: executablePath ?? observed.executablePath, anchorImagePath: anchor ?? observed.anchorImagePath,
                         images: images ?? observed.images, os: os ?? observed.os)
        }
        func bind(_ observation: Observation? = nil, refresh: (() throws -> Observation)? = nil,
                  boundary: ((AgentRunningFileAuthority.Boundary) throws -> Void)? = nil) throws -> AgentRunningProductBinding {
            try .generatedUnitOnly(receipts: receipts, observation: observation ?? observed, refresh: refresh, boundary: boundary)
        }
    }
    private func withFixture(kind: String = "mac", internalLink: Bool = false, widgets: Bool = false,
                             _ body: (Fixture) throws -> Void) throws {
        let f = try Fixture(kind: kind, internalLink: internalLink, widgets: widgets)
        defer { try? FileManager.default.removeItem(at: f.directory) }
        try body(f)
    }
    private func replace(_ url: URL, with bytes: Data) throws {
        try FileManager.default.moveItem(at: url, to: url.appendingPathExtension("saved"))
        try bytes.write(to: url)
        guard chmod(url.path, 0o755) == 0 else { throw AgentFailure.storage }
    }

    @Test func generatedObservationsRetainExactReceiptsWithoutGrantingSignedExecution() throws {
        try withFixture { f in
            let binding = try f.bind()
            #expect(binding.coverage == .generatedUnitOnly)
            #expect(binding.receipts.originalHeaderBytes == f.originalHeader)
            #expect(binding.observation == f.observed)
            #expect(binding.missingIndependentObservations == Set(AgentRunningProductBinding.MissingIndependentObservation.allCases))
            #expect(binding.receipts.deferredObservations == Set(AgentProductReceiptBinding.DeferredObservation.allCases))
            try binding.requireHeader(f.receipts.header, originalHeaderBytes: f.originalHeader)
            #expect(throws: AgentFailure.binding) {
                try binding.requireHeader(f.receipts.header, originalHeaderBytes: f.originalHeader + Data(" ".utf8))
            }
            #expect(throws: AgentFailure.implementationMissing) { try binding.requireIndependentCounterpart() }
            #if !MEGURI_TEST_HOOKS
            #expect(throws: AgentFailure.implementationMissing) { try AgentRunningProductBinding.inspectActual(receipts: f.receipts) }
            #endif
        }
    }
    @Test func standaloneAndEmbeddedWidgetDebugImagesNeedNotBeHostLoaded() throws {
        try withFixture(widgets: true) { f in
            for key in Self.widgetDebugs {
                #expect(f.receipts.build.products.entries[key] != nil)
                let path = f.directory.appendingPathComponent("live/" + key).path
                #expect(!f.observed.images.contains { $0.path == path })
            }
            let binding = try f.bind()
            try binding.revalidate()
            #expect(binding.coverage == .generatedUnitOnly)
            #expect(binding.missingIndependentObservations == Set(AgentRunningProductBinding.MissingIndependentObservation.allCases))
            #expect(binding.receipts.deferredObservations == Set(AgentProductReceiptBinding.DeferredObservation.allCases))
            #expect(throws: AgentFailure.implementationMissing) { try binding.requireIndependentCounterpart() }
            let missingHostDebug = f.observed.images.filter { $0.path != f.debugURL.path }
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: missingHostDebug)) }
        }
    }
    @Test func injectedSystemImageBeforeExecutableDoesNotSelectMainByPosition() throws {
        try withFixture { f in
            let system = AgentRunningImage(path: "/usr/lib/libRPAC.dylib", identity: f.observed.images[0].identity)
            let observation = f.candidate(images: [system] + f.observed.images)
            // This is the exact selector used by both observation producers.
            #expect(try observation.executableImage() == f.observed.images[0])
            #expect(try observation.executableImage().path == observation.executablePath)
            let binding = try f.bind(observation)
            try binding.revalidate()
            #expect(binding.coverage == .generatedUnitOnly)
            #expect(throws: AgentFailure.implementationMissing) { try binding.requireIndependentCounterpart() }

            let missing = f.candidate(images: [system] + Array(f.observed.images.dropFirst()))
            let duplicate = f.candidate(images: [system] + f.observed.images + [f.observed.images[0]])
            let mismatched = f.candidate(executablePath: f.agentURL.path, images: observation.images)
            let systemExecutable = f.candidate(executablePath: system.path, hostExecutable: system.path, images: observation.images)
            for candidate in [missing, duplicate, mismatched, systemExecutable] {
                #expect(throws: AgentFailure.binding) { try candidate.executableImage() }
                #expect(throws: AgentFailure.binding) { try f.bind(candidate) }
            }
            for identity in [AgentRunningImageIdentity(cpu: Self.cpu, subtype: 0, uuid: Data(repeating: 9, count: 16)),
                             AgentRunningImageIdentity(cpu: 7, subtype: 0, uuid: f.observed.images[0].identity.uuid),
                             AgentRunningImageIdentity(cpu: Self.cpu, subtype: 2, uuid: f.observed.images[0].identity.uuid)] {
                var images = observation.images
                images[1] = .init(path: f.mainURL.path, identity: identity)
                #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: images)) }
            }
        }
    }
    @Test func refreshIgnoresSystemOrderingAndKeepsOwnedIdentitiesAndExecutableFences() throws {
        for internalLink in [false, true] {
            try withFixture(internalLink: internalLink) { f in
                let system = AgentRunningImage(path: "/usr/lib/libRPAC.dylib", identity: f.observed.images[0].identity)
                let original = f.candidate(images: [system] + f.observed.images)
                var current = original
                let binding = try f.bind(original, refresh: { current })
                current = f.candidate(images: f.observed.images + [system])
                try binding.revalidate()
                current = f.candidate(images: [f.observed.images[0], system] + Array(f.observed.images.dropFirst()))
                try binding.revalidate()
                #expect(binding.coverage == .generatedUnitOnly)
                #expect(throws: AgentFailure.implementationMissing) { try binding.requireIndependentCounterpart() }

                var changedIdentity = f.observed.images
                changedIdentity[0] = .init(path: changedIdentity[0].path,
                    identity: .init(cpu: Self.cpu, subtype: 0, uuid: Data(repeating: 9, count: 16)))
                let failures = [
                    f.candidate(images: [system] + Array(f.observed.images.dropFirst())),
                    f.candidate(images: original.images + [f.observed.images[0]]),
                    f.candidate(images: [system] + changedIdentity),
                    f.candidate(executablePath: f.agentURL.path, images: original.images),
                    f.candidate(anchor: f.mainURL.path, images: original.images),
                    f.candidate(hostExecutable: f.agentURL.path, images: original.images),
                    f.candidate(images: original.images + [.init(path: f.product.path + "/unlisted", identity: system.identity)]),
                    f.candidate(images: [system] + Array(f.observed.images.dropLast()))
                ]
                for failure in failures {
                    current = original
                    let retained = try f.bind(original, refresh: { current })
                    current = failure
                    #expect(throws: AgentFailure.binding) { try retained.revalidate() }
                    #expect(throws: AgentFailure.storage) { try retained.revalidate() }
                }
                if internalLink {
                    current = original
                    let retained = try f.bind(original, refresh: { current })
                    // A declared alias may bind initially, but changing the OS
                    // executable spelling after retention still fails.
                    current = f.candidate(executablePath: f.physicalMainURL.path, images: original.images)
                    #expect(throws: AgentFailure.binding) { try retained.revalidate() }
                }
            }
        }
    }
    @Test func widgetProductsDoNotRelaxLoadedHostImageInventoryUUIDAndHashChecks() throws {
        try withFixture(widgets: true) { f in
            let binding = try f.bind()
            try binding.revalidate()
            let stale = try f.observed.images.map { image in
                image.path == f.debugURL.path
                    ? AgentRunningImage(path: image.path, identity: try AgentRunningMachO.identity(Self.image(9, subtype: 2)))
                    : image
            }
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: stale)) }
            let unlisted = f.observed.images + [AgentRunningImage(path: f.product.path + "/Unknown.debug.dylib",
                identity: f.observed.images[0].identity)]
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: unlisted)) }
            var changedHash = f.debugBytes
            changedHash[28] = 1 // Fat-table padding: same size and selected Mach-O UUID.
            try changedHash.write(to: f.debugURL)
            #expect(throws: AgentFailure.binding) { try f.bind() }
            #expect(throws: AgentFailure.storage) { try binding.revalidate() }
        }
    }
    @Test func hostAndAgentIdentityVersionAndMainPathMustAllMatch() throws {
        try withFixture { f in
            let rejected = [f.candidate(hostID: "other"), f.candidate(agentID: "other"), f.candidate(appBuild: ""),
                f.candidate(appBuild: "other"), f.candidate(executablePath: f.agentURL.path),
                f.candidate(hostExecutable: f.agentURL.path), f.candidate(agentRoot: f.product.path),
                f.candidate(anchor: f.mainURL.path)]
            for candidate in rejected { #expect(throws: AgentFailure.binding) { try f.bind(candidate) } }
            try Self.image(9).write(to: f.physicalMainURL)
            #expect(throws: (any Error).self) { try f.bind() }
        }
    }
    @Test func unchangedLauncherCannotAdmitStaleAgentOrDebugDylib() throws {
        for target in ["agent", "debug"] {
            try withFixture { f in
                #expect(try Data(contentsOf: f.physicalMainURL) == f.mainBytes)
                try Self.image(9).write(to: target == "agent" ? f.agentURL : f.debugURL)
                #expect(throws: (any Error).self) { try f.bind() }
            }
        }
    }
    @Test func missingDuplicateWrongPathUUIDAndCPUOfLoadedImagesRefuse() throws {
        try withFixture { f in
            for index in f.observed.images.indices {
                var missing = f.observed.images; missing.remove(at: index)
                #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: missing)) }
                var duplicate = f.observed.images; duplicate.append(duplicate[index])
                #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: duplicate)) }
                for path in [f.product.path + "/unlisted", f.directory.path + "/external"] {
                    var images = f.observed.images
                    images[index] = .init(path: path, identity: images[index].identity)
                    #expect(throws: (any Error).self) { try f.bind(f.candidate(images: images)) }
                }
                for identity in [AgentRunningImageIdentity(cpu: Self.cpu, subtype: 0, uuid: Data(repeating: 8, count: 16)),
                                 AgentRunningImageIdentity(cpu: 7, subtype: 0, uuid: f.observed.images[index].identity.uuid)] {
                    var images = f.observed.images; images[index] = .init(path: images[index].path, identity: identity)
                    #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(images: images)) }
                }
            }
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(anchor: f.directory.path + "/external")) }
        }
    }
    @Test func osMismatchAndSimulatorRuntimeInputCannotSupplyIndependentDestination() throws {
        try withFixture { f in
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(os: .mac(build: "other"))) }
            #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(os: .simulator(runtimeBuild: "os-sample-7"))) }
        }
        for kind in ["iphone", "ipad"] {
            try withFixture(kind: kind) { f in
                let binding = try f.bind()
                #expect(throws: AgentFailure.implementationMissing) { try binding.requireIndependentCounterpart() }
                #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(os: .simulator(runtimeBuild: "other"))) }
                #expect(throws: AgentFailure.binding) { try f.bind(f.candidate(os: .mac(build: "os-sample-7"))) }
            }
        }
    }
    @Test func declaredInternalLinksWorkButActualExternalOrUnlistedAliasesRefuse() throws {
        try withFixture(internalLink: true) { f in
            let binding = try f.bind(); try binding.revalidate()
            let outside = f.directory.appendingPathComponent("external")
            try f.mainBytes.write(to: outside)
            guard unlink(f.mainURL.path) == 0, symlink(outside.path, f.mainURL.path) == 0 else { throw AgentFailure.storage }
            #expect(throws: AgentFailure.storage) { try binding.revalidate() }
            #expect(throws: (any Error).self) { try f.bind() }
        }
        try withFixture { f in
            let alias = f.product.appendingPathComponent("unlisted-alias")
            guard symlink(Self.executable, alias.path) == 0 else { throw AgentFailure.storage }
            #expect(throws: (any Error).self) { try f.bind(f.candidate(executablePath: alias.path)) }
        }
    }
    @Test func mutationAndReplacementAtBothReadBoundariesRefuse() throws {
        for boundary in [AgentRunningFileAuthority.Boundary.beforeRead, .afterRead] {
            for mutation in ["content", "replacement", "ancestor", "restored-ancestor"] {
                try withFixture { f in
                    var changed = false
                    #expect(throws: AgentFailure.storage) {
                        try f.bind(boundary: { point in
                            guard !changed, point == boundary else { return }; changed = true
                            switch mutation {
                            case "content": try Self.image(8).write(to: f.physicalMainURL)
                            case "replacement": try replace(f.physicalMainURL, with: f.mainBytes)
                            default:
                                let parent = f.physicalMainURL.deletingLastPathComponent(), saved = parent.appendingPathExtension("saved")
                                try FileManager.default.moveItem(at: parent, to: saved)
                                if mutation == "restored-ancestor" { try FileManager.default.moveItem(at: saved, to: parent) }
                                else {
                                    try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o755])
                                    try f.mainBytes.write(to: f.physicalMainURL)
                                }
                            }
                        })
                    }
                    #expect(changed)
                }
            }
        }
    }
    @Test func retainedAuthorityRejectsLaterFileAncestorAndReceiptSubstitution() throws {
        for mutation in ["content", "replacement", "ancestor", "mode", "receipt"] {
            try withFixture { f in
                let binding = try f.bind()
                switch mutation {
                case "content": try Self.image(8).write(to: f.agentURL)
                case "replacement": try replace(f.agentURL, with: f.agentBytes)
                case "ancestor":
                    let parent = f.agentURL.deletingLastPathComponent(), saved = parent.appendingPathExtension("saved")
                    try FileManager.default.moveItem(at: parent, to: saved)
                    try FileManager.default.moveItem(at: saved, to: parent)
                case "mode": guard chmod(f.agentURL.path, 0o777) == 0 else { throw AgentFailure.storage }
                default:
                    let receipt = f.directory.appendingPathComponent(Copy.buildReceipt.relativePath)
                    try replace(receipt, with: f.receipts.originalBytes[.buildReceipt]!)
                }
                #expect(throws: AgentFailure.storage) { try binding.revalidate() }
                #expect(throws: AgentFailure.storage) { try binding.revalidate() }
            }
        }
    }
    @Test func directoryTimestampsAndLinkCountsAreNotIdentity() throws {
        try withFixture { f in
            let binding = try f.bind()
            let unrelated = f.product.appendingPathComponent("unrelated-directory", isDirectory: true)
            guard mkdir(unrelated.path, 0o755) == 0 else { throw AgentFailure.storage }
            try binding.revalidate()
            try FileManager.default.removeItem(at: unrelated)
            try binding.revalidate()
        }
    }
    @Test func boundedStringsUseFirstNULWithinReturnedCountAndMaximum() throws {
        #expect(try AgentRunningProductInspector.boundedString([65, 0, -1], count: 3, maximum: 256) == "A")
        #expect(try AgentRunningProductInspector.boundedString([-61, -87, 0, -1], count: 3, maximum: 256) == "é")
        #expect(try AgentRunningProductInspector.boundedString([0], count: 1, maximum: 256) == "")
        for maximum in [256, 4096] {
            let bytes = [CChar](repeating: 65, count: maximum - 1) + [0]
            #expect(try AgentRunningProductInspector.boundedString(bytes, count: maximum, maximum: maximum)
                == String(repeating: "A", count: maximum - 1))
            #expect(throws: AgentFailure.implementationMissing) {
                try AgentRunningProductInspector.boundedString(bytes, count: maximum - 1, maximum: maximum)
            }
            #expect(throws: AgentFailure.implementationMissing) {
                try AgentRunningProductInspector.boundedString(bytes + [0], count: maximum + 1, maximum: maximum)
            }
        }
        for count in [-1, 0, 1, 3] {
            #expect(throws: AgentFailure.implementationMissing) {
                try AgentRunningProductInspector.boundedString([65, 0], count: count, maximum: 256)
            }
        }
    }
    @Test func boundedStringsRefuseMalformedUTF8BeforeFirstNUL() throws {
        let malformed: [[CChar]] = [[-1, 0], [-61, 0], [-64, -128, 0]]
        for bytes in malformed {
            #expect(throws: AgentFailure.binding) {
                try AgentRunningProductInspector.boundedString(bytes, count: bytes.count, maximum: 256)
            }
        }
    }
    @Test func boundedMachOParsesArm64UniversalArm64eAndRefusesAmbiguity() throws {
        let arm64 = try AgentRunningMachO.identity(Self.image(1))
        let arm64e = try AgentRunningMachO.identity(Self.image(2, subtype: 2))
        #expect(try AgentRunningMachO.identity(Self.fat(Self.image(2, subtype: 2), subtype: 2), matching: arm64e) == arm64e)
        #expect(throws: AgentFailure.binding) { try AgentRunningMachO.identity(Self.image(2), matching: arm64) }
        #expect(throws: AgentFailure.binding) { try AgentRunningMachO.identity(Self.fat(Self.image(2, subtype: 2), subtype: 2), matching: arm64) }
        #expect(throws: AgentFailure.implementationMissing) { try AgentRunningMachO.identity(Data(repeating: 0, count: 32)) }
        for offset in [16, 20, 36] {
            var bytes = Self.image(1)
            bytes.replaceSubrange(offset..<(offset + 4), with: Self.word(UInt32.max))
            #expect(throws: AgentFailure.binding) { try AgentRunningMachO.identity(bytes) }
        }
        for count in [0, 3, 31, 55] {
            #expect(throws: AgentFailure.binding) { try AgentRunningMachO.identity(Data(Self.image(1).prefix(count))) }
        }
    }
}
