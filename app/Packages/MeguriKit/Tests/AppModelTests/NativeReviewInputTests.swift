import Foundation
import CardRendering
import Testing
@testable import AppModel

// These byte fixtures exercise facts and lifecycle, without native media proof.
@MainActor @Suite(.serialized) struct NativeReviewInputTests {
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
