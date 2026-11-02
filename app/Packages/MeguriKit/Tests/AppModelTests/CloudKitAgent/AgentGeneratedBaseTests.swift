import Darwin
import Foundation
import Testing

// Generated unit evidence only: opaque database/WAL bytes exercise delivery,
// not SQLite readback, a full leg, product admission or signed SDK execution.
@MainActor @Suite(.serialized) struct AgentGeneratedBaseTests {
    private static let host = "/synthetic-checkout"
    private static let source = host + "/build/fixtures/cloud-small-7"
    private static let runID = "20320405T060708Z-4"
    private static func encode(_ value: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    }
    private static func object(_ data: Data) throws -> [String: Any] {
        try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }
    private static func identity(_ data: Data) -> [String: Any] { ["sha256": AgentJSON.hash(data), "bytes": data.count] }
    private struct Draft {
        var generation: [String: Any], receipt: [String: Any], frozen: [String: Any], header: [String: Any]
        var files: [String: Data]
    }
    private static func draft(wal: Bool = false) throws -> Draft {
        let counts = ["notes": 20, "cards": 20, "revlog": 60, "notetypes": 1, "decks": 2, "presets": 1,
                      "tags": 20, "media_refs": 5, "media_files": 5, "media_bytes": 51200]
        let manifest: [String: Any] = ["generator_version": 5, "kind": "cloud-small", "name": "cloud-small", "seed": 7,
            "cards_requested": 20, "now_ms": 1735689600000 as Int64, "utc_offset_secs": 0, "profile_sha256": NSNull(),
            "worst_case_sha256": NSNull(), "media": "files", "counts": counts, "dump_sha256": String(repeating: "c", count: 64)]
        let request: [String: Any] = ["seed": 7, "now_ms": 1735689600000 as Int64, "utc_offset_secs": 0, "rollover_hour": 4,
            "notes": 20, "cards": 20, "media": "files", "media_files": 5, "media_file_bytes": 10240,
            "reviews_per_card": 3, "review_days_before_pinned": [3, 2, 1], "answer_seconds_after_day_start": 28800,
            "answer_ordinal_seconds": true]
        let generation: [String: Any] = ["dir": "build/fixtures/cloud-small-7", "cached": false, "elapsed_ms": 123,
            "peak_rss_kb": 321, "manifest": manifest, "request": request,
            "observed": ["counts": counts, "dump_sha256": manifest["dump_sha256"]!]]
        var files = ["collection.anki2": Data(repeating: 23, count: 1024), "manifest.json": try encode(manifest)]
        for index in 0..<5 { files["collection.media/unit-\(index).png"] = Data(repeating: UInt8(index), count: 10240) }
        if wal { files["collection.anki2-wal"] = Data(repeating: 31, count: 80) }
        let artifacts: [[String: Any]] = [
            ["kind": "fixture", "path": source], ["kind": "manifest", "path": source + "/manifest.json"],
            ["kind": "generation", "path": host + "/build/mg/fixtures/" + runID + "/cloud-small.json"]]
        let receipt: [String: Any] = ["schema_version": 1, "job": "fixtures", "run_id": runID, "status": "pass", "exit_code": 0,
            "placement": "docker", "service": "rust", "dry_run": false, "args": ["cloud-small", "--seed", "7", "--media", "files"],
            "root": host, "started_at": "2032-04-05T06:07:08Z", "finished_at": "2032-04-05T06:08:08Z", "duration_ms": 60000,
            "commit": ["sha": String(repeating: "a", count: 40), "dirty": false], "message": "unit evidence",
            "summary": ["total": 1, "passed": 1, "failed": 0, "skipped": 0], "failures": [], "user_steps": [], "artifacts": artifacts,
            "details": ["fixtures": []]]
        let header = try object(CloudKitAgentProtocolTests.header)
        let frozen: [String: Any] = ["protocol_version": 1, "run_id": header["run_id"]!, "device": header["device"]!,
            "source_commit": header["source_commit"]!, "base": "small", "prepared_source": source, "parent_files": [:], "files": [:]]
        return Draft(generation: generation, receipt: receipt, frozen: frozen, header: header, files: files)
    }
    private final class Run {
        let root: URL
        var header = Data()
        init(wal: Bool = false, edit: ((inout Draft) throws -> Void)? = nil) throws {
            guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            root = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
                .appendingPathComponent("meguri-generated-base-" + UUID().uuidString, isDirectory: true)
            guard mkdir(root.path, 0o700) == 0 else { throw AgentFailure.storage }
            var d = try AgentGeneratedBaseTests.draft(wal: wal)
            let map = d.files.mapValues { AgentGeneratedBaseTests.identity($0) }
            d.frozen["parent_files"] = map; d.frozen["files"] = map
            try edit?(&d)
            var selected = d.generation; selected["target"] = "cloud-small"; selected["kind"] = "cloud-small"
            selected["mg_only_metadata"] = "unit"
            if var details = d.receipt["details"] as? [String: Any],
               (details["fixtures"] as? [[String: Any]])?.isEmpty == true {
                details["fixtures"] = [selected]
                d.receipt["details"] = details
            }
            let frozenBytes = try AgentGeneratedBaseTests.encode(d.frozen)
            var manifestRef = AgentGeneratedBaseTests.identity(d.files["manifest.json"]!); manifestRef["path"] = "manifest.json"
            var frozenRef = AgentGeneratedBaseTests.identity(frozenBytes); frozenRef["path"] = "fixture-input.json"
            var delivery: [String: Any] = ["base": "small", "fixtures_receipt": ["job": "fixtures", "run_id": AgentGeneratedBaseTests.runID],
                "source": AgentGeneratedBaseTests.source, "manifest": manifestRef, "frozen_input": frozenRef]
            if let preparation = d.header["unit_preparation"] { delivery["preparation_receipt"] = preparation; d.header.removeValue(forKey: "unit_preparation") }
            d.header["fixture_delivery"] = delivery
            header = try AgentGeneratedBaseTests.encode(d.header)
            try mkdirs("fixture-evidence/parent/collection.media")
            try put("fixture-input.json", frozenBytes)
            try put("fixture-evidence/fixtures-result.json", AgentGeneratedBaseTests.encode(d.receipt))
            try put("fixture-evidence/generation.json", AgentGeneratedBaseTests.encode(d.generation))
            for (name, bytes) in d.files { try put("fixture-evidence/parent/" + name, bytes) }
            try start()
        }
        func url(_ path: String) -> URL { root.appendingPathComponent(path) }
        func mkdirs(_ path: String) throws {
            try FileManager.default.createDirectory(at: url(path), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        }
        func put(_ path: String, _ bytes: Data) throws { try bytes.write(to: url(path)); guard chmod(url(path).path, 0o600) == 0 else { throw AgentFailure.storage } }
        func start() throws {
            let store = try AgentFileStorage(directory: root), hash = AgentJSON.hash(header)
            let command = Data("{\"header_sha256\":\"\(hash)\",\"command_seq\":1,\"leg\":\"smoke\",\"action\":\"launch\",\"arguments\":{},\"account_before\":\"acct1\",\"account_after\":\"acct1\"}".utf8)
            try store.replace(.header, with: header)
            try store.replace(.commands, with: command + Data([10]))
            let started = AgentRecord(header_sha256: hash, record_seq: 1, command_seq: 1, command_sha256: AgentJSON.hash(command),
                agent_instance: "unit1", observed_at: "2032-04-05T06:07:08Z", kind: .started, action: .launch)
            try store.appendJournal(JSONEncoder().encode(started) + Data([10]))
        }
        func install(destination: URL? = nil, before: (() throws -> Void)? = nil, after: (() throws -> Void)? = nil) throws {
            try AgentGeneratedBase(admittedRunDirectory: root, originalHeaderBytes: header)
                .requireInstalled(at: destination ?? url("data"), beforeCopy: before, afterCopy: after)
        }
        func changeJSON(_ path: String, _ edit: (inout [String: Any]) throws -> Void) throws {
            var f = try AgentGeneratedBaseTests.object(Data(contentsOf: url(path)))
            try edit(&f); try put(path, AgentGeneratedBaseTests.encode(f))
        }
    }
    private func withRun(wal: Bool = false, edit: ((inout Draft) throws -> Void)? = nil, _ body: (Run) throws -> Void) throws {
        let run = try Run(wal: wal, edit: edit)
        defer { try? FileManager.default.removeItem(at: run.root) }
        try body(run)
    }
    private func refuses(edit: ((inout Draft) throws -> Void)? = nil, mutate: ((Run) throws -> Void)? = nil) throws {
        try withRun(edit: edit) { run in
            try mutate?(run)
            #expect(throws: (any Error).self) { try run.install() }
            #expect(!FileManager.default.fileExists(atPath: run.url("data").path))
        }
    }

    @Test func installsCompleteDirectSmallAndOptionalWALWithExactOriginalBytes() throws {
        for wal in [false, true] {
            try withRun(wal: wal) { run in
                try run.install()
                let original = try Data(contentsOf: run.url("fixture-evidence/parent/collection.anki2"))
                #expect(try Data(contentsOf: run.url("data/collection.anki2")) == original)
                #expect(FileManager.default.fileExists(atPath: run.url("data/collection.anki2-wal").path) == wal)
                let state = AgentJSON.lines(try Data(contentsOf: run.url("fixture-install/steps.jsonl")))
                #expect(state.complete.count == 2 && state.suffix.isEmpty)
                #expect(!FileManager.default.fileExists(atPath: run.url("fixture-evidence/prepared").path))
            }
        }
    }
    @Test func requiredWALAndEveryMediaFileAreChecked() throws {
        for mutation in ["missing", "corrupt"] {
            try withRun(wal: true) { run in
                if mutation == "missing" {
                    try FileManager.default.removeItem(at: run.url("fixture-evidence/parent/collection.anki2-wal"))
                } else { try run.put("fixture-evidence/parent/collection.anki2-wal", Data(repeating: 17, count: 80)) }
                #expect(throws: (any Error).self) { try run.install() }
                #expect(!FileManager.default.fileExists(atPath: run.url("data").path))
            }
        }
    }
    @Test func relaunchRetainsModifiedCollectionMediaAndTrackingWithoutRecopy() throws {
        try withRun { run in
            try run.install()
            let changed = Data("changed-live-database".utf8), tracking = Data("persisted-tracking".utf8)
            try run.put("data/collection.anki2", changed)
            try run.put("data/collection.media/unit-0.png", Data([12]))
            try run.put("data/cloud-tracking.sqlite", tracking)
            let state = try Data(contentsOf: run.url("fixture-install/steps.jsonl"))
            try run.install(before: { Issue.record("relaunch tried to copy") })
            #expect(try Data(contentsOf: run.url("data/collection.anki2")) == changed)
            #expect(try Data(contentsOf: run.url("data/cloud-tracking.sqlite")) == tracking)
            #expect(try Data(contentsOf: run.url("data/collection.media/unit-0.png")) == Data([12]))
            #expect(try Data(contentsOf: run.url("fixture-install/steps.jsonl")) == state)
        }
    }
    @Test func originalReceiptRequiresExactReferenceFullEnvelopeCleanPassAndArtifacts() throws {
        for key in ["schema_version", "summary", "details", "commit", "started_at"] {
            try refuses(edit: { $0.receipt.removeValue(forKey: key) })
        }
        for (key, value) in [("job", "other"), ("run_id", "20320405T060708Z-9"), ("status", "warn"),
                                          ("exit_code", 1), ("dry_run", true), ("artifacts", [])] as [(String, Any)] {
            try refuses(edit: { $0.receipt[key] = value })
        }
        try refuses(edit: { $0.receipt["commit"] = ["sha": String(repeating: "a", count: 40), "dirty": true] })
        try refuses(edit: { $0.receipt["commit"] = ["sha": String(repeating: "b", count: 40), "dirty": false] })
        try refuses(mutate: { run in
            let path = "fixture-evidence/fixtures-result.json", bytes = try Data(contentsOf: run.url(path))
            try run.put(path, Data("{\"job\":\"fixtures\",".utf8) + bytes.dropFirst())
        })
    }
    @Test(arguments: ["fixture", "manifest", "generation"], [false, true])
    func consumedArtifactPathRequiresExactlyOneDeclaration(kind: String, conflictingKind: Bool) throws {
        try withRun(edit: { d in
            var artifacts = try #require(d.receipt["artifacts"] as? [[String: Any]])
            var duplicate = try #require(artifacts.first { $0["kind"] as? String == kind })
            if conflictingKind { duplicate["kind"] = "other" }
            artifacts.append(duplicate)
            d.receipt["artifacts"] = artifacts
        }) { run in
            #expect(throws: AgentFailure.binding) { try run.install() }
            #expect(!FileManager.default.fileExists(atPath: run.url("data").path))
            #expect(!FileManager.default.fileExists(atPath: run.url("fixture-install").path))
        }
    }
    @Test func selectionAndProjectionMustMatchOriginalGeneration() throws {
        try refuses(edit: { d in d.receipt["details"] = ["fixtures": [["target": "cloud-small", "kind": "cloud-small"]]] })
        for count in [0, 2] {
            try refuses(mutate: { run in try run.changeJSON("fixture-evidence/fixtures-result.json") { receipt in
                var details = receipt["details"] as! [String: Any], entries = details["fixtures"] as! [[String: Any]]
                entries = count == 0 ? [] : entries + entries; details["fixtures"] = entries; receipt["details"] = details
            } })
        }
        for key in ["request", "manifest", "observed"] {
            try refuses(mutate: { run in try run.changeJSON("fixture-evidence/generation.json") { $0[key] = [:] } })
        }
        for key in ["notes", "now_ms", "rollover_hour", "media_file_bytes", "answer_seconds_after_day_start"] {
            try refuses(edit: { d in var r = d.generation["request"] as! [String: Any]; r[key] = 1; d.generation["request"] = r })
        }
        try refuses(edit: { d in var r = d.generation["request"] as! [String: Any]; r["review_days_before_pinned"] = [2, 1, 0]; d.generation["request"] = r })
        try refuses(edit: { d in var o = d.generation["observed"] as! [String: Any]; o["dump_sha256"] = String(repeating: "d", count: 64); d.generation["observed"] = o })
    }
    @Test func frozenBytesFieldsCompleteMapsAndSameSizeCorruptionAreBound() throws {
        for key in ["run_id", "device", "source_commit", "base", "prepared_source"] {
            try refuses(edit: { $0.frozen[key] = "changed" })
        }
        try refuses(edit: { $0.frozen["unexpected"] = 1 })
        try refuses(edit: { $0.frozen["files"] = [:] })
        try refuses(edit: { d in
            var map = d.frozen["files"] as! [String: Any]; map["collection.anki2"] = ["sha256": String(repeating: "a", count: 64), "bytes": 1024]
            d.frozen["files"] = map
        })
        try refuses(edit: { d in
            var map = d.frozen["files"] as! [String: Any]; map["../escape"] = map.removeValue(forKey: "collection.anki2")
            d.frozen["files"] = map; d.frozen["parent_files"] = map
        })
        try refuses(mutate: { try $0.put("fixture-input.json", Data(" ".utf8) + Data(contentsOf: $0.url("fixture-input.json"))) })
        for path in ["collection.anki2", "manifest.json", "collection.media/unit-3.png"] {
            try refuses(mutate: { run in
                let target = "fixture-evidence/parent/" + path, data = try Data(contentsOf: run.url(target))
                try run.put(target, Data(repeating: 3, count: data.count))
            })
        }
    }
    @Test func absentExtraOptionalLinkedAliasedAndSpecialPathsRefuse() throws {
        try refuses(mutate: { try FileManager.default.removeItem(at: $0.url("fixture-evidence/parent/collection.media/unit-1.png")) })
        for path in ["fixture-evidence/parent/extra", "fixture-evidence/parent/collection.media/extra", "fixture-evidence/preparation-result.json"] {
            try refuses(mutate: { try $0.put(path, Data([1])) })
        }
        try refuses(mutate: { try $0.mkdirs("fixture-evidence/prepared") })
        for kind in ["symlink", "hardlink", "fifo", "alias"] {
            try refuses(mutate: { run in
                let target = run.url("fixture-evidence/parent/collection.media/unit-0.png")
                if kind == "alias" {
                    var finder = [UInt8](repeating: 0, count: 32); finder[8] = 0x80
                    let result = finder.withUnsafeBytes { setxattr(target.path, "com.apple.FinderInfo", $0.baseAddress, $0.count, 0, 0) }
                    #expect(result == 0)
                } else {
                    try FileManager.default.removeItem(at: target)
                    switch kind {
                    case "symlink": #expect(symlink(run.url("fixture-evidence/parent/collection.media/unit-1.png").path, target.path) == 0)
                    case "hardlink": #expect(link(run.url("fixture-evidence/parent/collection.media/unit-1.png").path, target.path) == 0)
                    default: #expect(mkfifo(target.path, 0o600) == 0)
                    }
                }
            })
        }
    }
    @Test func unsafePermissionsAndLinkedFixtureDirectoriesRefuse() throws {
        for path in ["fixture-input.json", "fixture-evidence", "fixture-evidence/generation.json", "fixture-evidence/fixtures-result.json"] {
            try refuses(mutate: { #expect(chmod($0.url(path).path, 0o777) == 0) })
        }
        try refuses(mutate: { run in
            try FileManager.default.moveItem(at: run.url("fixture-evidence/parent"), to: run.url("fixture-evidence/other"))
            #expect(symlink("other", run.url("fixture-evidence/parent").path) == 0)
        })
    }
    @Test func refusesDefaultDestinationExistingDataAndLaunchWithoutStarted() throws {
        try withRun { run in
            #expect(throws: AgentFailure.binding) { try run.install(destination: run.url("Default")) }
            #expect(!FileManager.default.fileExists(atPath: run.url("Default").path))
            try run.mkdirs("data"); try run.put("data/do-not-touch", Data([42]))
            #expect(throws: (any Error).self) { try run.install() }
            #expect(try Data(contentsOf: run.url("data/do-not-touch")) == Data([42]))
        }
        try refuses(mutate: { try FileManager.default.removeItem(at: $0.url("steps.jsonl")) })
    }
    @Test func unsupportedPreparationFailsExplicitly() throws {
        try withRun(edit: { $0.header["unit_preparation"] = ["job": "prepare", "run_id": "20320405T060708Z-5"] }) { run in
            #expect(throws: AgentFailure.implementationMissing) { try run.install() }
        }
    }
    @Test func authorityChangesAndCopyCorruptionFailWithoutCompletion() throws {
        for change in ["permission", "rename", "same-size", "extra", "optional"] {
            try withRun { run in
                #expect(throws: (any Error).self) { try run.install(before: {
                    switch change {
                    case "permission": #expect(chmod(run.url("fixture-evidence/parent/collection.anki2").path, 0o666) == 0)
                    case "rename":
                        try FileManager.default.moveItem(at: run.url("fixture-evidence/parent"), to: run.url("fixture-evidence/moved"))
                        #expect(symlink("moved", run.url("fixture-evidence/parent").path) == 0)
                    case "same-size": try run.put("fixture-evidence/parent/collection.anki2", Data(repeating: 99, count: 1024))
                    case "extra": try run.put("fixture-evidence/parent/extra", Data([1]))
                    default: try run.mkdirs("fixture-evidence/prepared")
                    }
                }) }
                #expect(!FileManager.default.fileExists(atPath: run.url("data").path))
            }
        }
        try withRun { run in
            #expect(throws: (any Error).self) { try run.install(after: { try run.put("data/collection.anki2", Data(repeating: 9, count: 1024)) }) }
            let state = AgentJSON.lines(try Data(contentsOf: run.url("fixture-install/steps.jsonl")))
            #expect(state.complete.count == 1)
        }
    }
    @Test func interruptedInstallNeverRetriesDeletesOrOverwrites() throws {
        for afterCopy in [false, true] {
            try withRun { run in
                let killed: () throws -> Void = { throw AgentFailure.execution }
                #expect(throws: AgentFailure.execution) { try run.install(before: afterCopy ? nil : killed, after: afterCopy ? killed : nil) }
                let state = try Data(contentsOf: run.url("fixture-install/steps.jsonl"))
                #expect(throws: AgentFailure.recovery) { try run.install() }
                #expect(try Data(contentsOf: run.url("fixture-install/steps.jsonl")) == state)
                #expect(FileManager.default.fileExists(atPath: run.url("data/collection.anki2").path) == afterCopy)
            }
        }
    }
    @Test func relaunchRefusesChangedBindingReplacedDirectoryAndLinkedLiveCollection() throws {
        for change in ["binding", "directory", "link"] {
            try withRun { run in
                try run.install()
                switch change {
                case "binding": try run.changeJSON("fixture-evidence/fixtures-result.json") { $0["message"] = "changed valid envelope" }
                case "directory":
                    try FileManager.default.moveItem(at: run.url("data"), to: run.url("old-data")); try run.mkdirs("data")
                default:
                    try FileManager.default.removeItem(at: run.url("data/collection.anki2"))
                    #expect(symlink(run.url("fixture-evidence/parent/collection.anki2").path, run.url("data/collection.anki2").path) == 0)
                }
                #expect(throws: (any Error).self) { try run.install() }
            }
        }
    }
}
