import Darwin
import Foundation
import Testing

@MainActor @Suite(.serialized) struct AgentRunDirectoryTests {
    // Generated file authority tests only, never actual app-container evidence.
    private final class Fixture {
        let base: URL, support: URL, run: URL
        var observed: URL
        let bytes = CloudKitAgentProtocolTests.header
        init() throws {
            guard let resolved = realpath(FileManager.default.temporaryDirectory.path, nil) else { throw AgentFailure.storage }
            defer { free(resolved) }
            base = URL(fileURLWithPath: String(cString: resolved), isDirectory: true)
                .appendingPathComponent("meguri-run-directory-" + UUID().uuidString, isDirectory: true)
            support = base.appendingPathComponent("support", isDirectory: true)
            observed = support
            let header = try AgentHeader.read(bytes)
            run = support.appendingPathComponent("Meguri/CloudKitSigned/" + header.run_id + "/" + header.device, isDirectory: true)
            try FileManager.default.createDirectory(at: run, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
            try bytes.write(to: run.appendingPathComponent("run.json"))
        }
        func admit(_ spelling: String? = nil) throws -> AgentRunDirectory {
            try .generatedUnitOnly(expectedPath: spelling ?? run.path, trustedRoot: { self.observed })
        }
    }
    private func fixture(_ body: (Fixture) throws -> Void) throws {
        let value = try Fixture()
        defer { try? FileManager.default.removeItem(at: value.base) }
        try body(value)
    }
    @Test func exactPathAndOriginalHeaderBytesAreRetained() throws {
        try fixture { f in
            let admitted = try f.admit()
            #expect(admitted.directory == f.run)
            #expect(admitted.originalHeaderBytes == f.bytes)
            #expect(try admitted.storage.read(.run) == f.bytes)
            #expect(try admitted.retainedApplicationSupportRoot() == f.support)
            let equivalent = try JSONSerialization.data(withJSONObject: JSONSerialization.jsonObject(with: f.bytes), options: [.sortedKeys])
            #expect(equivalent != f.bytes)
            try equivalent.write(to: f.run.appendingPathComponent("run.json"))
            #expect(throws: (any Error).self) { try admitted.revalidate() }
            try f.bytes.write(to: f.run.appendingPathComponent("run.json"))
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
        }
    }
    @Test func rawAliasesTraversalAndOtherContainerRejectBeforeRunOpen() throws {
        try fixture { f in
            // A FIFO would block if the run reader were reached. Every raw
            // spelling must reject before constructing storage or opening it.
            let file = f.run.appendingPathComponent("run.json")
            try FileManager.default.removeItem(at: file)
            #expect(mkfifo(file.path, 0o600) == 0)
            let parent = f.run.deletingLastPathComponent().path
            for spelling in [parent + "/./rig7", parent + "/other/../rig7", parent + "//rig7",
                             f.run.path + "/", f.base.path + "/another-container/Meguri/CloudKitSigned/20320405T060708Z-1/rig7",
                             parent + "/bad.label", parent + "/-bad", parent + "/" + String(repeating: "a", count: 65),
                             f.support.path + "/Meguri/CloudKitSigned/not-a-run/rig7"] {
                #expect(throws: AgentFailure.binding) { try f.admit(spelling) }
            }
            let alias = f.base.appendingPathComponent("alias", isDirectory: true)
            try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: f.support)
            #expect(throws: AgentFailure.binding) {
                try f.admit(alias.path + "/Meguri/CloudKitSigned/20320405T060708Z-1/rig7")
            }
            #expect(!FileManager.default.fileExists(atPath: f.base.path + "/another-container"))
        }
    }
    @Test func fixedPathDescendantLinkIsNeverFollowed() throws {
        try fixture { f in
            let parent = f.run.deletingLastPathComponent()
            let saved = parent.appendingPathComponent("saved", isDirectory: true)
            try FileManager.default.moveItem(at: f.run, to: saved)
            try FileManager.default.createSymbolicLink(at: f.run, withDestinationURL: saved)
            #expect(throws: AgentFailure.storage) { try f.admit() }
        }
    }
    @Test func pathLabelsMustMatchParsedOriginalHeader() throws {
        try fixture { f in
            for replacement in ["\"device\":\"other\"", "\"device\":\"bad.label\""] {
                let bytes = Data(String(decoding: f.bytes, as: UTF8.self).replacingOccurrences(of: "\"device\":\"rig7\"", with: replacement).utf8)
                try bytes.write(to: f.run.appendingPathComponent("run.json"))
                #expect(throws: (any Error).self) { try f.admit() }
            }
            let bytes = Data(String(decoding: f.bytes, as: UTF8.self).replacingOccurrences(of: "20320405T060708Z-1", with: "20320405T060708Z-9").utf8)
            try bytes.write(to: f.run.appendingPathComponent("run.json"))
            #expect(throws: AgentFailure.binding) { try f.admit() }
        }
    }
    @Test func sameOriginalHeaderAtReplacementInodeStillRefuses() throws {
        try fixture { f in
            let admitted = try f.admit()
            let file = f.run.appendingPathComponent("run.json")
            let replacement = f.run.appendingPathComponent("replacement.json")
            try f.bytes.write(to: replacement)
            #expect(rename(replacement.path, file.path) == 0)
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
        }
    }
    @Test func observedRootChangeOrReplacementPermanentlyRefuses() throws {
        try fixture { f in
            let admitted = try f.admit()
            f.observed = f.base.appendingPathComponent("other", isDirectory: true)
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
            f.observed = f.support
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
        }
        try fixture { f in
            let admitted = try f.admit()
            let saved = f.base.appendingPathComponent("retained", isDirectory: true)
            try FileManager.default.moveItem(at: f.support, to: saved)
            try FileManager.default.createDirectory(at: f.support, withIntermediateDirectories: false)
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
            try FileManager.default.removeItem(at: f.support)
            try FileManager.default.moveItem(at: saved, to: f.support)
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
        }
    }
    @Test func transientRenameRestoredBeforeCheckStillInvalidates() throws {
        try fixture { f in
            let admitted = try f.admit()
            let saved = f.base.appendingPathComponent("transient", isDirectory: true)
            try FileManager.default.moveItem(at: f.support, to: saved)
            try FileManager.default.moveItem(at: saved, to: f.support)
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
            #expect(throws: AgentFailure.storage) { try admitted.revalidate() }
        }
    }
}
