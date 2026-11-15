import Darwin
import Foundation

// J2.1 receipt transport only. No host pathname in these models is opened.
// The binding holds handles for the four admitted evidence files.
// Callers keep this non-Sendable binding and its mutable FD authority thread-confined.
nonisolated final class AgentProductReceiptBinding {
    enum Copy: String, CaseIterable {
        case buildReceipt = "build-result.json", buildManifest = "agent-build.json"
        case deploymentReceipt = "deployment-result.json", deploymentManifest = "agent-deployment.json"
        var relativePath: String { "product-evidence/" + rawValue }
    }
    enum DeferredObservation: String, CaseIterable {
        case sourceCleanThroughFreeze, signaturesProvisioningGrantsAndConfiguration
        case cloudAgentPlanAndXctestrunBytes, destinationVMAndSimulatorLease
        case actualDeliveredInventory, simulatorInstallation, actualOSAndLoadedImages
        case completeSignedLane
    }
    let header: AgentHeader
    let originalHeaderBytes: Data
    let buildReceipt: AgentReceiptEnvelopeBinding
    let deploymentReceipt: AgentReceiptEnvelopeBinding
    let build: AgentBuildManifestBinding
    let deployment: AgentDeploymentManifestBinding
    let originalBytes: [Copy: Data]
    var deferredObservations: Set<DeferredObservation> { Set(DeferredObservation.allCases) }
    private let authority: AgentReceiptCopyAuthority

    // A deterministic real-file mutation boundary; never used as an inspector.
    init(admittedRunDirectory root: URL, originalHeaderBytes: Data,
         afterRead: ((Copy) throws -> Void)? = nil) throws {
        let header = try AgentHeader.read(originalHeaderBytes)
        guard header.build_receipt.job == "build-app",
              header.deployment_receipt.job == "cloudkit-agent-install" else { throw AgentFailure.binding }
        let authority = try AgentReceiptCopyAuthority(root: root)
        var bytes: [Copy: Data] = [:]
        for role in Copy.allCases {
            bytes[role] = try authority.retainFile(role.rawValue)
            do { try afterRead?(role) } catch { throw AgentFailure.storage }
            try authority.revalidate()
        }
        let buildReceipt = try AgentReceiptEnvelopeBinding(bytes: bytes[.buildReceipt]!, expected: header.build_receipt,
                                                          commit: header.source_commit)
        let deploymentReceipt = try AgentReceiptEnvelopeBinding(bytes: bytes[.deploymentReceipt]!, expected: header.deployment_receipt,
                                                               commit: header.source_commit)
        guard AgentJSON.hash(bytes[.buildReceipt]!) == header.build_receipt_sha256,
              buildReceipt.root == deploymentReceipt.root else { throw AgentFailure.binding }
        let buildRef = try buildReceipt.manifestReference("agent_build", filename: "agent-build.json")
        let deploymentRef = try deploymentReceipt.manifestReference("agent_deployment", filename: "agent-deployment.json")
        try buildRef.requireBytes(bytes[.buildManifest]!)
        try deploymentRef.requireBytes(bytes[.deploymentManifest]!)
        let build = try AgentBuildManifestBinding(bytes: bytes[.buildManifest]!, receipt: buildReceipt, header: header)
        let deployment = try AgentDeploymentManifestBinding(bytes: bytes[.deploymentManifest]!, receipt: deploymentReceipt,
            header: header, headerBytes: originalHeaderBytes, build: build, buildReference: buildRef)
        try authority.revalidate()
        self.header = header; self.originalHeaderBytes = originalHeaderBytes
        self.buildReceipt = buildReceipt; self.deploymentReceipt = deploymentReceipt
        self.build = build; self.deployment = deployment; self.originalBytes = bytes; self.authority = authority
    }

    // Explicit lifetime boundary. Failure permanently invalidates the retained copies.
    func revalidate() throws { try authority.revalidate() }
}

nonisolated struct AgentReceiptFileReference: Equatable {
    let path: String, sha256: String
    let bytes: UInt64
    fileprivate init(_ value: AgentJSON.Value, absolute: Bool) throws {
        let f = try value.record("path sha256 bytes")
        path = try f.text("path"); sha256 = try f.text("sha256"); bytes = try f.integer("bytes")
        guard ReceiptBindingSchema.path(path, absolute: absolute), AgentJSON.digest(sha256) else { throw AgentFailure.shape }
    }
    fileprivate func requireBytes(_ original: Data) throws {
        guard bytes == UInt64(original.count), sha256 == AgentJSON.hash(original) else { throw AgentFailure.binding }
    }
}

// Full validated MG metadata, including arbitrary schema-permitted details and
// summary extensions, remains available without reserializing original bytes.
nonisolated struct AgentReceiptEnvelopeBinding {
    let job: String, runID: String, root: String, sourceCommit: String
    let args: [String]
    let artifacts: [Artifact]
    let details: [String: AgentJSON.Value]
    let fields: [String: AgentJSON.Value]
    struct Artifact { let kind: String, path: String, label: String? }

    init(bytes: Data, expected: AgentHeader.Receipt, commit: String) throws {
        let f = try AgentJSON.parseEvidence(bytes).record("schema_version job run_id status exit_code placement service dry_run args root started_at finished_at duration_ms commit message summary failures artifacts user_steps details")
        guard try f.integer("schema_version") == 1 else { throw AgentFailure.version }
        job = try f.text("job"); runID = try f.text("run_id"); root = try f.text("root")
        guard AgentHeader.Receipt.validRunID(runID), ReceiptBindingSchema.path(root, absolute: true) else { throw AgentFailure.shape }
        let status = try f.text("status"), exit = try f.integer("exit_code")
        guard ["pass", "warn", "fail", "error"].contains(status), exit <= 2,
              ["host", "docker", "none"].contains(try f.text("placement")) else { throw AgentFailure.shape }
        _ = try f.nullableText("service"); _ = try f.text("message"); _ = try f.integer("duration_ms")
        args = try f.list("args").map { try $0.text() }
        let start = try ReceiptBindingSchema.time(f.text("started_at"))
        let finish = try ReceiptBindingSchema.time(f.text("finished_at"))
        guard start <= finish else { throw AgentFailure.binding }
        let c = try f.value("commit").record("sha dirty")
        let sha = try c.nullableText("sha"), dirty = try c.nullableBool("dirty")
        if let sha { guard ReceiptBindingSchema.commit(sha) else { throw AgentFailure.shape } }
        let summary = try f.value("summary").dictionary()
        guard Set(["total", "passed", "failed", "skipped"]).isSubset(of: Set(summary.keys)) else { throw AgentFailure.shape }
        for number in summary.values { _ = try number.integer() }
        let failures = try f.list("failures")
        for failure in failures {
            let item = try failure.record("id message repro")
            guard try !item.text("id").isEmpty, try !item.text("repro").isEmpty else { throw AgentFailure.shape }
            _ = try item.text("message")
        }
        artifacts = try f.list("artifacts").map { value in
            let item = try value.record("kind path", optional: "label")
            let kind = try item.text("kind"), path = try item.text("path")
            guard !kind.isEmpty, path.hasPrefix("/") else { throw AgentFailure.shape }
            return Artifact(kind: kind, path: path, label: try item["label"].map { try $0.text() })
        }
        let steps = try f.list("user_steps")
        for step in steps {
            let item = try step.record("id state required title how doc")
            guard ReceiptBindingSchema.matches(try item.text("id"), #"^[a-z0-9-]+$"#),
                  ["missing", "unverified"].contains(try item.text("state")) else { throw AgentFailure.shape }
            _ = try item.bool("required")
            for key in ["title", "how", "doc"] { _ = try item.text(key) }
        }
        details = try f.value("details").dictionary()
        try ReceiptBindingSchema.refusePending(.object(details))
        guard job == expected.job, runID == expected.run_id, sha == commit, dirty == false,
              try !f.bool("dry_run"), status == "pass", exit == 0,
              try summary.integer("failed") == 0, failures.isEmpty,
              !steps.contains(where: { (try? $0.dictionary().bool("required")) == true }) else { throw AgentFailure.binding }
        sourceCommit = commit; fields = f
    }
    func requireArtifact(_ path: String, kind: String? = nil) throws {
        let declarations = artifacts.filter { $0.path == path }
        guard ReceiptBindingSchema.hostBuildPath(path, root: root),
              declarations.count == 1, let declaration = declarations.first,
              kind == nil || declaration.kind == kind else { throw AgentFailure.binding }
    }
    fileprivate func manifestReference(_ key: String, filename: String) throws -> AgentReceiptFileReference {
        let reference = try AgentReceiptFileReference(details.value(key), absolute: true)
        guard reference.path.hasSuffix("/" + filename) else { throw AgentFailure.binding }
        try requireArtifact(reference.path, kind: "json")
        return reference
    }
}

nonisolated struct AgentBundleReceiptBinding {
    let path: String, bundleID: String, executable: String, executableSHA256: String
    fileprivate init(_ value: AgentJSON.Value) throws {
        let f = try value.record("path bundle_id executable executable_sha256")
        path = try f.text("path"); bundleID = try f.text("bundle_id")
        executable = try f.text("executable"); executableSHA256 = try f.text("executable_sha256")
        guard ReceiptBindingSchema.path(path, absolute: false), ReceiptBindingSchema.path(executable, absolute: false),
              !bundleID.isEmpty, AgentJSON.digest(executableSHA256) else { throw AgentFailure.shape }
    }
}

nonisolated struct AgentProductsInventoryBinding {
    enum Entry {
        case file(sha256: String, bytes: UInt64, mode: UInt16)
        case directory(mode: UInt16)
        case symlink(target: String)
    }
    let path: String
    let entries: [String: Entry]
    fileprivate init(_ value: AgentJSON.Value) throws {
        let f = try value.record("path entries")
        path = try f.text("path")
        guard ReceiptBindingSchema.path(path, absolute: true) else { throw AgentFailure.shape }
        var inventory: [String: Entry] = [:]
        for (key, value) in try f.value("entries").dictionary() {
            guard ReceiptBindingSchema.path(key, absolute: false) else { throw AgentFailure.shape }
            let raw = try value.dictionary()
            switch try raw.text("type") {
            case "file":
                let item = try value.record("type sha256 bytes mode")
                let hash = try item.text("sha256"), bytes = try item.integer("bytes"), mode = try item.integer("mode")
                guard AgentJSON.digest(hash), mode <= 0o7777 else { throw AgentFailure.shape }
                inventory[key] = .file(sha256: hash, bytes: bytes, mode: UInt16(mode))
            case "directory":
                let item = try value.record("type mode"), mode = try item.integer("mode")
                guard mode <= 0o7777 else { throw AgentFailure.shape }
                inventory[key] = .directory(mode: UInt16(mode))
            case "symlink":
                let item = try value.record("type target"), target = try item.text("target")
                // Normal '..' is allowed only in link targets, resolved below.
                guard !target.isEmpty, !target.hasPrefix("/"), !target.utf8.contains(0),
                      target.split(separator: "/", omittingEmptySubsequences: false).allSatisfy({ !$0.isEmpty }) else { throw AgentFailure.shape }
                inventory[key] = .symlink(target: target)
            default: throw AgentFailure.shape
            }
        }
        entries = inventory
        // Resolve every path, including every link and implicit parent. A file
        // cannot be traversed as a directory; no missing parent is invented.
        for key in entries.keys { _ = try resolved(key) }
    }
    func resolved(_ relative: String) throws -> String {
        guard ReceiptBindingSchema.path(relative, absolute: false) else { throw AgentFailure.shape }
        var pending = relative.split(separator: "/").map(String.init), stack: [String] = []
        var expansions = 0
        while !pending.isEmpty {
            let component = pending.removeFirst()
            if component == "." { continue }
            if component == ".." {
                guard !stack.isEmpty else { throw AgentFailure.binding }
                stack.removeLast(); continue
            }
            let key = (stack + [component]).joined(separator: "/")
            guard let entry = entries[key] else { throw AgentFailure.binding }
            switch entry {
            case .symlink(let target):
                expansions += 1
                guard expansions <= 128 else { throw AgentFailure.binding }
                pending = target.split(separator: "/", omittingEmptySubsequences: false).map(String.init) + pending
            case .directory:
                stack.append(component)
            case .file:
                guard pending.isEmpty else { throw AgentFailure.binding }
                stack.append(component)
            }
        }
        let key = stack.joined(separator: "/")
        guard entries[key] != nil else { throw AgentFailure.binding }
        return key
    }
    fileprivate func requireDirectory(_ path: String) throws {
        guard case .directory? = entries[try resolved(path)] else { throw AgentFailure.binding }
    }
    fileprivate func requireFile(_ path: String, sha256: String, bytes: UInt64? = nil, executable: Bool = false) throws {
        guard case .file(let hash, let count, let mode)? = entries[try resolved(path)], hash == sha256,
              bytes == nil || count == bytes, !executable || mode & 0o111 != 0 else { throw AgentFailure.binding }
    }
}

nonisolated struct AgentBuildManifestBinding {
    let job: String, runID: String, sourceCommit: String
    let scheme: String, configuration: String, testPlan: String, platform: String, signing: String, appBuild: String
    let app: AgentBundleReceiptBinding, agent: AgentBundleReceiptBinding
    let xctestrun: AgentReceiptFileReference
    let products: AgentProductsInventoryBinding
    fileprivate init(bytes: Data, receipt: AgentReceiptEnvelopeBinding, header: AgentHeader) throws {
        let f = try AgentJSON.parseEvidence(bytes).record("protocol_version job run_id source_commit scheme configuration test_plan platform signing app_build app agent xctestrun products")
        guard try f.integer("protocol_version") == 1 else { throw AgentFailure.version }
        job = try f.text("job"); runID = try f.text("run_id"); sourceCommit = try f.text("source_commit")
        scheme = try f.text("scheme"); configuration = try f.text("configuration"); testPlan = try f.text("test_plan")
        platform = try f.text("platform"); signing = try f.text("signing"); appBuild = try f.text("app_build")
        app = try AgentBundleReceiptBinding(f.value("app")); agent = try AgentBundleReceiptBinding(f.value("agent"))
        xctestrun = try AgentReceiptFileReference(f.value("xctestrun"), absolute: false)
        products = try AgentProductsInventoryBinding(f.value("products"))
        guard job == receipt.job, runID == receipt.runID, sourceCommit == receipt.sourceCommit,
              scheme == "Meguri", configuration == "Debug", testPlan == "CloudAgent", signing == "team",
              platform == (header.device_kind == "mac" ? "mac" : "ios-sim"), appBuild == header.app_build,
              app.executableSHA256 == header.executable_sha256,
              app.path.hasSuffix(".app"), agent.path.hasSuffix(".xctest"),
              agent.path.hasPrefix(app.path + "/"), app.bundleID != agent.bundleID,
              xctestrun.path.hasSuffix(".xctestrun") else { throw AgentFailure.binding }
        try ReceiptBindingSchema.explicitPlan(receipt.args, platform: platform)
        try receipt.requireArtifact(products.path)
        try products.requireDirectory(app.path); try products.requireDirectory(agent.path)
        try products.requireFile(app.path + "/" + app.executable, sha256: app.executableSHA256, executable: true)
        try products.requireFile(agent.path + "/" + agent.executable, sha256: agent.executableSHA256, executable: true)
        try products.requireFile(xctestrun.path, sha256: xctestrun.sha256, bytes: xctestrun.bytes)
    }
}

nonisolated struct AgentDeploymentManifestBinding {
    let job: String, runID: String, sourceCommit: String, parentRun: String
    let device: String, deviceKind: String, osBuild: String, headerSHA256: String
    let buildReceipt: AgentHeader.Receipt, buildReceiptSHA256: String
    let buildManifest: AgentReceiptFileReference, deliveredManifest: AgentReceiptFileReference
    let xctestrun: AgentReceiptFileReference, installation: AgentReceiptFileReference?
    fileprivate init(bytes: Data, receipt: AgentReceiptEnvelopeBinding, header: AgentHeader, headerBytes: Data,
                     build: AgentBuildManifestBinding, buildReference: AgentReceiptFileReference) throws {
        let f = try AgentJSON.parseEvidence(bytes).record("protocol_version job run_id source_commit parent_run device device_kind os_build header_sha256 build_receipt build_receipt_sha256 build_manifest delivered_manifest xctestrun installation")
        guard try f.integer("protocol_version") == 1 else { throw AgentFailure.version }
        job = try f.text("job"); runID = try f.text("run_id"); sourceCommit = try f.text("source_commit")
        parentRun = try f.text("parent_run"); device = try f.text("device"); deviceKind = try f.text("device_kind")
        osBuild = try f.text("os_build"); headerSHA256 = try f.text("header_sha256")
        let r = try f.value("build_receipt").record("job run_id")
        buildReceipt = AgentHeader.Receipt(job: try r.text("job"), run_id: try r.text("run_id"))
        buildReceiptSHA256 = try f.text("build_receipt_sha256")
        buildManifest = try AgentReceiptFileReference(f.value("build_manifest"), absolute: true)
        deliveredManifest = try AgentReceiptFileReference(f.value("delivered_manifest"), absolute: true)
        xctestrun = try AgentReceiptFileReference(f.value("xctestrun"), absolute: true)
        if case .null = try f.value("installation") { installation = nil }
        else { installation = try AgentReceiptFileReference(f.value("installation"), absolute: true) }
        guard job == receipt.job, runID == receipt.runID, sourceCommit == receipt.sourceCommit,
              parentRun == header.run_id, device == header.device, deviceKind == header.device_kind,
              osBuild == header.os_build, headerSHA256 == AgentJSON.hash(headerBytes),
              buildReceipt == header.build_receipt, buildReceiptSHA256 == header.build_receipt_sha256,
              buildManifest == buildReference, (deviceKind == "mac") == (installation == nil) else { throw AgentFailure.binding }
        guard deliveredManifest.path != buildManifest.path, xctestrun.path.hasSuffix(".xctestrun"),
              installation?.path != deliveredManifest.path else { throw AgentFailure.binding }
        try receipt.requireArtifact(deliveredManifest.path, kind: "json")
        try receipt.requireArtifact(xctestrun.path)
        if let installation { try receipt.requireArtifact(installation.path, kind: "json") }
        // The header's artifact is retained as a reference; its exact bytes were
        // supplied by the caller, never opened using the artifact's host path.
        guard receipt.artifacts.contains(where: {
            $0.kind == "json" && ReceiptBindingSchema.hostBuildPath($0.path, root: receipt.root) && $0.path.hasSuffix("/run.json")
        }) else { throw AgentFailure.binding }
        // An unchanged template must retain its exact hash/count. A separately
        // declared derived xctestrun has deferred routing/selection obligations.
        if xctestrun.path == build.products.path + "/" + build.xctestrun.path {
            guard xctestrun.sha256 == build.xctestrun.sha256, xctestrun.bytes == build.xctestrun.bytes else { throw AgentFailure.binding }
        }
    }
}

private nonisolated enum ReceiptBindingSchema {
    static func matches(_ text: String, _ pattern: String) -> Bool {
        text.range(of: pattern, options: .regularExpression) == (text.startIndex..<text.endIndex)
    }
    static func commit(_ text: String) -> Bool { matches(text, "^[0-9a-f]{40}$") }
    static func path(_ path: String, absolute: Bool) -> Bool { AgentHeader.FixtureDelivery.validPath(path, absolute: absolute) }
    static func hostBuildPath(_ path: String, root: String) -> Bool {
        self.path(path, absolute: true) && path.hasPrefix(root + "/build/")
    }
    static func time(_ value: String) throws -> Date {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = formatter.date(from: value) ?? ISO8601DateFormatter().date(from: value)
        guard let date else { throw AgentFailure.shape }
        return date
    }
    static func explicitPlan(_ args: [String], platform: String) throws {
        guard args.count.isMultiple(of: 2) else { throw AgentFailure.binding }
        var fields: [String: String] = [:]
        for index in stride(from: 0, to: args.count, by: 2) {
            let key = args[index]
            guard ["--platform", "--test-plan", "--config"].contains(key),
                  fields[key] == nil else { throw AgentFailure.binding }
            fields[key] = args[index + 1]
        }
        guard fields["--platform"] == platform, fields["--test-plan"] == "CloudAgent",
              (fields["--config"] ?? "Debug") == "Debug" else { throw AgentFailure.binding }
    }
    static func refusePending(_ value: AgentJSON.Value) throws {
        switch value {
        case .object(let f):
            for (key, v) in f {
                if ["status", "state", "outcome"].contains(key), case .string(let text) = v,
                   ["queued", "awaiting_user", "awaiting-user", "pending", "disabled", "unimplemented", "implementation_missing"].contains(text) { throw AgentFailure.binding }
                if ["queued", "awaiting_user", "awaiting-user", "pending", "disabled", "unimplemented", "implementation_missing"].contains(key),
                   case .boolean(true) = v { throw AgentFailure.binding }
                try refusePending(v)
            }
        case .array(let items): for item in items { try refusePending(item) }
        default: break
        }
    }
}

extension AgentJSON.Value {
    nonisolated func dictionary() throws -> [String: AgentJSON.Value] {
        guard case .object(let f) = self else { throw AgentFailure.shape }; return f
    }
    nonisolated func record(_ required: String, optional: String = "") throws -> [String: AgentJSON.Value] {
        try object(required: Set(required.split(separator: " ").map(String.init)), optional: Set(optional.split(separator: " ").map(String.init)))
    }
    nonisolated func text() throws -> String {
        guard case .string(let text) = self else { throw AgentFailure.shape }; return text
    }
    nonisolated func integer() throws -> UInt64 {
        guard case .number(let token) = self, !token.isEmpty,
              token.utf8.allSatisfy({ (48...57).contains($0) }), let number = UInt64(token) else { throw AgentFailure.shape }
        return number
    }
}
extension Dictionary where Key == String, Value == AgentJSON.Value {
    nonisolated func value(_ key: String) throws -> Value { guard let v = self[key] else { throw AgentFailure.shape }; return v }
    nonisolated func text(_ key: String) throws -> String { try value(key).text() }
    nonisolated func integer(_ key: String) throws -> UInt64 { try value(key).integer() }
    nonisolated func bool(_ key: String) throws -> Bool {
        guard case .boolean(let b) = try value(key) else { throw AgentFailure.shape }; return b
    }
    fileprivate nonisolated func nullableText(_ key: String) throws -> String? {
        if case .null = try value(key) { return nil }; return try text(key)
    }
    fileprivate nonisolated func nullableBool(_ key: String) throws -> Bool? {
        if case .null = try value(key) { return nil }; return try bool(key)
    }
    nonisolated func list(_ key: String) throws -> [Value] {
        guard case .array(let items) = try value(key) else { throw AgentFailure.shape }; return items
    }
}

// Same stable authority pattern as AgentExpectedAccounts, with public-copy
// owner/no-other-writer rules rather than that private file's 0700/0600 policy.
private nonisolated final class AgentReceiptCopyAuthority {
    private nonisolated final class Descriptor {
        let value: Int32
        init(_ value: Int32) throws { guard value >= 0 else { throw AgentFailure.storage }; self.value = value }
        deinit { _ = Darwin.close(value) }
    }
    private struct DirectoryIdentity: Equatable {
        let device: dev_t, inode: ino_t, owner: uid_t, group: gid_t, mode: mode_t
        init(_ s: stat) { device = s.st_dev; inode = s.st_ino; owner = s.st_uid; group = s.st_gid; mode = s.st_mode }
    }
    private struct Directory { let fd: Descriptor, name: String, identity: DirectoryIdentity }
    private struct File { let fd: Descriptor, name: String, info: stat, bytes: Data }
    private let events: Descriptor
    private var directories: [Directory] = []
    private var files: [File] = []
    private var invalidated = false
    private static let limit = 16 * 1024 * 1024

    init(root: URL) throws {
        events = try Descriptor(kqueue())
        guard fcntl(events.value, F_SETFD, FD_CLOEXEC) == 0,
              root.isFileURL, root.host == nil || root.host == "" || root.host == "localhost",
              root.path.utf8.count <= 4096, root.pathComponents.count <= 128,
              ReceiptBindingSchema.path(root.path, absolute: true) else { throw AgentFailure.storage }
        try retainDirectory("/", owned: false)
        for (index, name) in root.pathComponents.dropFirst().enumerated() {
            try retainDirectory(name, owned: index == root.pathComponents.count - 2)
        }
        try retainDirectory("product-evidence", owned: true)
        try revalidate()
    }
    private func retainDirectory(_ name: String, owned: Bool) throws {
        let fd = try Descriptor(directories.last.map {
            openat($0.fd.value, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        } ?? open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK))
        var info = stat()
        guard fstat(fd.value, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
              !owned || (info.st_uid == getuid() && info.st_mode & 0o022 == 0) else { throw AgentFailure.storage }
        var event = kevent(ident: UInt(fd.value), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                           fflags: UInt32(NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE), data: 0, udata: nil)
        guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
        directories.append(Directory(fd: fd, name: name, identity: DirectoryIdentity(info)))
    }
    func retainFile(_ name: String) throws -> Data {
        // Only the caller's compiled four role names reach this method.
        guard AgentProductReceiptBinding.Copy.allCases.contains(where: { $0.rawValue == name }) else { throw AgentFailure.storage }
        let parent = directories.last!.fd.value
        var named = stat(), info = stat()
        guard fstatat(parent, name, &named, AT_SYMLINK_NOFOLLOW) == 0 else { throw AgentFailure.storage }
        try Self.regular(named)
        let fd = try Descriptor(openat(parent, name, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK))
        guard fstat(fd.value, &info) == 0, Self.same(named, info) else { throw AgentFailure.storage }
        try Self.regular(info)
        var event = kevent(ident: UInt(fd.value), filter: Int16(EVFILT_VNODE), flags: UInt16(EV_ADD | EV_CLEAR),
                           fflags: UInt32(NOTE_WRITE | NOTE_EXTEND | NOTE_ATTRIB | NOTE_LINK | NOTE_RENAME | NOTE_DELETE | NOTE_REVOKE),
                           data: 0, udata: nil)
        guard kevent(events.value, &event, 1, nil, 0, nil) == 0 else { throw AgentFailure.storage }
        let bytes = try Self.read(fd.value, size: info.st_size)
        files.append(File(fd: fd, name: name, info: info, bytes: bytes))
        try revalidate()
        return bytes
    }
    func revalidate() throws {
        do {
            guard !invalidated else { throw AgentFailure.storage }
            try checkDirectories()
            for file in files {
                try check(file)
                guard try Self.read(file.fd.value, size: file.info.st_size) == file.bytes else { throw AgentFailure.storage }
                try check(file)
            }
            try checkDirectories()
        } catch { invalidated = true; throw AgentFailure.storage }
    }
    private func checkDirectories() throws {
        for (index, directory) in directories.enumerated() {
            var held = stat(), named = stat()
            guard fstat(directory.fd.value, &held) == 0, DirectoryIdentity(held) == directory.identity else { throw AgentFailure.storage }
            if index > 0 {
                guard fstatat(directories[index - 1].fd.value, directory.name, &named, AT_SYMLINK_NOFOLLOW) == 0,
                      DirectoryIdentity(named) == directory.identity else { throw AgentFailure.storage }
            } else {
                guard lstat("/", &named) == 0, DirectoryIdentity(named) == directory.identity else { throw AgentFailure.storage }
            }
        }
        var event = kevent(), timeout = timespec(tv_sec: 0, tv_nsec: 0)
        guard kevent(events.value, nil, 0, &event, 1, &timeout) == 0 else { throw AgentFailure.storage }
    }
    private func check(_ file: File) throws {
        var held = stat(), named = stat()
        guard fstat(file.fd.value, &held) == 0,
              fstatat(directories.last!.fd.value, file.name, &named, AT_SYMLINK_NOFOLLOW) == 0,
              Self.same(file.info, held), Self.same(file.info, named) else { throw AgentFailure.storage }
        try Self.regular(held); try Self.regular(named)
    }
    private static func regular(_ s: stat) throws {
        guard s.st_mode & S_IFMT == S_IFREG, s.st_uid == getuid(), s.st_nlink == 1, s.st_mode & 0o022 == 0,
              s.st_size >= 0, s.st_size <= limit else { throw AgentFailure.storage }
    }
    private static func same(_ a: stat, _ b: stat) -> Bool {
        DirectoryIdentity(a) == DirectoryIdentity(b) && a.st_nlink == b.st_nlink && a.st_size == b.st_size &&
        a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec &&
        a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec
    }
    private static func read(_ fd: Int32, size: off_t) throws -> Data {
        var bytes = Data(), buffer = [UInt8](repeating: 0, count: 16_384)
        while true {
            let count = buffer.withUnsafeMutableBytes { pread(fd, $0.baseAddress, $0.count, off_t(bytes.count)) }
            guard count >= 0, count <= limit - bytes.count else { throw AgentFailure.storage }
            if count == 0 { break }
            bytes.append(contentsOf: buffer.prefix(count))
        }
        guard bytes.count == size else { throw AgentFailure.storage }; return bytes
    }
}
