import Darwin
import Foundation
import Testing

@Suite(.serialized) nonisolated struct AgentProductReceiptBindingTests {
    private typealias Copy = AgentProductReceiptBinding.Copy
    private static let root = "/generated-checkout"
    private static let buildRun = "20320405T060708Z-2", installRun = "20320405T060708Z-3"
    private static let buildPath = root + "/build/mg/build-app/" + buildRun + "/agent-build.json"
    private static let installPath = root + "/build/mg/cloudkit-agent-install/" + installRun
    private static let products = root + "/build/products"
    private static let hash = String(repeating: "b", count: 64)
    private static let otherHash = String(repeating: "d", count: 64)

    // All host paths are synthetic metadata. Only these four generated copies
    // in a canonical run-owned temp directory can be read by the foundation.
    private final class Run {
        let directory: URL
        var evidence: URL { directory.appendingPathComponent("product-evidence", isDirectory: true) }
        init() throws {
            guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            directory = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
                .appendingPathComponent("meguri-receipt-binding-" + UUID().uuidString, isDirectory: true)
            guard mkdir(directory.path, 0o755) == 0, mkdir(evidence.path, 0o755) == 0 else { throw AgentFailure.storage }
        }
        func url(_ copy: Copy) -> URL { evidence.appendingPathComponent(copy.rawValue) }
        func write(_ packet: Packet) throws {
            for copy in Copy.allCases { try write(packet.bytes[copy]!, copy) }
        }
        func write(_ bytes: Data, _ copy: Copy) throws {
            try bytes.write(to: url(copy))
            guard chmod(url(copy).path, 0o644) == 0 else { throw AgentFailure.storage }
        }
        func admit(_ packet: Packet, afterRead: ((Copy) throws -> Void)? = nil) throws -> AgentProductReceiptBinding {
            try AgentProductReceiptBinding(admittedRunDirectory: directory, originalHeaderBytes: packet.header, afterRead: afterRead)
        }
    }
    private struct Packet { var header: Data; var bytes: [Copy: Data] }
    private struct Draft {
        var header: [String: Any]
        var build: [String: Any], buildReceipt: [String: Any]
        var deployment: [String: Any], deploymentReceipt: [String: Any]
    }
    private static func encode(_ object: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    }
    private static func object(_ bytes: Data) throws -> [String: Any] {
        guard let object = try JSONSerialization.jsonObject(with: bytes) as? [String: Any] else { throw AgentFailure.shape }
        return object
    }
    private static func reference(_ path: String, _ bytes: Data) -> [String: Any] {
        ["path": path, "sha256": AgentJSON.hash(bytes), "bytes": bytes.count]
    }
    private static func artifact(_ path: String, _ kind: String = "json") -> [String: Any] { ["kind": kind, "path": path] }
    private static func receipt(_ job: String, _ run: String, artifacts: [[String: Any]]) -> [String: Any] {
        ["schema_version": 1, "job": job, "run_id": run, "status": "pass", "exit_code": 0,
         "placement": "host", "service": NSNull(), "dry_run": false,
         "args": job == "build-app" ? ["--platform", "mac", "--test-plan", "CloudAgent"] : [],
         "root": root, "started_at": "2032-04-05T06:07:08.125Z", "finished_at": "2032-04-05T06:08:08Z", "duration_ms": 59875,
         "commit": ["sha": String(repeating: "a", count: 40), "dirty": false], "message": "generated receipt",
         "summary": ["total": 1, "passed": 1, "failed": 0, "skipped": 0, "future_count": 2],
         "failures": [], "artifacts": artifacts, "user_steps": [], "details": ["future_metadata": ["ratio": 1.25]]]
    }
    private static func draft(_ kind: String) throws -> Draft {
        var header = try object(CloudKitAgentProtocolTests.header)
        header["device_kind"] = kind
        header["build_receipt"] = ["job": "build-app", "run_id": buildRun]
        header["deployment_receipt"] = ["job": "cloudkit-agent-install", "run_id": installRun]
        let app = kind == "mac" ? "Meguri.app/Contents/MacOS/Meguri" : "Meguri.app/Meguri"
        let agentRoot = kind == "mac" ? "Meguri.app/Contents/PlugIns/MeguriSnapshotTests.xctest" : "Meguri.app/PlugIns/MeguriSnapshotTests.xctest"
        let agentExecutable = kind == "mac" ? "Contents/MacOS/MeguriSnapshotTests" : "MeguriSnapshotTests"
        var entries: [String: Any] = [:]
        func file(_ path: String, hash: String, bytes: Int, mode: Int) {
            let components = path.split(separator: "/")
            if components.count > 1 {
                for count in 1..<components.count { entries[components.prefix(count).joined(separator: "/")] = ["type": "directory", "mode": 0o755] }
            }
            entries[path] = ["type": "file", "sha256": hash, "bytes": bytes, "mode": mode]
        }
        file(app, hash: hash, bytes: 20, mode: 0o755)
        file(agentRoot + "/" + agentExecutable, hash: otherHash, bytes: 40, mode: 0o755)
        file("CloudAgent.xctestrun", hash: otherHash, bytes: 55, mode: 0o644)
        file("Meguri.app/Resources/Versions/A/input", hash: otherHash, bytes: 7, mode: 0o644)
        // The generated link steps up two directories; inventory resolution performs no I/O.
        entries["Meguri.app/Resources/Current"] = ["type": "symlink", "target": "Versions/A"]
        entries["Meguri.app/Resources/Versions/A/Back"] = ["type": "symlink", "target": "../../Current/input"]
        let build: [String: Any] = ["protocol_version": 1, "job": "build-app", "run_id": buildRun,
            "source_commit": header["source_commit"]!, "scheme": "Meguri", "configuration": "Debug", "test_plan": "CloudAgent",
            "platform": kind == "mac" ? "mac" : "ios-sim", "signing": "team", "app_build": header["app_build"]!,
            "app": ["path": "Meguri.app", "bundle_id": "generated.meguri.dev", "executable": kind == "mac" ? "Contents/MacOS/Meguri" : "Meguri", "executable_sha256": hash],
            "agent": ["path": agentRoot, "bundle_id": "generated.MeguriSnapshotTests", "executable": agentExecutable, "executable_sha256": otherHash],
            "xctestrun": ["path": "CloudAgent.xctestrun", "sha256": otherHash, "bytes": 55],
            "products": ["path": products, "entries": entries]]
        let deliveryPath = installPath + "/delivered-manifest.json", xctestrun = installPath + "/CloudAgent-derived.xctestrun"
        let installation = installPath + "/installation.json"
        var deploymentArtifacts = [artifact(installPath + "/agent-deployment.json"), artifact(installPath + "/run.json"), artifact(deliveryPath), artifact(xctestrun, "xctestrun")]
        if kind != "mac" { deploymentArtifacts.append(artifact(installation)) }
        let deployment: [String: Any] = ["protocol_version": 1, "job": "cloudkit-agent-install", "run_id": installRun,
            "source_commit": header["source_commit"]!, "parent_run": header["run_id"]!, "device": header["device"]!,
            "device_kind": kind, "os_build": header["os_build"]!, "header_sha256": String(repeating: "0", count: 64),
            "build_receipt": header["build_receipt"]!, "build_receipt_sha256": String(repeating: "0", count: 64),
            "build_manifest": NSNull(), "delivered_manifest": ["path": deliveryPath, "sha256": otherHash, "bytes": 123],
            "xctestrun": ["path": xctestrun, "sha256": otherHash, "bytes": 56],
            "installation": kind == "mac" ? NSNull() : ["path": installation, "sha256": otherHash, "bytes": 100]]
        return Draft(header: header, build: build,
            buildReceipt: receipt("build-app", buildRun, artifacts: [artifact(buildPath), artifact(products, "directory")]),
            deployment: deployment, deploymentReceipt: receipt("cloudkit-agent-install", installRun, artifacts: deploymentArtifacts))
    }
    // Rebind enclosing digests after a schema mutation so each negative test
    // reaches the intended validation rather than merely failing an outer hash.
    private static func packet(_ kind: String = "mac", edit: ((inout Draft) throws -> Void)? = nil,
                               editBuildBytes: ((Data) -> Data)? = nil,
                               editBuildReceiptBytes: ((Data) -> Data)? = nil) throws -> Packet {
        var d = try draft(kind)
        try edit?(&d)
        var buildBytes = try encode(d.build)
        if let editBuildBytes { buildBytes = editBuildBytes(buildBytes) }
        let buildRef = reference(buildPath, buildBytes)
        var buildDetails = d.buildReceipt["details"] as! [String: Any]; buildDetails["agent_build"] = buildRef
        d.buildReceipt["details"] = buildDetails
        // Keep the fixture's actual public platform selection consistent.
        if kind != "mac", let args = d.buildReceipt["args"] as? [String],
           args == ["--platform", "mac", "--test-plan", "CloudAgent"] {
            d.buildReceipt["args"] = ["--platform", "ios-sim", "--test-plan", "CloudAgent"]
        }
        var buildReceiptBytes = try encode(d.buildReceipt)
        if let editBuildReceiptBytes { buildReceiptBytes = editBuildReceiptBytes(buildReceiptBytes) }
        d.header["build_receipt_sha256"] = AgentJSON.hash(buildReceiptBytes)
        let headerBytes = try encode(d.header)
        d.deployment["header_sha256"] = AgentJSON.hash(headerBytes)
        d.deployment["build_receipt_sha256"] = AgentJSON.hash(buildReceiptBytes)
        d.deployment["build_manifest"] = buildRef
        let deploymentBytes = try encode(d.deployment)
        var details = d.deploymentReceipt["details"] as! [String: Any]
        details["agent_deployment"] = reference(installPath + "/agent-deployment.json", deploymentBytes)
        d.deploymentReceipt["details"] = details
        return Packet(header: headerBytes, bytes: [.buildManifest: buildBytes, .buildReceipt: buildReceiptBytes,
            .deploymentManifest: deploymentBytes, .deploymentReceipt: try encode(d.deploymentReceipt)])
    }
    private func withRun(_ body: (Run) throws -> Void) throws {
        let run = try Run(); defer { try? FileManager.default.removeItem(at: run.directory) }; try body(run)
    }
    private static func replacing(_ bytes: Data, _ old: String, _ new: String) -> Data {
        Data(String(decoding: bytes, as: UTF8.self).replacingOccurrences(of: old, with: new).utf8)
    }
    private func refuses(_ packet: Packet) throws {
        try withRun { run in
            try run.write(packet)
            do {
                _ = try run.admit(packet)
                Issue.record("receipt binding unexpectedly admitted malformed generated evidence")
            } catch let failure as AgentFailure {
                #expect([AgentFailure.json, .shape, .version, .binding].contains(failure))
            } catch {
                Issue.record("receipt binding returned an unexpected untyped schema failure")
            }
        }
    }

    @Test func canonicalPOSIXFixtureIsAdmittedThroughAuthority() throws {
        let packet = try Self.packet()
        try withRun { run in
            try run.write(packet)
            let foundationStandardizationDiffers = run.directory.path != run.directory.standardizedFileURL.path
            print("canonicalPOSIXFixtureFoundationStandardizationDiffers=\(foundationStandardizationDiffers)")
            let bound = try run.admit(packet)
            let admittedThroughAuthority = bound.originalBytes == packet.bytes
            #expect(admittedThroughAuthority)
            try bound.revalidate()
        }
    }

    @Test func validMacAndSimulatorBindingsRetainOnlyReceiptClaims() throws {
        for kind in ["mac", "iphone", "ipad"] {
            let packet = try Self.packet(kind)
            try withRun { run in
                try run.write(packet)
                let bound = try run.admit(packet)
                #expect(bound.originalBytes == packet.bytes)
                #expect(bound.originalHeaderBytes == packet.header)
                #expect(bound.buildReceipt.root == Self.root && bound.deploymentReceipt.root == Self.root)
                #expect(bound.build.testPlan == "CloudAgent" && bound.build.signing == "team")
                #expect((bound.deployment.installation == nil) == (kind == "mac"))
                #expect(bound.deferredObservations == Set(AgentProductReceiptBinding.DeferredObservation.allCases))
                #expect(bound.deployment.deliveredManifest.path.hasSuffix("/delivered-manifest.json"))
                #expect(try bound.build.products.resolved("Meguri.app/Resources/Versions/A/Back") == "Meguri.app/Resources/Versions/A/input")
                #expect(bound.buildReceipt.details["future_metadata"] != nil)
                try bound.revalidate()
            }
        }
    }
    @Test func buildReceiptHashUsesOriginalBytes() throws {
        let packet = try Self.packet(editBuildReceiptBytes: { Data(" \n".utf8) + $0 + Data("\n".utf8) })
        try withRun { run in
            try run.write(packet)
            _ = try run.admit(packet)
            let normalized = try Self.encode(Self.object(packet.bytes[.buildReceipt]!))
            #expect(normalized != packet.bytes[.buildReceipt]!)
            try run.write(normalized, .buildReceipt)
            #expect(throws: AgentFailure.binding) { try run.admit(packet) }
        }
    }
    @Test func manifestHashAndCountTamperingRefuses() throws {
        for copy in [Copy.buildManifest, .deploymentManifest] {
            var packet = try Self.packet()
            packet.bytes[copy]! += Data(" ".utf8)
            try refuses(packet)
        }
        for field in ["sha256", "bytes", "path"] {
            var packet = try Self.packet()
            var receipt = try Self.object(packet.bytes[.deploymentReceipt]!)
            var details = receipt["details"] as! [String: Any]
            var reference = details["agent_deployment"] as! [String: Any]
            reference[field] = field == "bytes" ? 0 : field == "path" ? Self.installPath + "/other.json" : Self.hash
            details["agent_deployment"] = reference; receipt["details"] = details
            packet.bytes[.deploymentReceipt] = try Self.encode(receipt)
            try refuses(packet)
        }
    }
    @Test func receiptIdentitySourceOutcomeAndRequiredEnvelopeFieldsRefuse() throws {
        let mutations: [(String, Any)] = [
            ("job", "preview"), ("run_id", "20320405T060708Z-99"), ("root", "/another-checkout"),
            ("root", Self.root + "/../generated-checkout"), ("dry_run", true), ("status", "warn"), ("exit_code", 1),
            ("commit", ["sha": String(repeating: "f", count: 40), "dirty": false]),
            ("commit", ["sha": String(repeating: "a", count: 40), "dirty": true]),
            ("summary", ["total": 1, "passed": 0, "failed": 1, "skipped": 0]),
            ("details", ["status": "queued"]), ("details", ["nested": ["state": "awaiting_user"]]),
            ("failures", [["id": "generated", "message": "failure", "repro": "generated"]]),
            ("user_steps", [["id": "generated", "state": "missing", "required": true, "title": "t", "how": "h", "doc": "d"]]),
            ("artifacts", []), ("started_at", "invalid"), ("started_at", "2033-01-01T00:00:00Z")]
        for (key, value) in mutations {
            try refuses(Self.packet(edit: { $0.buildReceipt[key] = value }))
            try refuses(Self.packet(edit: { $0.deploymentReceipt[key] = value }))
        }
        for key in ["service", "args", "placement", "message", "duration_ms", "finished_at", "user_steps", "failures"] {
            try refuses(Self.packet(edit: { $0.buildReceipt.removeValue(forKey: key) }))
        }
    }
    @Test func completeNestedSchemaAndNumericTokensAreStrict() throws {
        let receiptMutations: [(String, Any)] = [
            ("extra", 1), ("schema_version", true), ("exit_code", "0"), ("duration_ms", -1),
            ("service", 4), ("args", [true]), ("message", false), ("commit", ["sha": NSNull(), "dirty": false, "extra": 1]),
            ("summary", ["total": 1, "passed": 1, "failed": 0, "skipped": 0, "extension": true]),
            ("artifacts", [["kind": "json", "path": Self.buildPath, "label": 1]]),
            ("failures", [["id": "", "message": 1, "repro": "x"]]),
            ("user_steps", [["id": "BAD", "state": "ready", "required": "false", "title": "t", "how": "h", "doc": "d"]])]
        for (key, value) in receiptMutations { try refuses(Self.packet(edit: { $0.buildReceipt[key] = value })) }
        for token in ["1.0", "1e0", "true", "\"1\""] {
            try refuses(Self.packet(editBuildReceiptBytes: { Self.replacing($0, "\"schema_version\":1", "\"schema_version\":" + token) }))
            try refuses(Self.packet(editBuildBytes: { Self.replacing($0, "\"protocol_version\":1", "\"protocol_version\":" + token) }))
        }
        for (old, new) in [("\"mode\":493", "\"mode\":493.0"), ("\"bytes\":55", "\"bytes\":55e0"),
                           ("\"mode\":493", "\"mode\":true"), ("\"mode\":493", "\"mode\":4096")] {
            try refuses(Self.packet(editBuildBytes: { Self.replacing($0, old, new) }))
        }
        // Escaped duplicate spelling is rejected recursively before decoding.
        try refuses(Self.packet(editBuildBytes: { Self.replacing($0, "\"bundle_id\":", "\"bundle_id\":\"duplicate\",\"bundle_\\u0069d\":") }))
        try refuses(Self.packet(editBuildReceiptBytes: { Self.replacing($0, "\"dirty\":false", "\"dirty\":false,\"dirty\":false") }))
        try refuses(Self.packet(edit: { d in
            var app = d.build["app"] as! [String: Any]; app["extra"] = 1; d.build["app"] = app
        }))
        try refuses(Self.packet(edit: { $0.deployment["extra"] = "unknown" }))
    }
    @Test func legacyPreviewAdHocAndWrongBuildIdentityRefuse() throws {
        let mutations: [(String, Any)] = [("job", "preview"), ("run_id", "20320405T060708Z-99"),
            ("source_commit", String(repeating: "f", count: 40)), ("scheme", "MeguriStandalone"),
            ("configuration", "Profile"), ("configuration", "Release"), ("configuration", "AppStore"),
            ("test_plan", "UI"), ("signing", "ad-hoc"), ("platform", "ios-sim"), ("app_build", "different")]
        for (key, value) in mutations { try refuses(Self.packet(edit: { $0.build[key] = value })) }
        for args in [[], ["--test-plan", "UI"], ["--test-plan", "CloudAgent", "--test-plan", "UI"], ["MEGURI_TEST_PLAN=CloudAgent"]] {
            try refuses(Self.packet(edit: { $0.buildReceipt["args"] = args }))
        }
        try refuses(Self.packet(edit: { d in
            var app = d.build["app"] as! [String: Any]; app["executable_sha256"] = Self.otherHash; d.build["app"] = app
        }))
    }
    @Test func publicBuildArgumentPairsAllowExplicitOrDefaultDebug() throws {
        for kind in ["mac", "iphone", "ipad"] {
            let platform = kind == "mac" ? "mac" : "ios-sim"
            let selections = [
                ["--platform", platform, "--test-plan", "CloudAgent"],
                ["--test-plan", "CloudAgent", "--platform", platform],
                ["--platform", platform, "--test-plan", "CloudAgent", "--config", "Debug"],
                ["--config", "Debug", "--test-plan", "CloudAgent", "--platform", platform]]
            for args in selections {
                let packet = try Self.packet(kind, edit: { $0.buildReceipt["args"] = args })
                try withRun { run in
                    try run.write(packet)
                    let bound = try run.admit(packet)
                    #expect(bound.buildReceipt.args == args)
                    #expect(bound.build.configuration == "Debug")
                    #expect(bound.build.platform == platform && bound.build.testPlan == "CloudAgent")
                }
            }
        }
    }
    @Test func unsupportedOrMalformedBuildArgumentsRefuseAsBindingFailures() throws {
        for kind in ["mac", "iphone", "ipad"] {
            let platform = kind == "mac" ? "mac" : "ios-sim"
            let base = ["--platform", platform, "--test-plan", "CloudAgent"]
            let rejected: [[String]] = [
                [], ["--test-plan", "CloudAgent"], ["--platform", platform],
                ["--platform", "", "--test-plan", "CloudAgent"],
                ["--platform", platform, "--test-plan", ""],
                ["--platform", platform, "--test-plan"], ["--test-plan", "CloudAgent", "--platform"],
                base + ["--config"], base + ["--config", ""],
                base + ["--config", "Release"], base + ["--config", "Profile"],
                base + ["--config", "AppStore"], base + ["--config", "debug"],
                base + ["--platform", platform], base + ["--test-plan", "CloudAgent"],
                base + ["--test-plan", "UI"], base + ["--config", "Debug", "--config", "Debug"],
                base + ["--config", "Debug", "--config", "Release"],
                ["--platform", platform, "--test-plan", "UI"],
                ["--platform", kind == "mac" ? "ios-sim" : "mac", "--test-plan", "CloudAgent", "--config", "Debug"],
                ["--platform", "iphone", "--test-plan", "CloudAgent"],
                ["--platform", platform, "-testPlan", "CloudAgent"],
                ["--platform", platform, "--test-plan=CloudAgent"],
                ["--platform=" + platform, "--test-plan=CloudAgent"],
                base + ["--config=Debug"], base + ["--scheme", "Meguri"],
                base + ["-scheme", "Meguri"], base + ["--configuration", "Debug"],
                base + ["-configuration", "Debug"], base + ["--signing", "team"],
                base + ["--preview"], base + ["--personal-preview"], base + ["--standalone"],
                base + ["--preview", "--standalone"],
                base + ["--caller-option", "value"], base + ["caller", "argument"],
                base + ["MEGURI_TEST_PLAN=CloudAgent"], base + ["--platform", "--config"]]
            for args in rejected {
                let packet = try Self.packet(kind, edit: { $0.buildReceipt["args"] = args })
                try withRun { run in
                    try run.write(packet)
                    #expect(throws: AgentFailure.binding) { try run.admit(packet) }
                }
            }
        }
    }
    @Test func deploymentDeviceParentAndReferencesRefuse() throws {
        for (key, value) in [("job", "build-app"), ("run_id", "20320405T060708Z-99"), ("source_commit", String(repeating: "f", count: 40)),
                            ("parent_run", "20320405T060708Z-99"), ("device", "other-device"), ("device_kind", "iphone"), ("os_build", "other-os")] {
            try refuses(Self.packet(edit: { $0.deployment[key] = value }))
        }
        try refuses(Self.packet(edit: { $0.deployment["build_receipt"] = ["job": "build-app", "run_id": "20320405T060708Z-99"] }))
        try refuses(Self.packet(edit: { $0.deployment["installation"] = ["path": Self.installPath + "/installation.json", "sha256": Self.hash, "bytes": 1] }))
        try refuses(Self.packet("iphone", edit: { $0.deployment["installation"] = NSNull() }))
        for field in ["delivered_manifest", "xctestrun"] {
            try refuses(Self.packet(edit: { d in
                var ref = d.deployment[field] as! [String: Any]; ref["path"] = "/never-open/other.json"; d.deployment[field] = ref
            }))
        }
    }
    @Test func inventoryRejectsEscapesMissingEntriesCyclesExternalLinksAndWrongTags() throws {
        let edits: [([String: Any]) -> [String: Any]] = [
            { var e = $0; e["../escape"] = ["type": "directory", "mode": 493]; return e },
            { var e = $0; e["/absolute"] = ["type": "directory", "mode": 493]; return e },
            { var e = $0; e["Meguri.app/./alias"] = ["type": "directory", "mode": 493]; return e },
            { var e = $0; e["Meguri.app//alias"] = ["type": "directory", "mode": 493]; return e },
            { var e = $0; e["nul\0path"] = ["type": "directory", "mode": 493]; return e },
            { var e = $0; e.removeValue(forKey: "Meguri.app/Contents/MacOS/Meguri"); return e },
            { var e = $0; e.removeValue(forKey: "Meguri.app/Contents/MacOS"); return e },
            { var e = $0; e.removeValue(forKey: "CloudAgent.xctestrun"); return e },
            { var e = $0; e["bad"] = ["type": "symlink", "target": "missing"]; return e },
            { var e = $0; e["bad"] = ["type": "symlink", "target": "bad"]; return e },
            { var e = $0; e["bad"] = ["type": "symlink", "target": "second"]; e["second"] = ["type": "symlink", "target": "bad"]; return e },
            { var e = $0; e["bad"] = ["type": "symlink", "target": "/outside"]; return e },
            { var e = $0; e["bad"] = ["type": "symlink", "target": "../outside"]; return e },
            { var e = $0; e["bad"] = ["type": "socket"]; return e },
            { var e = $0; e["bad"] = ["type": "directory", "mode": 493, "mtime": 1]; return e }]
        for edit in edits {
            try refuses(Self.packet(edit: { d in
                var p = d.build["products"] as! [String: Any]; p["entries"] = edit(p["entries"] as! [String: Any]); d.build["products"] = p
            }))
        }
    }
    @Test func evidenceBoundDoesNotChangeDefaultJSONBound() throws {
        let packet = try Self.packet(editBuildBytes: { $0 + Data(repeating: 32, count: 1_048_576) })
        try withRun { run in
            try run.write(packet); _ = try run.admit(packet)
            #expect(throws: AgentFailure.json) { try AgentJSON.parse(packet.bytes[.buildManifest]!) }
            try run.write(Data(repeating: 32, count: 16 * 1024 * 1024 + 1), .buildManifest)
            #expect(throws: AgentFailure.storage) { try run.admit(packet) }
        }
    }
    @Test func realFileMutationAndSubstitutionAtReadBoundaryRefuse() throws {
        let packet = try Self.packet()
        for copy in Copy.allCases {
            try withRun { run in
                try run.write(packet)
                #expect(throws: AgentFailure.storage) {
                    try run.admit(packet, afterRead: { role in
                        if role == copy { try run.write(packet.bytes[copy]! + Data(" ".utf8), copy) }
                    })
                }
            }
            try withRun { run in
                try run.write(packet)
                #expect(throws: AgentFailure.storage) {
                    try run.admit(packet, afterRead: { role in
                        if role == copy {
                            let saved = run.directory.appendingPathComponent("original-" + copy.rawValue)
                            try FileManager.default.moveItem(at: run.url(copy), to: saved)
                            try run.write(packet.bytes[copy]!, copy)
                        }
                    })
                }
            }
        }
    }
    @Test func retainedAuthorityRejectsLaterChangesAndStaysInvalidated() throws {
        let packet = try Self.packet()
        for copy in Copy.allCases {
            try withRun { run in
                try run.write(packet); let binding = try run.admit(packet)
                guard chmod(run.url(copy).path, 0o600) == 0 else { throw AgentFailure.storage }
                #expect(throws: AgentFailure.storage) { try binding.revalidate() }
                guard chmod(run.url(copy).path, 0o644) == 0 else { throw AgentFailure.storage }
                #expect(throws: AgentFailure.storage) { try binding.revalidate() }
            }
        }
        try withRun { run in
            try run.write(packet); let binding = try run.admit(packet)
            try run.write(packet.bytes[.buildManifest]! + Data(" ".utf8), .buildManifest)
            #expect(throws: AgentFailure.storage) { try binding.revalidate() }
        }
    }
    @Test func directoriesAreBoundWithoutTimestampOrLinkCountPolicy() throws {
        let packet = try Self.packet()
        try withRun { run in
            try run.write(packet); let binding = try run.admit(packet)
            let unrelated = run.evidence.appendingPathComponent("unrelated-directory", isDirectory: true)
            guard mkdir(unrelated.path, 0o755) == 0 else { throw AgentFailure.storage }
            try binding.revalidate()
            try FileManager.default.removeItem(at: unrelated)
            try binding.revalidate()
        }
        for duringRead in [false, true] {
            try withRun { run in
                try run.write(packet)
                let substitute = {
                    let saved = run.directory.appendingPathComponent("saved-evidence", isDirectory: true)
                    try FileManager.default.moveItem(at: run.evidence, to: saved)
                    guard mkdir(run.evidence.path, 0o755) == 0 else { throw AgentFailure.storage }
                    try run.write(packet)
                }
                if duringRead {
                    #expect(throws: AgentFailure.storage) {
                        try run.admit(packet, afterRead: { if $0 == .buildReceipt { try substitute() } })
                    }
                } else {
                    let binding = try run.admit(packet); try substitute()
                    #expect(throws: AgentFailure.storage) { try binding.revalidate() }
                }
            }
        }
        try withRun { run in
            try run.write(packet); let binding = try run.admit(packet)
            let saved = run.directory.appendingPathComponent("saved-evidence", isDirectory: true)
            try FileManager.default.moveItem(at: run.evidence, to: saved)
            try FileManager.default.moveItem(at: saved, to: run.evidence)
            #expect(throws: AgentFailure.storage) { try binding.revalidate() }
        }
    }
    @Test func linksSpecialFilesAndOtherWritersAreRejected() throws {
        let packet = try Self.packet()
        for kind in ["symlink", "hardlink", "fifo", "writable"] {
            try withRun { run in
                try run.write(packet)
                let path = run.url(.buildReceipt).path
                if kind == "writable" { guard chmod(path, 0o666) == 0 else { throw AgentFailure.storage } }
                else {
                    let saved = run.directory.appendingPathComponent("saved.json")
                    try FileManager.default.moveItem(at: run.url(.buildReceipt), to: saved)
                    let result: Int32
                    switch kind {
                    case "symlink": result = symlink(saved.path, path)
                    case "hardlink": result = link(saved.path, path)
                    default: result = mkfifo(path, 0o644)
                    }
                    guard result == 0 else { throw AgentFailure.storage }
                }
                #expect(throws: AgentFailure.storage) { try run.admit(packet) }
            }
        }
    }
}
