import CryptoKit
import Darwin
import Foundation

// F11.2's Rust Input/load_frozen contract. These are input values, never
// reconstructed from a later collection, review record or native export.
nonisolated struct NativeReviewIdentity: Codable, Sendable, Equatable {
    let sha256: String
    let bytes: UInt64
    static func of(_ data: Data) -> Self {
        Self(sha256: SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined(), bytes: UInt64(data.count))
    }
}
nonisolated struct FrozenNativeReviewInput: Decodable, Sendable {
    struct Bound: Decodable, Sendable { let path: String; let identity: NativeReviewIdentity }
    struct Seeds: Decodable, Sendable {
        let source_open: UInt64, package_write: UInt64, inspection_and_initial_activation: UInt64, reopen: UInt64
    }
    struct Wall: Decodable, Sendable, Equatable { let now: String, tz: String; let now_ms: Int64 }
    struct Choice: Decodable, Sendable {
        let deck_id: Int64, deck_name: String, card_id: Int64, resume_card_id: Int64?
    }
    struct Answer: Decodable, Sendable {
        let rating: String, elapsed_ms: UInt64, count: UInt8, preset_cap_answer_time_secs: UInt32
    }
    struct Code: Decodable, Sendable {
        let revision: String, build_profile: String
        let executable: NativeReviewIdentity, replay_source: NativeReviewIdentity
    }
    let schema_version: UInt32, purpose: String, run: String, platform: String
    let preparation: Bound, parent_source: String, parent_files: [String: NativeReviewIdentity]
    let prepared_source: String, prepared_files: [String: NativeReviewIdentity], package: Bound
    let generator_seed: UInt64, wall: Wall, seeds: Seeds, chosen: Choice, answer: Answer
    let no_old_database: Bool, actions: [String], code: Code
    static let now: Int64 = 1_894_698_000_000
    static let media = ["_meguri-unused.txt", "meguri-review.png", "meguri-review.wav"]
    static let actions = ["inspect_and_activate_initial", "select_deck", "start_session", "reveal_answer", "answer_easy",
                          "end_session", "close", "reopen", "export_all_media"]
    var mediaBytes: UInt64 { Self.media.reduce(0) { $0 + prepared_files["collection.media/\($1)"]!.bytes } }

    struct Loaded: Sendable {
        let input: FrozenNativeReviewInput
        let identity: NativeReviewIdentity
        let sourceReviews: UInt64
    }
    private struct Freeze: Decodable {
        let schema_version: UInt32, run: String, platform: String, stage: String
        let files: [String: NativeReviewIdentity]
    }
    static func load(_ request: NativeReviewInput.Request, admittedRoot: URL) throws -> Loaded {
        try check(request.root.isFileURL && request.root.path == admittedRoot.path, "checkout mismatch")
        try check(component(request.caseName) && ["mac", "iphone", "ipad"].contains(request.platform), "case or platform")
        let runPath = "build/mg/ui-test/\(request.caseName)"
        let artifacts = "\(runPath)/expected-replay"
        let expectedFreeze = NativeReviewIdentity(sha256: request.freezeSHA256, bytes: request.freezeBytes)
        let freezeBytes = try read(request.root, "\(artifacts)/freeze.json", limit: 128 * 1024)
        try check(expectedFreeze == NativeReviewIdentity.of(freezeBytes), "freeze digest mismatch")
        let freezeObject = try object(freezeBytes, keys: "schema_version run platform stage files")
        let freezeHeader = try JSONDecoder().decode(Freeze.self, from: freezeBytes)
        try check(freezeHeader.schema_version == 1 && freezeHeader.run == runPath && freezeHeader.platform == request.platform &&
            freezeHeader.stage == "expected-states-before-app", "freeze metadata")
        let artifactIDs = try identities(freezeObject["files"])
        var paths = ["input.json", "input.colpkg", "replay.json"]
        for stage in ["source", "import", "review"] {
            paths.append("\(stage)/collection.anki2")
            paths.append("\(stage)/manifest.json")
            paths.append(contentsOf: media.map { "\(stage)/collection.media/\($0)" })
        }
        try check(Set(artifactIDs.keys) == Set(paths), "artifact names")
        for path in paths {
            let bytes = try read(request.root, "\(artifacts)/\(path)")
            try check(NativeReviewIdentity.of(bytes) == artifactIDs[path], "artifact digest mismatch")
        }
        let inputBytes = try read(request.root, "\(artifacts)/input.json", limit: 128 * 1024)
        let record = try object(inputBytes, keys: "schema_version purpose run platform preparation parent_source parent_files prepared_source prepared_files package generator_seed wall seeds chosen answer no_old_database actions code")
        for name in ["preparation", "package"] {
            try Self.fields(record[name], "path identity")
            try Self.fields((record[name] as? [String: Any])?["identity"], "sha256 bytes")
        }
        try Self.fields(record["wall"], "now tz now_ms")
        try Self.fields(record["seeds"], "source_open package_write inspection_and_initial_activation reopen")
        try Self.fields(record["chosen"], "deck_id deck_name card_id resume_card_id")
        try Self.fields(record["answer"], "rating elapsed_ms count preset_cap_answer_time_secs")
        try Self.fields(record["code"], "revision build_profile executable replay_source")
        for name in ["executable", "replay_source"] {
            try Self.fields((record["code"] as? [String: Any])?[name], "sha256 bytes")
        }
        _ = try identities(record["parent_files"])
        _ = try identities(record["prepared_files"])
        let input = try JSONDecoder().decode(Self.self, from: inputBytes)
        try check(input.schema_version == 1 && input.purpose == "f11.2-trusted-rust-delivery-persistence" &&
            input.run == runPath && input.platform == request.platform && input.no_old_database && input.actions == actions, "input metadata or actions")
        try check(input.wall == Wall(now: "2030-01-15T10:00:00+01:00", tz: "Europe/Paris", now_ms: now), "clock mismatch")
        try check(input.answer.rating == "easy" && input.answer.count == 1 && input.answer.elapsed_ms == 1000 &&
            input.answer.preset_cap_answer_time_secs <= UInt32.max / 1000 &&
            UInt64(input.answer.preset_cap_answer_time_secs) * 1000 >= input.answer.elapsed_ms, "answer timing")
        try check(input.chosen.deck_id == 1 && input.chosen.deck_name == "Default" && input.chosen.card_id > 0 &&
            input.chosen.card_id < now && input.chosen.resume_card_id == nil, "card or deck mismatch")
        try check(hex(input.code.revision, 40) && ["dev", "release"].contains(input.code.build_profile), "code metadata")
        for item in [input.code.executable, input.code.replay_source] {
            try check(hex(item.sha256, 64) && item.bytes > 0 && item.bytes <= 512 * 1024 * 1024, "code digest or size")
        }
        try check(input.preparation.path == "\(runPath)/source-preparation/preparation.json" &&
            input.prepared_source == "\(runPath)/source-preparation/fixture" && input.package.path == "\(artifacts)/input.colpkg" &&
            input.parent_source.hasPrefix("build/") && !input.parent_source.hasPrefix("\(runPath)/"), "source location")
        try fixture(request.root, input.parent_source, input.parent_files, withMedia: false)
        try fixture(request.root, input.prepared_source, input.prepared_files, withMedia: true)
        for item in [input.preparation, input.package] {
            let actual = try read(request.root, item.path)
            try check(NativeReviewIdentity.of(actual) == item.identity, "source digest mismatch")
        }
        try check(input.package.identity == artifactIDs["input.colpkg"], "package digest mismatch")
        let preparationObject = try object(read(request.root, input.preparation.path), keys: "schema_version operation stage run platform source prepared_source clock generator_request parent_files prepared_files source_facts preset_cap_answer_time_secs media row_changes selected_parent_deck preserved_card preserved_history media_pattern derivation remaining", integersOnly: false)
        let generation = preparationObject["generator_request"] as? [String: Any]
        let source = preparationObject["source_facts"] as? [String: Any]
        try check(preparationObject["run"] as? String == runPath && preparationObject["platform"] as? String == input.platform &&
            preparationObject["source"] as? String == input.parent_source && preparationObject["prepared_source"] as? String == input.prepared_source &&
            preparationObject["preset_cap_answer_time_secs"] as? UInt32 == input.answer.preset_cap_answer_time_secs &&
            generation?["seed"] as? UInt64 == input.generator_seed && source?["card_id"] as? Int64 == input.chosen.card_id &&
            source?["deck_id"] as? Int64 == input.chosen.deck_id && source?["deck_name"] as? String == input.chosen.deck_name,
            "preparation mismatch")
        try check(try identities(preparationObject["parent_files"]) == input.parent_files &&
            identities(preparationObject["prepared_files"]) == input.prepared_files, "preparation file digests")
        let clockBytes = try JSONSerialization.data(withJSONObject: preparationObject["clock"] as Any)
        try check(try JSONDecoder().decode(Wall.self, from: clockBytes) == input.wall, "preparation clock mismatch")
        guard let counts = source?["row_counts"] as? [String: UInt64], let reviewCount = counts["revlog"] else {
            throw NativeReviewInput.Refusal(reason: "source review count missing")
        }
        try check(counts["notes"] == 1 && counts["cards"] == 1 && source?["media_files"] as? Int == 3 &&
            source?["media_bytes"] as? UInt64 == input.mediaBytes, "source counts mismatch")
        return Loaded(input: input, identity: NativeReviewIdentity.of(inputBytes), sourceReviews: reviewCount)
    }

    static func check(_ condition: Bool, _ reason: String) throws {
        if !condition { throw NativeReviewInput.Refusal(reason: reason) }
    }
    static func hex(_ value: String, _ count: Int) -> Bool {
        value.utf8.count == count && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    static func component(_ value: String) -> Bool {
        let bytes = Array(value.utf8)
        return !bytes.isEmpty && bytes.count <= 120 && bytes[0].isASCIIAlphanumeric &&
            bytes.allSatisfy { $0.isASCIIAlphanumeric || [45, 46, 95].contains($0) }
    }
    private static func fields(_ raw: Any?, _ names: String) throws {
        guard let raw = raw as? [String: Any] else { throw NativeReviewInput.Refusal(reason: "missing object") }
        try check(Set(raw.keys) == Set(names.split(separator: " ").map(String.init)), "record fields")
    }
    private static func identities(_ raw: Any?) throws -> [String: NativeReviewIdentity] {
        guard let raw = raw as? [String: Any], raw.count <= 18 else { throw NativeReviewInput.Refusal(reason: "identity map bound") }
        for identity in raw.values { try fields(identity, "sha256 bytes") }
        let values = try JSONDecoder().decode([String: NativeReviewIdentity].self, from: JSONSerialization.data(withJSONObject: raw))
        for v in values.values { try check(hex(v.sha256, 64) && v.bytes <= 512 * 1024 * 1024, "file identity") }
        return values
    }
    private static func object(_ data: Data, keys: String, integersOnly: Bool = true) throws -> [String: Any] {
        // Foundation's decoder otherwise accepts repeated keys. F11.2 refuses
        // them, including escaped spellings; JSONDecoder checks the syntax.
        let bytes = Array(data); var stack: [Set<String>?] = []; var i = 0
        while i < bytes.count {
            switch bytes[i] {
            case 123: stack.append([])
            case 91: stack.append(nil)
            case 125, 93: if !stack.isEmpty { stack.removeLast() }
            case 45, 48...57:
                let start = i
                while i + 1 < bytes.count && ([43, 45, 46, 69, 101].contains(bytes[i + 1]) || (48...57).contains(bytes[i + 1])) { i += 1 }
                if integersOnly {
                    try check(!bytes[start...i].contains(where: { [46, 69, 101].contains($0) }), "integer input required")
                }
            case 34:
                let start = i; i += 1
                while i < bytes.count && bytes[i] != 34 { if bytes[i] == 92 { i += 1 }; i += 1 }
                let end = i + 1; var next = end
                while next < bytes.count && [9, 10, 13, 32].contains(bytes[next]) { next += 1 }
                if next < bytes.count && bytes[next] == 58, !stack.isEmpty, stack[stack.count - 1] != nil {
                    let key = try JSONDecoder().decode(String.self, from: Data(bytes[start..<min(end, bytes.count)]))
                    try check(stack[stack.count - 1]!.insert(key).inserted, "duplicate record key")
                }
            default: break
            }
            try check(stack.count <= 32, "record nesting bound"); i += 1
        }
        let raw = try JSONSerialization.jsonObject(with: data)
        try fields(raw, keys)
        return raw as! [String: Any]
    }
    private static func fixture(_ root: URL, _ source: String, _ files: [String: NativeReviewIdentity], withMedia: Bool) throws {
        let expected = ["collection.anki2", "manifest.json"] + (withMedia ? media.map { "collection.media/\($0)" } : [])
        try check(Set(files.keys) == Set(expected), "source file list")
        let path = try scoped(root, source)
        try check(Set(FileManager.default.contentsOfDirectory(atPath: path.path)) == ["collection.anki2", "collection.media", "manifest.json"], "fixture layout")
        let mediaPath = try scoped(root, "\(source)/collection.media")
        try check(Set(FileManager.default.contentsOfDirectory(atPath: mediaPath.path)) == Set(withMedia ? media : []), "media layout")
        for (name, identity) in files { try check(try NativeReviewIdentity.of(read(root, "\(source)/\(name)")) == identity, "fixture changed") }
    }
    static func scoped(_ root: URL, _ relative: String) throws -> URL {
        try check(root.path == root.standardizedFileURL.path && root.path == root.resolvingSymlinksInPath().path, "checkout links")
        let parts = relative.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        try check(!parts.isEmpty && parts.allSatisfy { !$0.isEmpty && $0 != "." && $0 != ".." &&
            !["refs", "oracle", "local-data"].contains($0) && !$0.hasPrefix("similarity") }, "excluded input path")
        var path = root
        for part in [""] + parts {
            if !part.isEmpty { path.append(path: part) }
            var info = stat()
            try check(lstat(path.path, &info) == 0 && (info.st_mode & S_IFMT) != S_IFLNK &&
                      info.st_uid == getuid() && info.st_mode & 0o022 == 0, "link/foreign/writable input")
        }
        return path
    }
    static func read(_ root: URL, _ relative: String, limit: Int = 8 * 1024 * 1024) throws -> Data {
        let path = try scoped(root, relative)
        let descriptor = Darwin.open(path.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW)
        try check(descriptor >= 0, "missing input file")
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? handle.close() }
        var before = stat(), after = stat(), current = stat()
        try check(fstat(descriptor, &before) == 0 && before.st_mode & S_IFMT == S_IFREG &&
                  before.st_size >= 0 && before.st_size <= limit, "input file bound")
        let data = try handle.read(upToCount: limit + 1) ?? Data()
        try check(fstat(descriptor, &after) == 0 && lstat(path.path, &current) == 0 &&
                  before.st_dev == after.st_dev && before.st_ino == after.st_ino && before.st_size == after.st_size &&
                  before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec && before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec &&
                  before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec && before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec &&
                  current.st_dev == before.st_dev && current.st_ino == before.st_ino && data.count == before.st_size, "input changed while reading")
        return data
    }
}
private extension UInt8 {
    nonisolated var isASCIIAlphanumeric: Bool { (48...57).contains(self) || (65...90).contains(self) || (97...122).contains(self) }
}
