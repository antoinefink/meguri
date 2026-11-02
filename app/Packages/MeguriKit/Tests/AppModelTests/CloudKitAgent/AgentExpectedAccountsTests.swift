import Darwin
import Foundation
import Testing

@MainActor @Suite(.serialized) struct AgentExpectedAccountsTests {
    // All values are generated test data; no account observation is involved.
    private static let identity: [UInt8] = [0, 17, 65, 90, 97, 255]
    private static var header: Data { CloudKitAgentProtocolTests.header }

    private final class Run {
        let directory: URL
        let support: URL
        let privateDirectories: [URL]
        let file: URL

        init() throws {
            // Canonicalize only the trusted OS fixture root, before creating owned paths.
            guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            let temporaryRoot = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
            directory = temporaryRoot
                .appendingPathComponent("meguri-expected-accounts-" + UUID().uuidString, isDirectory: true)
            support = directory.appendingPathComponent("Application Support", isDirectory: true)
            var path = support
            let parsed = try AgentHeader.read(AgentExpectedAccountsTests.header)
            privateDirectories = ["Meguri", "CloudKitSignedPrivate", parsed.run_id, parsed.device].map {
                path = path.appendingPathComponent($0, isDirectory: true)
                return path
            }
            file = path.appendingPathComponent("expected-accounts.json")
            guard mkdir(directory.path, 0o700) == 0 else { throw AgentFailure.storage }
            do {
                for url in [directory, support] + privateDirectories {
                    if url != directory, mkdir(url.path, 0o700) != 0 { throw AgentFailure.storage }
                    guard chmod(url.path, 0o700) == 0 else { throw AgentFailure.storage }
                }
            } catch {
                try? FileManager.default.removeItem(at: directory)
                throw error
            }
        }

        func write(_ data: Data) throws {
            try data.write(to: file)
            guard chmod(file.path, 0o600) == 0 else { throw AgentFailure.storage }
        }
        func admit(header: Data = AgentExpectedAccountsTests.header,
                   afterRead: (() throws -> Void)? = nil) throws -> AgentExpectedAccounts {
            try AgentExpectedAccounts(admittedApplicationSupportRoot: support,
                                      originalHeaderBytes: header, afterRead: afterRead)
        }
    }

    // Each test owns one canonical run directory. Every file, moved directory,
    // link and FIFO stays inside it, and cleanup never reaches outside it.
    private func withRun(_ body: (Run) throws -> Void) throws {
        let run = try Run()
        defer { try? FileManager.default.removeItem(at: run.directory) }
        try body(run)
    }
    private static func object() throws -> [String: Any] {
        let parsed = try AgentHeader.read(header)
        return ["protocol_version": 1, "header_sha256": AgentJSON.hash(header), "job": parsed.job,
                "run_id": parsed.run_id, "source_commit": parsed.source_commit, "device": parsed.device,
                "executable_sha256": parsed.executable_sha256, "container": parsed.container,
                "environment": parsed.environment, "account_scope": parsed.account_scope,
                "accounts": ["acct1": identity.map(Int.init)]]
    }
    private static func encode(_ object: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    }
    private static func input() throws -> Data { try encode(object()) }
    private static func replaced(_ bytes: Data, _ old: String, _ new: String) -> Data {
        Data(String(decoding: bytes, as: UTF8.self).replacingOccurrences(of: old, with: new).utf8)
    }

    @Test func admissionMatchesOnlyExactBytesAndExpectedFixedLabel() throws {
        try withRun { run in
            try run.write(Self.input())
            let expected = try run.admit()
            try expected.requireMatch(Self.identity, label: "acct1")
            try expected.revalidate()
            let nonmatching: [[UInt8]] = [[], [0, 17, 97, 90, 97, 255], [0, 17, 65, 90, 97], [255, 17, 65, 90, 97, 0]]
            for actual in nonmatching {
                #expect(throws: AgentFailure.wrongContainer) { try expected.requireMatch(actual, label: "acct1") }
            }
            for label in ["acct2", "ACCT1", "noAccount", "", "unapproved"] {
                #expect(throws: AgentFailure.wrongContainer) { try expected.requireMatch(Self.identity, label: label) }
            }
            // A comparison failure neither enrolls new bytes nor relabels them.
            try expected.requireMatch(Self.identity, label: "acct1")
        }
    }

    @Test func headerHashUsesOriginalBytesRatherThanEquivalentReserialization() throws {
        try withRun { run in
            try run.write(Self.input())
            let object = try #require(try JSONSerialization.jsonObject(with: Self.header) as? [String: Any])
            let reserialized = try Self.encode(object)
            #expect(reserialized != Self.header)
            #expect(try AgentHeader.read(reserialized) == AgentHeader.read(Self.header))
            #expect(throws: AgentFailure.binding) { try run.admit(header: reserialized) }
            var input = try Self.object()
            input["header_sha256"] = AgentJSON.hash(reserialized)
            try run.write(Self.encode(input))
            #expect(throws: AgentFailure.binding) { try run.admit() }
        }
    }

    @Test func everyProtectedHeaderBindingIsCheckedIndependently() throws {
        try withRun { run in
            for (key, value) in [
                ("header_sha256", String(repeating: "f", count: 64)), ("job", "other-job"),
                ("run_id", "20261102T000000Z-99"), ("source_commit", String(repeating: "f", count: 40)),
                ("device", "B"), ("executable_sha256", String(repeating: "f", count: 64)),
                ("container", "other-container"), ("environment", "production"), ("account_scope", "scopeB")
            ] {
                var input = try Self.object()
                input[key] = value
                try run.write(Self.encode(input))
                let failure: AgentFailure = ["container", "environment", "account_scope"].contains(key) ? .wrongContainer : .binding
                #expect(throws: failure) { try run.admit() }
            }
        }
    }

    @Test func headerIsValidatedBeforePathUseAndCannotSelectAnInputSource() throws {
        try withRun { run in
            try run.write(Self.input())
            let originalHeader = Self.header
            let originalObject = try #require(try JSONSerialization.jsonObject(with: originalHeader) as? [String: Any])
            for (key, value) in [("device", "../A"), ("run_id", "../escape"),
                                 ("device", "/A"), ("device", "A\u{0000}")] {
                var object = originalObject
                let originalValue = try #require(object[key] as? String)
                object[key] = value
                try #require(object[key] as? String != originalValue)
                let header = try Self.encode(object)
                try #require(header != originalHeader)
                let serializedObject = try #require(try JSONSerialization.jsonObject(with: header) as? [String: Any])
                try #require(serializedObject[key] as? String == value)
                #expect(throws: AgentFailure.shape) { try run.admit(header: header) }
            }
            var input = try Self.object()
            input["path"] = "expected-accounts.json"
            try run.write(Self.encode(input))
            #expect(throws: AgentFailure.shape) { try run.admit() }
            // A valid input at a different name never supplies missing setup.
            try FileManager.default.removeItem(at: run.file)
            try Self.input().write(to: run.support.appendingPathComponent("other.json"))
            #expect(throws: AgentFailure.awaitingUser) { try run.admit() }
        }
    }

    @Test func exactTopLevelAndNestedFieldsAndDuplicateKeysAreRequired() throws {
        try withRun { run in
            let original = try Self.object()
            for key in original.keys {
                var missing = original
                missing.removeValue(forKey: key)
                try run.write(Self.encode(missing))
                #expect(throws: AgentFailure.shape) { try run.admit() }
            }
            var extra = original
            extra["unknown"] = true
            try run.write(Self.encode(extra))
            #expect(throws: AgentFailure.shape) { try run.admit() }
            let invalidLabels: [[String: Any]] = [[:], ["acct2": [1]], ["acct1": [1], "acct2": [2]],
                                                ["ACCT1": [1]], ["acct1": [1], "other": [2]]]
            for labels in invalidLabels {
                var input = original
                input["accounts"] = labels
                try run.write(Self.encode(input))
                #expect(throws: AgentFailure.shape) { try run.admit() }
            }
            let valid = try Self.input()
            for bytes in [
                Self.replaced(valid, "\"job\":", "\"job\":\"cloudkit-signed\",\"job\":"),
                Self.replaced(valid, "\"job\":", "\"\\u006aob\":\"cloudkit-signed\",\"job\":"),
                Self.replaced(valid, "\"acct1\":", "\"acct1\":[1],\"acct1\":"),
                Self.replaced(valid, "\"acct1\":", "\"acct\\u0031\":[1],\"acct1\":")
            ] {
                try run.write(bytes)
                #expect(throws: AgentFailure.json) { try run.admit() }
            }
        }
    }

    @Test func byteArraysAndVersionRequireExactIntegerTokensAndBounds() throws {
        try withRun { run in
            let valid = try Self.input()
            for value in ["[]", "[true]", "[false]", "[1.0]", "[1e0]", "[-1]", "[-0]", "[256]",
                          "[99999999999999999999]", "[\"1\"]", "[null]", "[[1]]", "{}", "\"AQ==\""] {
                try run.write(Self.replaced(valid, "[0,17,65,90,97,255]", value))
                #expect(throws: AgentFailure.shape) { try run.admit() }
            }
            for value in ["true", "\"1\"", "1.0", "1e0", "-1", "null"] {
                try run.write(Self.replaced(valid, "\"protocol_version\":1", "\"protocol_version\":\(value)"))
                #expect(throws: AgentFailure.shape) { try run.admit() }
            }
            try run.write(Self.replaced(valid, "\"protocol_version\":1", "\"protocol_version\":2"))
            #expect(throws: AgentFailure.version) { try run.admit() }
            for value in ["[0]", "[255]", "[0,255]"] {
                try run.write(Self.replaced(valid, "[0,17,65,90,97,255]", value))
                _ = try run.admit()
            }
        }
    }

    @Test func invalidJSONAndParserByteLimitAreEnforcedBeforeAdmission() throws {
        try withRun { run in
            for bytes in [Data(), Data([255]), Data("{\"x\":NaN}".utf8), Data("{\"x\":1e999}".utf8)] {
                try run.write(bytes)
                #expect(throws: AgentFailure.json) { try run.admit() }
            }
            let valid = try Self.input()
            var atLimit = valid
            atLimit.append(Data(repeating: 32, count: 1_048_576 - valid.count))
            try run.write(atLimit)
            try run.admit().requireMatch(Self.identity, label: "acct1")
            atLimit.append(32)
            try run.write(atLimit)
            #expect(throws: AgentFailure.storage) { try run.admit() }
        }
    }

    @Test func onlyGenuinelyMissingPrivateSetupAwaitsUser() throws {
        try withRun { run in
            #expect(throws: AgentFailure.awaitingUser) { try run.admit() }
            for directory in run.privateDirectories.reversed() {
                try FileManager.default.removeItem(at: directory)
                #expect(throws: AgentFailure.awaitingUser) { try run.admit() }
            }
            try FileManager.default.removeItem(at: run.support)
            #expect(throws: AgentFailure.storage) { try run.admit() }
        }
    }

    @Test func rootPrivateDirectoryAndFileModesAreEnforced() throws {
        try withRun { run in
            try run.write(Self.input())
            let rootModes: [mode_t] = [0o777, 0o720, 0o702]
            for mode in rootModes {
                guard chmod(run.support.path, mode) == 0 else { throw AgentFailure.storage }
                defer { _ = chmod(run.support.path, 0o700) }
                #expect(throws: AgentFailure.storage) { try run.admit() }
            }
            for directory in run.privateDirectories {
                let privateModes: [mode_t] = [0o755, 0o701, 0o1700, 0o500]
                for mode in privateModes {
                    guard chmod(directory.path, mode) == 0 else { throw AgentFailure.storage }
                    defer { _ = chmod(directory.path, 0o700) }
                    #expect(throws: AgentFailure.storage) { try run.admit() }
                }
            }
            let fileModes: [mode_t] = [0o644, 0o400, 0o700, 0o1600, 0o4600]
            for mode in fileModes {
                guard chmod(run.file.path, mode) == 0 else { throw AgentFailure.storage }
                defer { _ = chmod(run.file.path, 0o600) }
                #expect(throws: AgentFailure.storage) { try run.admit() }
            }
        }
    }

    @Test func symbolicLinksHardLinksFIFOAndNonDirectoriesAreInvalidAuthority() throws {
        try withRun { run in
            try run.write(Self.input())
            let saved = run.directory.appendingPathComponent("saved-input")
            try FileManager.default.moveItem(at: run.file, to: saved)
            guard symlink(saved.path, run.file.path) == 0 else { throw AgentFailure.storage }
            #expect(throws: AgentFailure.storage) { try run.admit() }
            try FileManager.default.removeItem(at: run.file)
            guard symlink("missing-input", run.file.path) == 0 else { throw AgentFailure.storage }
            #expect(throws: AgentFailure.storage) { try run.admit() }
            try FileManager.default.removeItem(at: run.file)
            guard link(saved.path, run.file.path) == 0 else { throw AgentFailure.storage }
            #expect(throws: AgentFailure.storage) { try run.admit() }
            try FileManager.default.removeItem(at: run.file)
            guard mkfifo(run.file.path, 0o600) == 0 else { throw AgentFailure.storage }
            #expect(throws: AgentFailure.storage) { try run.admit() }
            try FileManager.default.removeItem(at: run.file)
            try FileManager.default.moveItem(at: saved, to: run.file)
            for directory in [run.support] + run.privateDirectories {
                let moved = directory.appendingPathExtension("saved")
                try FileManager.default.moveItem(at: directory, to: moved)
                guard symlink(moved.path, directory.path) == 0 else { throw AgentFailure.storage }
                #expect(throws: AgentFailure.storage) { try run.admit() }
                try FileManager.default.removeItem(at: directory)
                try FileManager.default.moveItem(at: moved, to: directory)
            }
            let device = try #require(run.privateDirectories.last)
            try FileManager.default.removeItem(at: device)
            try Data([0]).write(to: device)
            #expect(throws: AgentFailure.storage) { try run.admit() }
        }
    }

    @Test func fileReplacementContentAndPermissionChangesDuringReadAreRejected() throws {
        try withRun { run in
            let valid = try Self.input()
            for mutation in 0..<6 {
                try run.write(valid)
                var mutationCompleted = false
                #expect(throws: AgentFailure.storage) {
                    try run.admit(afterRead: {
                        switch mutation {
                        case 0:
                            let moved = run.directory.appendingPathComponent("replaced-file")
                            try FileManager.default.moveItem(at: run.file, to: moved)
                            try run.write(valid)
                        case 1:
                            try run.write(Self.replaced(valid, "[0,17,65,90,97,255]", "[0,18,65,90,97,255]"))
                        case 2:
                            guard chmod(run.file.path, 0o644) == 0 else { throw AgentFailure.storage }
                        case 3:
                            guard chmod(run.file.path, 0o400) == 0, chmod(run.file.path, 0o600) == 0 else { throw AgentFailure.storage }
                        case 4:
                            guard link(run.file.path, run.directory.appendingPathComponent("extra-link").path) == 0 else { throw AgentFailure.storage }
                        default:
                            try FileManager.default.removeItem(at: run.file)
                        }
                        mutationCompleted = true
                    })
                }
                #expect(mutationCompleted)
                if mutation == 4 { try FileManager.default.removeItem(at: run.directory.appendingPathComponent("extra-link")) }
            }
        }
    }

    @Test func everyNamedAncestorRemainsAuthoritativeDuringRead() throws {
        try withRun { run in
            try run.write(Self.input())
            for directory in [run.support] + run.privateDirectories {
                for mutation in 0..<4 {
                    let moved = directory.appendingPathExtension("moved")
                    var mutationCompleted = false
                    #expect(throws: AgentFailure.storage) {
                        try run.admit(afterRead: {
                            if mutation == 3 {
                                guard chmod(directory.path, 0o755) == 0 else { throw AgentFailure.storage }
                            } else {
                                try FileManager.default.moveItem(at: directory, to: moved)
                                switch mutation {
                                case 0:
                                    guard mkdir(directory.path, 0o700) == 0 else { throw AgentFailure.storage }
                                case 1:
                                    // Link back to the same retained directory.
                                    guard symlink(moved.path, directory.path) == 0 else { throw AgentFailure.storage }
                                default:
                                    // Restore the same name/inode: vnode rename
                                    // evidence, rather than directory timestamps.
                                    try FileManager.default.moveItem(at: moved, to: directory)
                                }
                            }
                            mutationCompleted = true
                        })
                    }
                    #expect(mutationCompleted)
                    if mutation == 3 { _ = chmod(directory.path, 0o700) }
                    else if mutation != 2 {
                        try FileManager.default.removeItem(at: directory)
                        try FileManager.default.moveItem(at: moved, to: directory)
                    }
                }
            }
        }
    }

    @Test func retainedAdmissionRevalidatesAtLaterComparisonBoundaries() throws {
        try withRun { run in
            let bytes = try Self.input()
            try run.write(bytes)
            let expected = try run.admit()
            // Unrelated directory entries change timestamps/link counts only.
            let unrelated = run.privateDirectories[0].appendingPathComponent("unrelated", isDirectory: true)
            guard mkdir(unrelated.path, 0o700) == 0 else { throw AgentFailure.storage }
            try expected.revalidate()
            try FileManager.default.removeItem(at: unrelated)
            try expected.requireMatch(Self.identity, label: "acct1")
            try run.write(Self.replaced(bytes, "[0,17,65,90,97,255]", "[0,18,65,90,97,255]"))
            #expect(throws: AgentFailure.storage) { try expected.requireMatch(Self.identity, label: "acct1") }
            try run.write(bytes)
            // Once changed, a retained admission cannot become valid again.
            #expect(throws: AgentFailure.storage) { try expected.revalidate() }
        }
    }
}
