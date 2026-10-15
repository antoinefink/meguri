#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import SQLite3
import Testing

@testable import Meguri

/// The generated recipe directories `MEGURI_RECIPE_FIXTURES` names; nil when
/// it names none. Outside the suite: its own traits cannot refer to it.
private let recipeFixtureDirectories: [String]? = {
    let paths = (ProcessInfo.processInfo.environment["MEGURI_RECIPE_FIXTURES"] ?? "").split(separator: ":").map(String.init)
    return paths.isEmpty ? nil : paths
}()

/// The generated UI recipes (U11, R1963) launched through `-MeguriFixture`
/// at their pinned instants, in the app process: the hooks install the
/// generator's output file for file, the clock stands at the recipe's
/// instant in its offset, and the installed database opens with the rows
/// and media its manifest counts.
///
/// `tools/mg fixtures-check` generates the recipes and runs this suite with
/// `MEGURI_RECIPE_FIXTURES`, their directories separated by `:`; without it
/// the suite is skipped.
@MainActor
@Suite(.serialized, .enabled(if: recipeFixtureDirectories != nil, "MEGURI_RECIPE_FIXTURES names no recipe directory"))
struct RecipeFixtureTests {
    /// `manifest.json` of a generated directory (the generator's F10.2).
    struct Manifest: Decodable {
        struct Counts: Decodable {
            var notes, cards, revlog, notetypes, decks, presets, tags: Int
            var mediaFiles, mediaBytes: Int

            /// Rows by the table that holds them.
            var rows: [String: Int] {
                [
                    "notes": notes, "cards": cards, "revlog": revlog, "notetypes": notetypes,
                    "decks": decks, "deck_config": presets, "tags": tags,
                ]
            }
        }

        var kind: String
        var name: String
        var nowMs: Int64
        var utcOffsetSecs: Int
        var counts: Counts
    }

    @Test(arguments: recipeFixtureDirectories ?? [])
    func installsTheRecipeAtItsPinnedInstant(path: String) throws {
        let source = URL(filePath: path, directoryHint: .isDirectory)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let manifest = try decoder.decode(Manifest.self, from: Data(contentsOf: source.appending(path: "manifest.json")))
        let recipe = Comment(rawValue: "recipe \(manifest.name)")
        #expect(manifest.kind == "recipe", recipe)
        let now = Date(timeIntervalSince1970: Double(manifest.nowMs) / 1000)
        let zone = try #require(Self.zone(offset: manifest.utcOffsetSecs), "no whole-hour zone for \(manifest.utcOffsetSecs) s")

        let scratch = FileManager.default.temporaryDirectory.appending(
            path: "RecipeFixtureTests-\(UUID().uuidString)", directoryHint: .isDirectory
        )
        defer { try? FileManager.default.removeItem(at: scratch) }
        // -MeguriTimeZone sets the process zone; later tests get theirs back.
        let processZone = (NSTimeZone.default, ProcessInfo.processInfo.environment["TZ"])
        defer { Self.restore(processZone) }

        var configuration = LaunchConfiguration()
        configuration.collectionLocation = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))
        let session = TestHooks.prepare(
            &configuration,
            arguments: [
                "Meguri",
                "-MeguriFixture", source.path(percentEncoded: false),
                "-MeguriNow", Self.launchValue(now, offset: manifest.utcOffsetSecs),
                "-MeguriTimeZone", zone.identifier,
            ]
        )
        try #require(session.error == nil, "\(recipe): \(session.error?.message ?? "")")

        // The pinned clock, in the collection's offset.
        #expect(configuration.clock.now() == now, recipe)
        #expect(configuration.timeZone?.secondsFromGMT(for: now) == manifest.utcOffsetSecs, recipe)

        // The generated directory, installed file for file.
        let installed = configuration.collectionLocation
        #expect(try Self.files(under: installed.directory) == Self.files(under: source), recipe)

        // The installed database opens with the manifest's rows and media.
        let database = try Self.read(installed.collectionFile, tables: Array(manifest.counts.rows.keys))
        #expect(database.version == 18, recipe)
        #expect(database.rows == manifest.counts.rows, recipe)
        let media = try Self.files(under: installed.directory.appending(path: "collection.media", directoryHint: .isDirectory))
        #expect(media.count == manifest.counts.mediaFiles, recipe)
        #expect(media.values.reduce(0) { $0 + $1.count } == manifest.counts.mediaBytes, recipe)
    }

    /// An IANA zone with this fixed offset: `UTC`, or `Etc/GMT∓h` (whose
    /// sign is reversed) for a whole number of hours.
    static func zone(offset: Int) -> TimeZone? {
        guard offset != 0 else { return TimeZone(identifier: "UTC") }
        guard offset % 3600 == 0 else { return nil }
        let hours = offset / 3600
        return TimeZone(identifier: "Etc/GMT\(hours > 0 ? "-" : "+")\(abs(hours))")
    }

    /// `now` as `-MeguriNow` takes it: local time in the offset, then the
    /// offset (`Z` or `±HH:MM`).
    static func launchValue(_ now: Date, offset: Int) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = TimeZone(secondsFromGMT: offset)
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        guard offset != 0 else { return formatter.string(from: now) + "Z" }
        let minutes = abs(offset) / 60
        return formatter.string(from: now) + String(format: "%@%02d:%02d", offset > 0 ? "+" : "-", minutes / 60, minutes % 60)
    }

    static func restore(_ saved: (TimeZone, String?)) {
        if let tz = saved.1 {
            setenv("TZ", tz, 1)
        } else {
            unsetenv("TZ")
        }
        tzset()
        NSTimeZone.resetSystemTimeZone()
        NSTimeZone.default = saved.0
    }

    /// Every regular file under `directory`, by relative path.
    static func files(under directory: URL) throws -> [String: Data] {
        var result: [String: Data] = [:]
        let root = directory.standardizedFileURL.pathComponents.count
        let entries = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: [.isRegularFileKey])
        while let url = entries?.nextObject() as? URL {
            guard try url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile == true else { continue }
            result[url.standardizedFileURL.pathComponents.dropFirst(root).joined(separator: "/")] = try Data(contentsOf: url)
        }
        return result
    }

    struct DatabaseError: Error, CustomStringConvertible {
        let description: String
    }

    /// `col.ver` and the rows of `tables`, read with the system SQLite from
    /// the installed copy (a WAL database cannot be opened read-only before
    /// its shared-memory file exists). The schema's `unicase` collation is
    /// registered so that opening an index on a name column cannot fail;
    /// nothing here compares with it.
    static func read(_ file: URL, tables: [String]) throws -> (version: Int, rows: [String: Int]) {
        var db: OpaquePointer?
        guard sqlite3_open_v2(file.path(percentEncoded: false), &db, SQLITE_OPEN_READWRITE, nil) == SQLITE_OK else {
            defer { sqlite3_close(db) }
            throw DatabaseError(description: "cannot open \(file.lastPathComponent): \(String(cString: sqlite3_errmsg(db)))")
        }
        defer { sqlite3_close(db) }
        sqlite3_create_collation_v2(db, "unicase", SQLITE_UTF8, nil, { _, _, _, _, _ in 0 }, nil)
        func integer(_ sql: String) throws -> Int {
            var statement: OpaquePointer?
            defer { sqlite3_finalize(statement) }
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, sqlite3_step(statement) == SQLITE_ROW else {
                throw DatabaseError(description: "\(sql): \(String(cString: sqlite3_errmsg(db)))")
            }
            return Int(sqlite3_column_int64(statement, 0))
        }
        var rows: [String: Int] = [:]
        for table in tables {
            rows[table] = try integer("SELECT count(*) FROM \(table)")
        }
        return (try integer("SELECT ver FROM col"), rows)
    }
}
#endif
