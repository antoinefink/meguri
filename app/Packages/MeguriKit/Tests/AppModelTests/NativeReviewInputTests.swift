import Darwin
import Foundation
@testable import CardRendering
import Testing
@testable import AppModel

// These byte fixtures exercise facts and lifecycle, without native media proof.
@MainActor @Suite(.serialized) struct NativeReviewInputTests {
    @Test(arguments: ["missingMedia", "audioSession", "audioDecode", "presentFailed", "updateFailed", "appearanceFailed",
                      "processTerminated", "typedAnswerRead", "bridgeOrigin", "bridgeRejected", "recoveryUnavailable", "answerAccessibility"])
    func diagnosticSummaryCountsRecordsByFullUpdate(kind: String) {
        let current = CardUpdate(card: 1, generation: 2, side: .answer)
        let updates: [CardUpdate?] = [current, current, nil,
            CardUpdate(card: 1, generation: 1, side: .answer),
            CardUpdate(card: 1, generation: 2, side: .question),
            CardUpdate(card: 2, generation: 2, side: .answer)]
        let entries = updates.map {
            CardDiagnostics.Entry(update: $0, kind: kind, name: "", index: nil, script: nil, repeats: 1_000_000)
        }
        #expect(NativeReviewInput.diagnosticSummary(entries, currentUpdate: current) ==
            "media renderer diagnostics present: \(kind)(current=2,earlier/other=4)")
    }

    @Test func diagnosticSummaryDistinguishesCurrentMissingMediaFromEarlierPresentationFailure() {
        let current = CardUpdate(card: 1, generation: 2, side: .answer)
        let prior = CardUpdate(card: 1, generation: 1, side: .question)
        let entries = [
            CardDiagnostics.Entry(update: prior, kind: "presentFailed", name: "", index: nil, script: nil, repeats: 1),
            CardDiagnostics.Entry(update: current, kind: "missingMedia", name: "", index: nil, script: nil, repeats: 1),
            CardDiagnostics.Entry(update: nil, kind: "presentFailed", name: "", index: nil, script: nil, repeats: 1),
        ]
        #expect(NativeReviewInput.diagnosticSummary(entries, currentUpdate: current) ==
            "media renderer diagnostics present: missingMedia(current=1,earlier/other=0); presentFailed(current=0,earlier/other=2)")
    }

    @Test func diagnosticSummaryExportsOnlyFixedLabelsAndCounts() {
        let current = CardUpdate(card: 1, generation: 2, side: .answer)
        let privateText = "synthetic-private-<html>-https://example.invalid/private/template.js"
        let script = CardDiagnostics.ScriptLocation(errorClass: privateText, line: 98765, column: 87654, source: privateText)
        let kinds = ["script.\(privateText)", "mediaError.\(privateText)", privateText,
                     "missingMedia.\(privateText)", "script", "mediaError"]
        let entries = kinds.map {
            CardDiagnostics.Entry(update: current, kind: $0, name: privateText, index: 76543, script: script, repeats: 999999)
        }
        let summary = NativeReviewInput.diagnosticSummary(entries, currentUpdate: current)
        #expect(summary == "media renderer diagnostics present: script(current=1,earlier/other=0); mediaError(current=1,earlier/other=0); other(current=4,earlier/other=0)")
        #expect(!summary.contains(privateText))
    }

    @Test func diagnosticSummaryCountsOnlyTypedBridgeReasons() {
        let current = CardUpdate(card: 1, generation: 2, side: .answer)
        let prior = CardUpdate(card: 1, generation: 1, side: .question)
        let privateText = "synthetic-private-https://example.invalid/template.js"
        let values: [(CardUpdate?, CardBridge.RejectionReason?)] = [
            (current, .world), (current, .world), (prior, .olderGeneration), (nil, .world),
            (current, nil), (current, nil), (current, nil),
        ]
        let entries = values.map { update, reason in
            CardDiagnostics.Entry(update: update, kind: "bridgeRejected", name: "", index: nil, script: nil, repeats: 1_000_000,
                                  bridgeRejectionReason: reason)
        }
        #expect(entries.allSatisfy { $0.name.isEmpty })
        let summary = NativeReviewInput.diagnosticSummary(entries, currentUpdate: current)
        #expect(summary == "media renderer diagnostics present: bridgeRejected(current=5,earlier/other=2); bridgeReasons: world(current=2,earlier/other=1); olderGeneration(current=0,earlier/other=1)")
        #expect(!summary.contains(privateText))
        // Neither known nor unknown names on old records become reason detail.
        let names = ["world", "olderGeneration", privateText, "", "world/" + privateText]
        let old = names.map {
            CardDiagnostics.Entry(update: current, kind: "bridgeRejected", name: $0, index: nil, script: nil, repeats: 1)
        }
        #expect(old.allSatisfy { $0.bridgeRejectionReason == nil })
        #expect(NativeReviewInput.diagnosticSummary(old, currentUpdate: current) ==
            "media renderer diagnostics present: bridgeRejected(current=5,earlier/other=0)")
        // A typed reason in any other category is not a bridge reason.
        let other = [CardDiagnostics.Entry(update: current, kind: "audioDecode", name: "", index: nil, script: nil, repeats: 1,
                                           bridgeRejectionReason: .world)]
        #expect(NativeReviewInput.diagnosticSummary(other, currentUpdate: current) ==
            "media renderer diagnostics present: audioDecode(current=1,earlier/other=0)")
    }

    @Test func diagnosticSummaryRemainsBoundedForFullRetainedStore() {
        let current = CardUpdate(card: 1, generation: 2, side: .answer)
        let kinds = ["missingMedia", "audioSession", "audioDecode", "presentFailed", "updateFailed", "appearanceFailed",
                     "processTerminated", "typedAnswerRead", "bridgeOrigin", "bridgeRejected", "recoveryUnavailable", "answerAccessibility",
                     "script.synthetic", "mediaError.synthetic", "synthetic-unknown"]
        let reasons = CardBridge.RejectionReason.allCases
        let entries = (0..<256).map { index in
            let kind = index < kinds.count ? kinds[index] : "bridgeRejected"
            let name = kind == "bridgeRejected" ? "" : String(repeating: "synthetic-private", count: 100)
            return CardDiagnostics.Entry(update: index.isMultiple(of: 2) ? current : nil, kind: kind,
                                         name: name, index: index, script: nil, repeats: 1_000_000,
                                         bridgeRejectionReason: kind == "bridgeRejected" ? reasons[index % reasons.count] : nil)
        }
        let description = NativeReviewInput.Refusal(reason: NativeReviewInput.diagnosticSummary(entries, currentUpdate: current)).description
        #expect(description.utf8.count < 900)
        #expect(!description.contains("synthetic"))
        #expect(description.contains("other(current="))
        #expect(description.contains("bridgeReasons: "))
        #expect(description.hasSuffix("; more"))
    }

    @Test func phaseTransferReturnsOriginalFileBytesAndConsumesNonceOnce() async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let owner = sample.owner()
        try await sample.activate(owner)
        owner.completed(try sample.answer(owner), outcome: "accepted")
        await owner.flushEvidence()
        let file = owner.receiptURL.deletingLastPathComponent().appending(path: "accepted.json")
        let original = try Data(contentsOf: file)
        let request = try sample.phaseRequest(owner, phase: "accepted")
        let response = try await owner.phaseReply(to: request)
        let fields = try NativeReviewInput.PhaseTransport.decode(response, reply: true)
        let encoded: String = try #require(fields["data"])
        let bytes: Data = try #require(Data(base64Encoded: encoded))
        #expect(bytes == original)
        var binding = fields; binding.removeValue(forKey: "data")
        #expect(binding == (try NativeReviewInput.PhaseTransport.decode(request)))
        #expect(try Data(contentsOf: file) == original)
        await #expect(throws: NativeReviewInput.Refusal.self) { try await owner.phaseReply(to: request) }
        var repeated = try NativeReviewInput.PhaseTransport.decode(request)
        repeated["phase"] = "closed" // A different phase cannot reuse the nonce.
        await #expect(throws: NativeReviewInput.Refusal.self) {
            try await owner.phaseReply(to: NativeReviewInput.PhaseTransport.encode(repeated))
        }
    }

    @Test func earlierLaunchPhasesTransferUnchangedThroughCurrentOwner() async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let first = sample.owner()
        try await sample.activate(first)
        first.completed(try sample.answer(first), outcome: "accepted")
        first.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles(); try await first.closed()
        let retiredRequest = try sample.phaseRequest(first, phase: "accepted")
        let directory = first.receiptURL.deletingLastPathComponent()
        let originals = try ["accepted", "closed"].map { try Data(contentsOf: directory.appending(path: "\($0).json")) }
        let next = sample.owner(); try await next.prepare()
        #expect(try await next.reopenSeed() == 44)
        await #expect(throws: NativeReviewInput.Refusal.self) { try await first.phaseReply(to: retiredRequest) }
        await #expect(throws: NativeReviewInput.Refusal.self) { try await next.phaseReply(to: retiredRequest) }
        for (index, phase) in ["accepted", "closed"].enumerated() {
            let response = try await next.phaseReply(to: sample.phaseRequest(next, phase: phase))
            let fields = try NativeReviewInput.PhaseTransport.decode(response, reply: true)
            let encoded: String = try #require(fields["data"])
            let bytes: Data = try #require(Data(base64Encoded: encoded))
            #expect(bytes == originals[index])
            #expect(try Data(contentsOf: directory.appending(path: "\(phase).json")) == originals[index])
            let historical = try JSONDecoder().decode(NativeReviewInput.Receipt.self, from: bytes)
            #expect(historical.owner.uuidString != fields["owner"])
            #expect(historical.admission?.owner == historical.owner)
        }
    }

    @Test(arguments: ["case", "platform", "freezeSHA256", "freezeBytes", "inputSHA256", "inputBytes", "owner", "nonce", "phase", "extra", "duplicate", "object-bound"])
    func phaseRequestRefusesWrongOrMalformedBinding(change: String) async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let owner = sample.owner(); try await sample.activate(owner)
        owner.completed(try sample.answer(owner), outcome: "accepted"); await owner.flushEvidence()
        let valid = try sample.phaseRequest(owner, phase: "accepted")
        var fields = try NativeReviewInput.PhaseTransport.decode(valid)
        switch change {
        case "case": fields[change] = "another-case"
        case "platform": fields[change] = "iphone"
        case "freezeSHA256", "inputSHA256": fields[change] = String(repeating: "f", count: 64)
        case "freezeBytes", "inputBytes": fields[change] = "1"
        case "owner": fields[change] = UUID().uuidString
        case "nonce": fields[change] = "not-a-UUID"
        case "phase": fields[change] = "receipt" // Never an arbitrary filename/path.
        case "extra": fields["path"] = "/tmp/arbitrary.json"
        default: break
        }
        var object = String(decoding: try JSONSerialization.data(withJSONObject: fields,
            options: [.sortedKeys, .withoutEscapingSlashes]), as: UTF8.self)
        if change == "duplicate" { object = "{\"phase\":\"accepted\"," + object.dropFirst() }
        if change == "object-bound" { object = String(repeating: " ", count: NativeReviewInput.PhaseTransport.objectLimit + 1) }
        await #expect(throws: (any Error).self) { try await owner.phaseReply(to: object) }
        // Refusing a malformed request does not consume the valid request.
        let response = try await owner.phaseReply(to: valid)
        #expect(!(try NativeReviewInput.PhaseTransport.decode(response, reply: true)).isEmpty)
    }

    @Test(arguments: ["missing", "changed", "symlink", "hardlink", "oversize", "directory", "writable", "freeze", "missing-phase"])
    func phaseTransferRefusesUnsafeOrChangedFiles(change: String) async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let owner = sample.owner(); try await sample.activate(owner)
        owner.completed(try sample.answer(owner), outcome: "accepted"); await owner.flushEvidence()
        let request = try sample.phaseRequest(owner, phase: change == "missing-phase" ? "import" : "accepted")
        let file = owner.receiptURL.deletingLastPathComponent().appending(path: "accepted.json")
        switch change {
        case "missing": try FileManager.default.removeItem(at: file)
        case "changed":
            var original = try Data(contentsOf: file); original.append(0x20)
            try original.write(to: file) // Still valid JSON with the same receipt facts.
        case "symlink":
            let moved = file.appendingPathExtension("original")
            try FileManager.default.moveItem(at: file, to: moved)
            try FileManager.default.createSymbolicLink(at: file, withDestinationURL: moved)
        case "hardlink": try FileManager.default.linkItem(at: file, to: file.appendingPathExtension("linked"))
        case "oversize": try Data(repeating: 0x20, count: 128 * 1024 + 1).write(to: file)
        case "directory":
            try FileManager.default.removeItem(at: file)
            try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        case "writable": try FileManager.default.setAttributes([.posixPermissions: 0o666], ofItemAtPath: file.path)
        case "freeze": try sample.write("\(sample.base)/freeze.json", Data("{}".utf8))
        default: break
        }
        await #expect(throws: (any Error).self) { try await owner.phaseReply(to: request) }
    }

    @Test func phaseDigestSurvivesRelaunchAndDetectsHistoricalFileMutation() async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let first = sample.owner(); try await sample.activate(first)
        first.completed(try sample.answer(first), outcome: "accepted")
        first.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles(); try await first.closed()
        let file = first.receiptURL.deletingLastPathComponent().appending(path: "accepted.json")
        var original = try Data(contentsOf: file); original.append(0x20); try original.write(to: file)
        let next = sample.owner(); try await next.prepare(); _ = try await next.reopenSeed()
        await #expect(throws: NativeReviewInput.Refusal.self) {
            try await next.phaseReply(to: sample.phaseRequest(next, phase: "accepted"))
        }
    }

    @Test func cancelledPhaseReadCannotReplyAndUnpreparedOwnerIsInert() async throws {
        let sample = try NativeReviewInputFixture(); defer { sample.remove() }
        let owner = sample.owner()
        #expect(throws: NativeReviewInput.Refusal.self) { try owner.phaseTransportOwner() }
        try await sample.activate(owner)
        owner.completed(try sample.answer(owner), outcome: "accepted"); await owner.flushEvidence()
        let object = try sample.phaseRequest(owner, phase: "accepted")
        let pending = Task { try await owner.phaseReply(to: object) }
        pending.cancel()
        do { _ = try await pending.value; Issue.record("Cancelled phase read replied") }
        catch { #expect(error is CancellationError) }
    }

    @Test func noFollowPhaseReadAndNotificationRespectExactByteBounds() throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let ceiling = NativeReviewInput.PhaseTransport.fileLimit
        let permitted = Data(repeating: 0x35, count: ceiling)
        let receiptName = "size-ceiling.json"
        try fixture.write(receiptName, permitted)
        let diskBytes = try FrozenNativeReviewInput.read(fixture.root, receiptName, limit: ceiling, singleLink: true)
        #expect(diskBytes == permitted)

        var envelope = try fixture.phaseFields(owner: UUID().uuidString, phase: "accepted")
        envelope["data"] = permitted.base64EncodedString()
        let notificationText = try NativeReviewInput.PhaseTransport.encode(envelope, reply: true)
        #expect(notificationText.utf8.count <= NativeReviewInput.PhaseTransport.objectLimit)
        let decodedEnvelope = try NativeReviewInput.PhaseTransport.decode(notificationText, reply: true)
        let encodedPayload: String = try #require(decodedEnvelope["data"])
        let decodedPayload: Data = try #require(Data(base64Encoded: encodedPayload))
        #expect(decodedPayload == permitted)

        var beyondCeiling = permitted
        beyondCeiling.append(0x35)
        try fixture.write(receiptName, beyondCeiling)
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.read(fixture.root, receiptName, limit: ceiling, singleLink: true)
        }
        envelope["data"] = beyondCeiling.base64EncodedString()
        #expect(throws: NativeReviewInput.Refusal.self) {
            try NativeReviewInput.PhaseTransport.encode(envelope, reply: true)
        }
        envelope["data"] = Data().base64EncodedString()
        #expect(throws: NativeReviewInput.Refusal.self) {
            try NativeReviewInput.PhaseTransport.encode(envelope, reply: true)
        }
    }

    @Test func phaseReadAcceptsVerifiedSystemAliasAndAdmittedPhysicalRoot() throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let parts = fixture.root.pathComponents
        let physicalSpelling = parts.count > 2 && parts[1] == "private"
        let aliasIndex = physicalSpelling ? 2 : 1
        try #require(parts.count > aliasIndex)
        let alias = parts[aliasIndex]
        try #require(["var", "tmp", "etc"].contains(alias))
        let suffix = parts.dropFirst(aliasIndex + 1).joined(separator: "/")
        let aliasRoot = URL(filePath: "/\(alias)/\(suffix)", directoryHint: .isDirectory)
        let alternateRoot = URL(filePath: "/private/\(alias)/\(suffix)", directoryHint: .isDirectory)
        var info = stat()
        try #require(lstat("/\(alias)", &info) == 0)
        try #require(info.st_uid == 0 && info.st_mode & S_IFMT == S_IFLNK)
        let expected = Data("verified OS anchor".utf8)
        try fixture.write("anchor-positive.json", expected)
        #expect(try FrozenNativeReviewInput.read(aliasRoot, "anchor-positive.json", singleLink: true) == expected)
        if alternateRoot.path != alternateRoot.standardizedFileURL.path ||
            alternateRoot.path != alternateRoot.resolvingSymlinksInPath().path {
            #expect(throws: NativeReviewInput.Refusal.self) {
                try FrozenNativeReviewInput.read(alternateRoot, "anchor-positive.json", singleLink: true)
            }
        } else {
            #expect(try FrozenNativeReviewInput.read(alternateRoot, "anchor-positive.json", singleLink: true) == expected)
        }

        let cache = try FileManager.default.url(for: .cachesDirectory, in: .userDomainMask,
            appropriateFor: nil, create: false).resolvingSymlinksInPath()
        let physicalRoot = cache.appending(path: "Meguri-native-input-\(UUID().uuidString)", directoryHint: .isDirectory)
        try #require(mkdir(physicalRoot.path, 0o700) == 0)
        defer { _ = rmdir(physicalRoot.path) }
        let file = physicalRoot.appending(path: "anchor-positive.json")
        let descriptor = Darwin.open(file.path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0o600)
        try #require(descriptor >= 0)
        defer { _ = unlink(file.path) }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? handle.close() }
        try handle.write(contentsOf: expected)
        try handle.close()
        try #require(lstat(physicalRoot.path, &info) == 0 && info.st_uid == getuid() &&
            info.st_mode & S_IFMT == S_IFDIR && info.st_mode & 0o777 == 0o700)
        try #require(lstat(file.path, &info) == 0 && info.st_uid == getuid() &&
            info.st_mode & S_IFMT == S_IFREG && info.st_nlink == 1 && info.st_mode & 0o022 == 0)
        try #require(physicalRoot.path == physicalRoot.standardizedFileURL.path)
        try #require(physicalRoot.path == physicalRoot.resolvingSymlinksInPath().path)
        try #require(FrozenNativeReviewInput.scoped(physicalRoot, "anchor-positive.json").path == file.path)
        #expect(try FrozenNativeReviewInput.read(physicalRoot, "anchor-positive.json", singleLink: true) == expected)
    }

    @Test(arguments: ["var", "tmp", "etc"])
    func systemAliasPolicyAcceptsOnlyExactCompiledRootTargets(alias: String) {
        var info = stat()
        info.st_mode = mode_t(S_IFLNK | 0o755); info.st_uid = 0
        let target = Array("private/\(alias)".utf8)
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 1, ownedRootDepth: 6,
            info: info, target: target) == ["private", alias])
        for wrongTarget in [target + [0], target + Array("/suffix".utf8), Array("/private/\(alias)".utf8), Array("private/other".utf8)] {
            #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 1, ownedRootDepth: 6,
                info: info, target: wrongTarget) == nil)
        }
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 2, ownedRootDepth: 6,
            info: info, target: target) == nil)
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 1, ownedRootDepth: 1,
            info: info, target: target) == nil)
        #expect(FrozenNativeReviewInput.systemAliasTarget("unknown", index: 1, ownedRootDepth: 6,
            info: info, target: target) == nil)
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias.uppercased(), index: 1, ownedRootDepth: 6,
            info: info, target: target) == nil)
        info.st_uid = 501
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 1, ownedRootDepth: 6,
            info: info, target: target) == nil)
        info.st_uid = 0; info.st_mode = mode_t(S_IFDIR | 0o755)
        #expect(FrozenNativeReviewInput.systemAliasTarget(alias, index: 1, ownedRootDepth: 6,
            info: info, target: target) == nil)
    }

    @Test(arguments: ["device", "inode", "owner", "group", "mode", "links", "size", "mtime", "ctime"])
    func systemAliasIdentityRefusesSubstitution(field: String) throws {
        var original = stat()
        original.st_mode = mode_t(S_IFLNK | 0o755); original.st_uid = 0
        var changed = original
        switch field {
        case "device": changed.st_dev += 1
        case "inode": changed.st_ino += 1
        case "owner": changed.st_uid += 1
        case "group": changed.st_gid += 1
        case "mode": changed.st_mode ^= 0o020
        case "links": changed.st_nlink += 1
        case "size": changed.st_size += 1
        case "mtime": changed.st_mtimespec.tv_nsec += 1
        case "ctime": changed.st_ctimespec.tv_nsec += 1
        default: Issue.record("Unknown substitution control")
        }
        try FrozenNativeReviewInput.checkSystemAliasIdentity(original, original)
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.checkSystemAliasIdentity(original, changed)
        }
    }

    @Test(arguments: ["var", "tmp", "etc", "unknown"])
    func phaseReadRejectsAliasSpellingInsideOwnedTree(alias: String) throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let expected = Data("same bytes through owned link".utf8)
        try fixture.write("physical/result.json", expected)
        try FileManager.default.createSymbolicLink(at: fixture.root.appending(path: alias),
            withDestinationURL: fixture.root.appending(path: "physical"))
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.read(fixture.root, "\(alias)/result.json", singleLink: true)
        }
        #expect(try Data(contentsOf: fixture.root.appending(path: "\(alias)/result.json")) == expected)
    }

    @Test func phaseReadAllowsUnrelatedSharedDirectoryEntryChanges() throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let expected = Data("stable directory identity".utf8)
        try fixture.write("shared/records/result.json", expected)
        let actual = try FrozenNativeReviewInput.read(fixture.root, "shared/records/result.json", singleLink: true) {
            try fixture.write("unrelated/new-entry", Data([1]))
            try fixture.write("shared/unrelated/new-entry", Data([2]))
        }
        #expect(actual == expected)
    }

    @Test func phaseDirectoryIdentityPinsTargetWithoutSharedEntryMetadata() {
        var original = stat()
        original.st_mode = mode_t(S_IFDIR | 0o755); original.st_uid = 0
        var unrelated = original
        unrelated.st_size += 1; unrelated.st_nlink += 1
        unrelated.st_mtimespec.tv_nsec += 1; unrelated.st_ctimespec.tv_nsec += 1
        #expect(FrozenNativeReviewInput.PhaseDirectoryIdentity(original) == FrozenNativeReviewInput.PhaseDirectoryIdentity(unrelated))
        var substituted = original
        substituted.st_ino += 1
        #expect(FrozenNativeReviewInput.PhaseDirectoryIdentity(original) != FrozenNativeReviewInput.PhaseDirectoryIdentity(substituted))
        substituted = original; substituted.st_uid += 1
        #expect(FrozenNativeReviewInput.PhaseDirectoryIdentity(original) != FrozenNativeReviewInput.PhaseDirectoryIdentity(substituted))
        substituted = original; substituted.st_mode ^= 0o020
        #expect(FrozenNativeReviewInput.PhaseDirectoryIdentity(original) != FrozenNativeReviewInput.PhaseDirectoryIdentity(substituted))
    }

    @Test func phaseReadRefusesReplacedAncestorWithSameLeafAndParent() throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let expected = Data("unchanged leaf and parent".utf8)
        let leaf = "swap-check/records/result.json"
        try fixture.write(leaf, expected)
        let named = fixture.root.appending(path: "swap-check")
        let moved = fixture.root.appending(path: "moved-check")
        var didRunAfterRead = false
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.read(fixture.root, leaf, singleLink: true) {
                try FileManager.default.moveItem(at: named, to: moved)
                try FileManager.default.createDirectory(at: named, withIntermediateDirectories: false,
                    attributes: [.posixPermissions: 0o700])
                try FileManager.default.moveItem(at: moved.appending(path: "records"), to: named.appending(path: "records"))
                didRunAfterRead = true
            }
        }
        #expect(didRunAfterRead)
        #expect(try Data(contentsOf: fixture.root.appending(path: leaf)) == expected)
    }

    @Test func phaseReadRefusesAncestorLinkSubstitutionDespiteUnchangedBytes() throws {
        let scenario = try NativeReviewInputFixture()
        defer { scenario.remove() }
        let leaf = "swap-check/records/result.json"
        let expected = Data([0x7b, 0x7d, 0x0a])
        try scenario.write(leaf, expected)
        let expectedIdentity = NativeReviewIdentity.of(expected)
        let namedDirectory = scenario.root.appending(path: "swap-check")
        let movedDirectory = scenario.root.appending(path: "moved-check")
        var didRunAfterRead = false
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.read(scenario.root, leaf, singleLink: true) {
                try FileManager.default.moveItem(at: namedDirectory, to: movedDirectory)
                try FileManager.default.createSymbolicLink(at: namedDirectory, withDestinationURL: movedDirectory)
                didRunAfterRead = true
            }
        }
        #expect(didRunAfterRead)
        let reread = try Data(contentsOf: scenario.root.appending(path: leaf))
        #expect(reread == expected)
        #expect(NativeReviewIdentity.of(reread) == expectedIdentity)
    }

    @Test func frozenRecordBindsEveryRequiredFileAndExactRun() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let loaded = try FrozenNativeReviewInput.load(fixture.request, admittedRoot: fixture.root)
        #expect(loaded.input.run == fixture.run)
        #expect(loaded.input.seeds.inspection_and_initial_activation == 43)
        #expect(loaded.input.seeds.reopen == 44)
        let owner = fixture.owner()
        try await owner.prepare()
        #expect(try await owner.inspectionSeed() == 43)
        await #expect(throws: NativeReviewInput.Refusal.self) { try await owner.inspectionSeed() }
        await #expect(throws: NativeReviewInput.Refusal.self) { try await owner.reopenSeed() }
        await owner.flushEvidence()
        #expect(owner.flushStatus == .saved)
        let receipt = try fixture.receipt(owner)
        #expect(receipt.seeds == ["inspection_and_initial_activation": 43])
        #expect(receipt.admission == nil && receipt.completion == nil)
        #expect(!FileManager.default.fileExists(atPath: fixture.location.collectionFile.path))
        let next = fixture.owner()
        await #expect(throws: NativeReviewInput.Refusal.self) { try await owner.inspectionSeed() }
        _ = next
    }
    @Test(arguments: ["absent", "edited", "other-run", "extra-key", "repeat-key", "absent-null", "limit", "steps", "build", "freeze"])
    func invalidRecordIsRejected(change: String) throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        switch change {
        case "absent":
            try FileManager.default.removeItem(at: sample.root.appending(path: "\(sample.base)/review/collection.media/meguri-review.wav"))
        case "edited": try sample.write("\(sample.base)/input.colpkg", Data("edited package".utf8))
        case "freeze": try sample.write("\(sample.base)/freeze.json", Data("{}".utf8))
        default:
            var record = sample.input
            switch change {
            case "other-run": record["run"] = "build/mg/ui-test/other-case"
            case "extra-key": record["ambient_seed"] = 99
            case "absent-null":
                var choice = record["chosen"] as! [String: Any]
                choice.removeValue(forKey: "resume_card_id")
                record["chosen"] = choice
            case "limit":
                var answer = record["answer"] as! [String: Any]
                answer["preset_cap_answer_time_secs"] = 0
                record["answer"] = answer
            case "steps": record["actions"] = ["answer_easy"]
            case "build":
                var code = record["code"] as! [String: Any]
                code["revision"] = "unknown"
                record["code"] = code
            default: break
            }
            var bytes = try sample.json(record)
            if change == "repeat-key" {
                let tail = String(decoding: bytes.dropFirst(), as: UTF8.self)
                bytes = Data(("{\"schema_version\":1," + tail).utf8)
            }
            try sample.write("\(sample.base)/input.json", bytes)
            try sample.refreeze()
        }
        #expect(throws: (any Error).self) { try FrozenNativeReviewInput.load(sample.request, admittedRoot: sample.root) }
    }
    @Test func boundSourceChangeAndUnadmittedCheckoutRefuse() throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        #expect(throws: NativeReviewInput.Refusal.self) {
            try FrozenNativeReviewInput.load(fixture.request, admittedRoot: fixture.root.deletingLastPathComponent())
        }
        try fixture.write("\(fixture.run)/source-preparation/fixture/collection.anki2", Data("later app data".utf8))
        #expect(throws: NativeReviewInput.Refusal.self) { try FrozenNativeReviewInput.load(fixture.request, admittedRoot: fixture.root) }
    }
    @Test func noDatabaseResetAndNoAnswerReplayOnBoundReopen() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let owner = fixture.owner()
        try await fixture.activate(owner)
        let use = try fixture.answer(owner)
        owner.completed(use, outcome: "accepted")
        owner.ended(coordinator: fixture.coordinator, session: 5)
        try fixture.installClosedFiles()
        try await owner.closed()
        let before = try Data(contentsOf: fixture.location.collectionFile)
        let reopened = fixture.owner()
        try await reopened.prepare()
        #expect(try await reopened.reopenSeed() == 44)
        await #expect(throws: NativeReviewInput.Refusal.self) { try await reopened.inspectionSeed() }
        #expect(throws: NativeReviewInput.Refusal.self) { try fixture.answer(reopened) }
        await #expect(throws: NativeReviewInput.Refusal.self) { try await reopened.reopenSeed() }
        #expect(try Data(contentsOf: fixture.location.collectionFile) == before)
        await reopened.flushEvidence()
        #expect(try fixture.receipt(reopened).completion == "accepted")
    }
    @Test(arguments: ["absent", "digest", "database", "unfinished"])
    func priorDatabaseRequiresValidRecord(change: String) async throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        let input = sample.owner()
        try await sample.activate(input)
        let admitted = try sample.answer(input)
        input.completed(admitted, outcome: "accepted")
        input.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles()
        try await input.closed()
        switch change {
        case "absent": try FileManager.default.removeItem(at: input.receiptURL)
        case "database": try Data("edited database".utf8).write(to: sample.location.collectionFile)
        default:
            var record = try sample.receipt(input)
            if change == "unfinished" {
                record.phase = "answer-admitted"
                record.completion = nil
                try JSONEncoder().encode(record).write(to: input.receiptURL)
            } else {
                let replacement = NativeReviewIdentity(sha256: String(repeating: "f", count: 64), bytes: 1)
                let receiptData = try JSONEncoder().encode(record)
                var receiptFields = try JSONSerialization.jsonObject(with: receiptData) as! [String: Any]
                receiptFields["freezeIdentity"] = try JSONSerialization.jsonObject(with: JSONEncoder().encode(replacement))
                try sample.json(receiptFields).write(to: input.receiptURL)
            }
        }
        let next = sample.owner()
        do {
            try await next.prepare()
            Issue.record("Invalid prior record was admitted")
        } catch { #expect(error is NativeReviewInput.Refusal) }
        #expect(FileManager.default.fileExists(atPath: sample.location.collectionFile.path))
    }
    @Test func changingInputAfterLoadRefusesBeforeAnActualSeedConsumer() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let owner = fixture.owner()
        try await owner.prepare()
        try fixture.write("\(fixture.base)/input.json", Data("changed after load".utf8))
        await #expect(throws: NativeReviewInput.Refusal.self) { try await owner.inspectionSeed() }
        await owner.flushEvidence()
        #expect(try fixture.receipt(owner).seeds.isEmpty)
    }
    @Test func admissionAndAcceptedCompletionAreSeparateAndSingleUse() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let owner = fixture.owner()
        try await fixture.activate(owner)
        let use = try fixture.answer(owner)
        await owner.flushEvidence()
        #expect(try fixture.receipt(owner).admission == use)
        #expect(try fixture.receipt(owner).completion == nil)
        let factsOnly = try fixture.receipt(owner)
        #expect(factsOnly.image == nil && factsOnly.audio == nil && factsOnly.journal == nil)
        #expect(throws: NativeReviewInput.Refusal.self) { try fixture.answer(owner) }
        owner.completed(use, outcome: "stale")
        owner.completed(use, outcome: "accepted") // Late/duplicate completion cannot upgrade failure.
        await owner.flushEvidence()
        #expect(try fixture.receipt(owner).completion == "stale")
        #expect(throws: NativeReviewInput.Refusal.self) { try fixture.answer(owner) }
    }
    @Test func activationIdentityAndActualFactsRefuseMismatch() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let owner = fixture.owner()
        try await owner.prepare(); _ = try await owner.inspectionSeed()
        try owner.inspected(fixture.inspection(), identity: ObjectIdentifier(fixture))
        let wrong = NSObject()
        #expect(throws: NativeReviewInput.Refusal.self) { try owner.willActivate(identity: ObjectIdentifier(wrong)) }
        try owner.willActivate(identity: ObjectIdentifier(fixture))
        #expect(throws: NativeReviewInput.Refusal.self) { try owner.willActivate(identity: ObjectIdentifier(fixture)) }
        await owner.flushEvidence()
    }

    @Test func nativePagesWithoutDecodedMediaAndJournalRefuseAdmission() async throws {
        let fixture = try NativeReviewInputFixture()
        defer { fixture.remove() }
        let owner = fixture.owner()
        try await fixture.activate(owner)
        let mediaRoot = fixture.root.appending(path: "\(fixture.run)/source-preparation/fixture/collection.media")
        let resolver = try await MediaPathResolver.open(root: mediaRoot, trackedNames: [])
        let pages = ReviewPages(resolver: resolver)
        defer { pages.close() }
        let item = try #require(ReviewCoordinatorTests.head(1).items.first)
        #expect(throws: NativeReviewInput.Refusal.self) {
            try pages.admitNativeInput(owner, coordinator: fixture.coordinator, session: 5, generation: 1,
                deck: 1, item: item, rating: .easy, instant: FrozenNativeReviewInput.now, eligible: true)
        }
        await owner.flushEvidence()
        let refused = try fixture.receipt(owner)
        #expect(refused.admission == nil && refused.completion == nil)
        #expect(refused.image == nil && refused.audio == nil && refused.journal == nil)
        await resolver.close()
    }

    @Test func exactEmptyFixtureAllowsInitialResetAndMarkerBootstrap() async throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        var checkout = URL(filePath: #filePath)
        for _ in 0..<6 { checkout.deleteLastPathComponent() }
        let relative = "app/Fixtures/CollectionEntry/EmptyCollection"
        let source = sample.root.appending(path: relative)
        try FileManager.default.createDirectory(at: source.deletingLastPathComponent(), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: checkout.appending(path: relative), to: source)
        try sample.write("container/Collection/collection.anki2", Data("prior development data".utf8))
        let owner = sample.owner()
        try owner.validateInitialReset(from: source)
        // The app's FixtureInstaller performs this copy after validation.
        try FileManager.default.removeItem(at: sample.location.directory)
        try FileManager.default.copyItem(at: source, to: sample.location.directory)
        try await owner.prepare()
        #expect(try await owner.inspectionSeed() == 43)
        await owner.flushEvidence()
        #expect(try sample.receipt(owner).phase == "inspecting")
        #expect(!FileManager.default.fileExists(atPath: sample.location.collectionFile.path))
    }

    @Test(arguments: [true, false])
    func initialResetAcceptsEquivalentCheckoutURLHints(requestIsDirectory: Bool) throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        let requestRoot = URL(filePath: sample.root.path, directoryHint: requestIsDirectory ? .isDirectory : .notDirectory)
        let admittedRoot = URL(filePath: sample.root.path, directoryHint: requestIsDirectory ? .notDirectory : .isDirectory)
        try #require(requestRoot != admittedRoot)
        try #require(requestRoot.path == admittedRoot.path)
        let original = sample.request
        let request = NativeReviewInput.Request(root: requestRoot, caseName: original.caseName, platform: original.platform,
            freezeSHA256: original.freezeSHA256, freezeBytes: original.freezeBytes)
        let owner = NativeReviewInput(request: request, collection: sample.location, admittedRoot: admittedRoot,
            evidenceRoot: sample.root.appending(path: "container/test-evidence"))
        let source = sample.root.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection")
        try sample.write("app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep", Data())
        try owner.validateInitialReset(from: source)
        // Equivalent URL representations still cannot reset a recorded case.
        try FileManager.default.createDirectory(at: owner.receiptURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data().write(to: owner.receiptURL)
        #expect(throws: NativeReviewInput.Refusal.self) { try owner.validateInitialReset(from: source) }
    }

    @Test(arguments: ["foreign", "non-file", "noncanonical", "link", "freeze"])
    func initialResetRetainsCheckoutAndFrozenInputGuards(change: String) throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        try sample.write("app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep", Data())
        var requestRoot = sample.root, admittedRoot = sample.root
        switch change {
        case "foreign": requestRoot = sample.root.deletingLastPathComponent()
        case "non-file":
            requestRoot = try #require(URL(string: "https://localhost\(sample.root.path)"))
            try #require(requestRoot.path == admittedRoot.path && !requestRoot.isFileURL)
        case "noncanonical":
            requestRoot = URL(filePath: sample.root.path + "/.", directoryHint: .isDirectory)
            try #require(requestRoot.path != requestRoot.standardizedFileURL.path)
            admittedRoot = requestRoot
        case "link":
            requestRoot = sample.root.appending(path: "checkout-link", directoryHint: .isDirectory)
            try FileManager.default.createSymbolicLink(at: requestRoot, withDestinationURL: sample.root)
            admittedRoot = requestRoot
        case "freeze": try sample.write("\(sample.base)/freeze.json", Data("{}".utf8))
        default: break
        }
        let original = sample.request
        let request = NativeReviewInput.Request(root: requestRoot, caseName: original.caseName, platform: original.platform,
            freezeSHA256: original.freezeSHA256, freezeBytes: original.freezeBytes)
        let owner = NativeReviewInput(request: request, collection: sample.location, admittedRoot: admittedRoot,
            evidenceRoot: sample.root.appending(path: "container/test-evidence"))
        #expect(throws: NativeReviewInput.Refusal.self) {
            try owner.validateInitialReset(from: admittedRoot.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"))
        }
    }

    @Test(arguments: ["arbitrary", "source", "missing", "extra", "bytes", "link"])
    func initialResetRejectsOtherSourcesAndMarkerChanges(change: String) throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        let source = sample.root.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection")
        try sample.write("app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep", Data())
        try sample.write("container/Collection/collection.anki2", Data("keep database".utf8))
        var requested = source
        switch change {
        case "arbitrary":
            try sample.write("app/Fixtures/Other/.gitkeep", Data())
            requested = sample.root.appending(path: "app/Fixtures/Other")
        case "source": requested = sample.root.appending(path: "\(sample.run)/source-preparation/fixture")
        case "missing": try FileManager.default.removeItem(at: source.appending(path: ".gitkeep"))
        case "extra": try Data().write(to: source.appending(path: "collection.anki2"))
        case "bytes": try Data([1]).write(to: source.appending(path: ".gitkeep"))
        case "link":
            try FileManager.default.removeItem(at: source.appending(path: ".gitkeep"))
            try sample.write("empty-marker", Data())
            try FileManager.default.createSymbolicLink(at: source.appending(path: ".gitkeep"), withDestinationURL: sample.root.appending(path: "empty-marker"))
        default: break
        }
        let owner = sample.owner()
        #expect(throws: NativeReviewInput.Refusal.self) { try owner.validateInitialReset(from: requested) }
        #expect(try Data(contentsOf: sample.location.collectionFile) == Data("keep database".utf8))
    }

    @Test func savedCaseCannotResetOnRelaunch() async throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        try sample.write("app/Fixtures/CollectionEntry/EmptyCollection/.gitkeep", Data())
        let owner = sample.owner()
        try await sample.activate(owner)
        let use = try sample.answer(owner)
        owner.completed(use, outcome: "accepted")
        owner.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles()
        try await owner.closed()
        let database = try Data(contentsOf: sample.location.collectionFile)
        let next = sample.owner()
        #expect(throws: NativeReviewInput.Refusal.self) {
            try next.validateInitialReset(from: sample.root.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection"))
        }
        #expect(try Data(contentsOf: sample.location.collectionFile) == database)
        try await next.prepare()
        #expect(try await next.reopenSeed() == 44)
        await #expect(throws: NativeReviewInput.Refusal.self) { try await next.inspectionSeed() }
    }

    @Test func closeRequestWaitsForRealCloseAndSavedReceipt() async throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        let owner = sample.owner()
        try await sample.activate(owner)
        let use = try sample.answer(owner)
        owner.completed(use, outcome: "accepted")
        var calls = 0
        await #expect(throws: NativeReviewInput.Refusal.self) {
            try await owner.closeForEvidence { calls += 1; return true }
        }
        #expect(calls == 0)
        owner.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles()
        try await owner.closeForEvidence {
            calls += 1
            do { try await owner.closed(); return true }
            catch { return false }
        }
        #expect(calls == 1)
        #expect(owner.flushStatus == .saved)
        #expect(try sample.receipt(owner).phase == "closed")
        #expect(try sample.receipt(owner).closedFiles != nil)
    }

    @Test(arguments: ["close", "receipt", "write"])
    func closeRequestPropagatesCloseAndEvidenceFailures(failure: String) async throws {
        let sample = try NativeReviewInputFixture()
        defer { sample.remove() }
        let owner = sample.owner()
        try await sample.activate(owner)
        let use = try sample.answer(owner)
        owner.completed(use, outcome: "accepted")
        owner.ended(coordinator: sample.coordinator, session: 5)
        try sample.installClosedFiles()
        await owner.flushEvidence()
        if failure == "write" {
            try FileManager.default.removeItem(at: owner.receiptURL)
            try FileManager.default.createDirectory(at: owner.receiptURL, withIntermediateDirectories: false)
        }
        await #expect(throws: NativeReviewInput.Refusal.self) {
            try await owner.closeForEvidence {
                if failure == "close" { return false }
                if failure == "receipt" { return true }
                do { try await owner.closed(); return true }
                catch { return false }
            }
        }
        if failure == "write" {
            if case .failed = owner.flushStatus {} else { Issue.record("Write failure was lost") }
        }
    }
}

@MainActor final class NativeReviewInputFixture {
    let root = FileManager.default.temporaryDirectory.appending(path: "native-input-\(UUID().uuidString)").resolvingSymlinksInPath()
    let run = "build/mg/ui-test/owner-case"
    var base: String { "\(run)/expected-replay" }
    var input: [String: Any] = [:]
    var freezeIdentity: NativeReviewIdentity!
    let coordinator = UUID()
    var location: AppModel.CollectionLocation { .init(directory: root.appending(path: "container/Collection")) }
    var request: NativeReviewInput.Request {
        NativeReviewInput.Request(root: root, caseName: "owner-case", platform: "mac",
            freezeSHA256: freezeIdentity.sha256, freezeBytes: freezeIdentity.bytes)
    }
    init() throws {
        let origin = "build/fixtures/owner-parent"
        let staging = "\(run)/source-preparation/fixture"
        var originIDs = [String: Any]()
        var stagingIDs = [String: Any]()
        for directory in [origin, staging] {
            let mediaDirectory = root.appending(path: "\(directory)/collection.media")
            try FileManager.default.createDirectory(at: mediaDirectory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            var paths = ["collection.anki2", "manifest.json"]
            if directory == staging { paths.append(contentsOf: FrozenNativeReviewInput.media.map { "collection.media/\($0)" }) }
            for path in paths {
                let bytes = Data("sample \(path)".utf8)
                try write("\(directory)/\(path)", bytes)
                if directory == origin { originIDs[path] = try identity(bytes) }
                else { stagingIDs[path] = try identity(bytes) }
            }
        }
        let totalMedia = FrozenNativeReviewInput.media.reduce(0) { total, name in
            total + Data("sample collection.media/\(name)".utf8).count
        }
        let clock: [String: Any] = ["now": "2030-01-15T10:00:00+01:00", "tz": "Europe/Paris", "now_ms": FrozenNativeReviewInput.now]
        var preparation: [String: Any] = [
            "schema_version": 1, "operation": "prepare-native-review", "stage": "source-only-before-app",
            "run": run, "platform": "mac", "source": origin, "prepared_source": staging, "clock": clock,
            "generator_request": ["seed": 40, "generator_version": 5, "recipe": [:]] as [String: Any],
            "parent_files": originIDs, "prepared_files": stagingIDs,
            "source_facts": ["card_id": 1, "deck_id": 1, "deck_name": "Default",
                "row_counts": ["notes": 1, "cards": 1, "revlog": 2], "media_files": 3, "media_bytes": totalMedia] as [String: Any],
            "preset_cap_answer_time_secs": 60,
        ]
        let nullable = ["media", "row_changes", "selected_parent_deck", "preserved_card", "preserved_history", "media_pattern", "derivation", "remaining"]
        for field in nullable { preparation[field] = NSNull() }
        let preparationBytes = try json(preparation)
        try write("\(run)/source-preparation/preparation.json", preparationBytes)
        let packageBytes = Data("sample package".utf8)
        try write("\(base)/input.colpkg", packageBytes)
        let code: [String: Any] = ["revision": String(repeating: "a", count: 40), "build_profile": "dev",
            "executable": try identity(Data([1])), "replay_source": try identity(Data([2]))]
        input = [
            "schema_version": 1, "purpose": "f11.2-trusted-rust-delivery-persistence", "run": run, "platform": "mac",
            "preparation": ["path": "\(run)/source-preparation/preparation.json", "identity": try identity(preparationBytes)] as [String: Any],
            "parent_source": origin, "parent_files": originIDs, "prepared_source": staging, "prepared_files": stagingIDs,
            "package": ["path": "\(base)/input.colpkg", "identity": try identity(packageBytes)] as [String: Any],
            "generator_seed": 40, "wall": clock,
            "seeds": ["source_open": 41, "package_write": 42, "inspection_and_initial_activation": 43, "reopen": 44],
            "chosen": ["deck_id": 1, "deck_name": "Default", "card_id": 1, "resume_card_id": NSNull()] as [String: Any],
            "answer": ["rating": "easy", "elapsed_ms": 1000, "count": 1, "preset_cap_answer_time_secs": 60] as [String: Any],
            "no_old_database": true, "actions": FrozenNativeReviewInput.actions, "code": code,
        ]
        try write("\(base)/input.json", json(input))
        try write("\(base)/replay.json", Data("sample replay".utf8))
        for stage in ["source", "import", "review"] {
            let paths = ["collection.anki2", "manifest.json"] + FrozenNativeReviewInput.media.map { "collection.media/\($0)" }
            for path in paths { try write("\(base)/\(stage)/\(path)", Data("expected \(stage)/\(path)".utf8)) }
        }
        try refreeze()
    }
    func json(_ value: Any) throws -> Data { try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) }
    func identity(_ bytes: Data) throws -> Any { try JSONSerialization.jsonObject(with: JSONEncoder().encode(NativeReviewIdentity.of(bytes))) }
    func write(_ relative: String, _ bytes: Data) throws {
        let target = root.appending(path: relative)
        try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try bytes.write(to: target)
    }
    func refreeze() throws {
        var paths = ["input.json", "input.colpkg", "replay.json"]
        for stage in ["source", "import", "review"] {
            paths.append(contentsOf: ["\(stage)/collection.anki2", "\(stage)/manifest.json"])
            paths.append(contentsOf: FrozenNativeReviewInput.media.map { "\(stage)/collection.media/\($0)" })
        }
        var digests = [String: Any]()
        for path in paths {
            let bytes = try Data(contentsOf: root.appending(path: "\(base)/\(path)"))
            digests[path] = try identity(bytes)
        }
        let record: [String: Any] = ["schema_version": 1, "run": run, "platform": "mac",
            "stage": "expected-states-before-app", "files": digests]
        let bytes = try json(record)
        try write("\(base)/freeze.json", bytes)
        freezeIdentity = NativeReviewIdentity.of(bytes)
    }
    func owner() -> NativeReviewInput {
        let evidence = root.appending(path: "container/test-evidence")
        return NativeReviewInput(request: request, collection: location, admittedRoot: root, evidenceRoot: evidence)
    }
    func phaseFields(owner: String, phase: String) throws -> [String: String] {
        let inputID = NativeReviewIdentity.of(try Data(contentsOf: root.appending(path: "\(base)/input.json")))
        return ["version": "1", "case": request.caseName, "platform": request.platform,
            "freezeSHA256": request.freezeSHA256, "freezeBytes": String(request.freezeBytes),
            "inputSHA256": inputID.sha256, "inputBytes": String(inputID.bytes),
            "owner": owner, "nonce": UUID().uuidString, "phase": phase]
    }
    func phaseRequest(_ owner: NativeReviewInput, phase: String) throws -> String {
        try NativeReviewInput.PhaseTransport.encode(phaseFields(owner: owner.phaseTransportOwner(), phase: phase))
    }
    func receipt(_ owner: NativeReviewInput) throws -> NativeReviewInput.Receipt {
        try JSONDecoder().decode(NativeReviewInput.Receipt.self, from: Data(contentsOf: owner.receiptURL))
    }
    func inspection() throws -> NativeReviewInput.Inspection {
        let loaded = try FrozenNativeReviewInput.load(request, admittedRoot: root)
        return try JSONDecoder().decode(NativeReviewInput.Inspection.self, from: json([
            "sha256": loaded.input.package.identity.sha256, "notes": 1, "cards": 1, "reviews": 2, "schema": 18,
            "replacing": false, "mediaCount": 3, "mediaBytes": loaded.input.mediaBytes]))
    }
    func activate(_ owner: NativeReviewInput, bindSession: Bool = true) async throws {
        try await owner.prepare(); #expect(try await owner.inspectionSeed() == 43)
        try owner.inspected(inspection(), identity: ObjectIdentifier(self)); try owner.willActivate(identity: ObjectIdentifier(self))
        let loaded = try FrozenNativeReviewInput.load(request, admittedRoot: root)
        let activation = try JSONDecoder().decode(NativeReviewInput.Activation.self, from: json([
            "generation": 1, "backup": NSNull(), "retention": NSNull(),
            "media": ["incoming": 3, "decodedBytes": loaded.input.mediaBytes, "reused": 0, "added": 3, "trashed": 0]] as [String: Any]))
        owner.activated(activation)
        if bindSession { try owner.bindSession(coordinator: coordinator, session: 5, generation: 1, deck: 1) }
    }
    @discardableResult func answer(_ owner: NativeReviewInput) throws -> NativeReviewInput.AnswerUse {
        let item = try #require(ReviewCoordinatorTests.head(1).items.first)
        return try owner.admitFacts(coordinator: coordinator, session: 5, generation: 1, deck: 1, item: item,
            rating: .easy, instant: FrozenNativeReviewInput.now, eligible: true)
    }
    func installClosedFiles() throws {
        try FileManager.default.createDirectory(at: location.directory.appending(path: "collection.media"), withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try Data("actual closed database bytes".utf8).write(to: location.collectionFile)
        for name in FrozenNativeReviewInput.media {
            try Data(contentsOf: root.appending(path: "\(run)/source-preparation/fixture/collection.media/\(name)")).write(to: location.directory.appending(path: "collection.media/\(name)"))
        }
    }
    func remove() { try? FileManager.default.removeItem(at: root) }
}
