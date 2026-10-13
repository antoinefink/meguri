#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import Testing

@testable import Meguri

/// The `-Meguri…` launch-option parser (U7, R1963), in the app process.
@MainActor
@Suite struct LaunchOptionsTests {
    typealias Options = TestHooks.LaunchOptions

    @Test func parsesEveryOption() throws {
        let options = try Options(arguments: [
            "-MeguriFixture", "/tmp/fixture/collection.anki2",
            "-MeguriNow", "2030-01-15T10:00:00",
            "-MeguriTimeZone", "Asia/Tokyo",
            "-MeguriLocale", "en_US",
            "-MeguriAnimations", "off",
            "-MeguriReduceMotion", "on",
            "-MeguriReduceTransparency", "off",
            "-MeguriWindowSize", "1000x700",
            "-MeguriAnkiConnectPort", "18765",
            "-MeguriMigakuPort", "54432",
            "-MeguriRoute", "review/answer",
        ])
        #expect(options.fixture == URL(filePath: "/tmp/fixture/collection.anki2"))
        // 10:00 in Tokyo (UTC+9) is 01:00 UTC.
        #expect(options.now == Date(timeIntervalSince1970: 1_894_669_200))
        #expect(options.timeZone?.identifier == "Asia/Tokyo")
        #expect(options.locale?.identifier == "en_US")
        #expect(options.animationsEnabled == false)
        #expect(options.reduceMotion == true)
        #expect(options.reduceTransparency == false)
        #expect(options.windowSize == .explicit(width: 1000, height: 700))
        #expect(options.ankiConnectPort == 18765)
        #expect(options.migakuPort == 54432)
        #expect(options.route?.path == "review/answer")
    }

    @Test func leavesOtherArgumentsToTheSystem() throws {
        let options = try Options(arguments: ["-AppleLanguages", "(en)", "-NSDocumentRevisionsDebugMode", "YES", "plain"])
        #expect(options == Options())
    }

    @Test(arguments: [
        ("2030-01-15T10:00:00Z", 1_894_701_600.0),
        ("2030-01-15T10:00:00+05:30", 1_894_681_800.0),
        ("2030-01-15T10:00:00-08:00", 1_894_730_400.0),
    ])
    func zoneDesignatorsWinOverTheLaunchZone(value: String, seconds: Double) throws {
        let options = try Options(arguments: ["-MeguriTimeZone", "Asia/Tokyo", "-MeguriNow", value])
        #expect(options.now == Date(timeIntervalSince1970: seconds))
    }

    @Test func namedWindowSizes() throws {
        #expect(try Options(arguments: ["-MeguriWindowSize", "minimum"]).windowSize == .minimum)
        #expect(try Options(arguments: ["-MeguriWindowSize", "default"]).windowSize == .standard)
        #expect(try Options(arguments: ["-MeguriWindowSize", "large"]).windowSize == .large)
    }

    @Test func theWindowSizeIsMacOnly() throws {
        let options = try Options(arguments: ["-MeguriWindowSize", "large"])
        var configuration = LaunchConfiguration()
        let apply = {
            try options.apply(to: &configuration, installer: TestHooks.FixtureInstaller(bundleIdentifier: "x.dev"), routes: .standard)
        }
        #if os(macOS)
        try apply()
        #else
        #expect(throws: TestHookError("-MeguriWindowSize applies to the Mac only"), performing: apply)
        #endif
    }

    @Test(arguments: [
        (["-MeguriBogus", "1"], "unknown launch option -MeguriBogus"),
        (["-MeguriRoute", "a/b", "-MeguriRoute", "c/d"], "-MeguriRoute is given twice"),
        (["-MeguriRoute"], "-MeguriRoute needs a value"),
        (["-MeguriFixture", "relative/collection.anki2"], "-MeguriFixture needs an absolute path"),
        (["-MeguriNow", "2030-01-15 10:00:00"], "-MeguriNow takes"),
        (["-MeguriNow", "2030-02-30T10:00:00Z"], "-MeguriNow takes"),
        (["-MeguriNow", "2030-01-15T10:00:00+25:00"], "-MeguriNow takes"),
        (["-MeguriTimeZone", "Mars/Olympus"], "unknown time zone 'Mars/Olympus'"),
        (["-MeguriLocale", "xx_NOPE"], "unknown locale 'xx_NOPE'"),
        (["-MeguriAnimations", "yes"], "-MeguriAnimations takes on or off"),
        (["-MeguriReduceMotion", "1"], "-MeguriReduceMotion takes on or off"),
        (["-MeguriAnkiConnectPort", "0"], "-MeguriAnkiConnectPort takes a port from 1 to 65535"),
        (["-MeguriMigakuPort", "70000"], "-MeguriMigakuPort takes a port from 1 to 65535"),
        (["-MeguriWindowSize", "1000x"], "-MeguriWindowSize takes minimum, default, large or <width>x<height>"),
        (["-MeguriWindowSize", "0x700"], "-MeguriWindowSize takes"),
        (["-MeguriRoute", "sample"], "is not a route"),
        (["-MeguriRoute", "Sample/Sheet"], "is not a route"),
        (["-MeguriRoute", "sample//sheet"], "is not a route"),
    ])
    func rejectsBadArguments(arguments: [String], message: String) {
        #expect {
            try Options(arguments: arguments)
        } throws: { error in
            (error as? TestHookError)?.message.contains(message) == true
        }
    }
}
#endif
