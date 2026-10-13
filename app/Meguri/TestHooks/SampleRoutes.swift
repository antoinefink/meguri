#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import CryptoKit
import DesignSystem
import SwiftUI
#if !os(macOS)
import UIKit
#endif

extension TestHooks {
    /// Three sample routes, one per presentation kind (U7): a screen, a sheet
    /// and an error state. The sheet shows what the launch overrides did, read
    /// back from the app rather than from the options, so tests can check them.
    enum SampleRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("sample/screen") { _ in
                .screen {
                    Text("Sample screen")
                        .accessibilityIdentifier("sample.screen.title")
                }
            }
            registry.register("sample/sheet") { context in
                .sheet {
                    SampleSheet(configuration: context.configuration)
                }
            }
            registry.register("sample/alert") { _ in
                .alert(title: "Sample error", message: "The sample error state.") {
                    Button("Try Again") {}
                    Button("Cancel", role: .cancel) {}
                }
            }
        }
    }

    struct SampleSheet: View {
        let configuration: LaunchConfiguration
        @Environment(\.appClock) private var clock
        @Environment(\.timeZone) private var timeZone
        @Environment(\.locale) private var locale
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
        @Environment(\.dismiss) private var dismiss
        @State private var coreFeatures = "loading"

        var body: some View {
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    Text("Sample sheet")
                        .font(.headline)
                        .accessibilityIdentifier("sample.sheet.title")
                    row("Clock", "sample.sheet.clock", clock.now().formatted(Date.ISO8601FormatStyle(timeZoneSeparator: .colon, timeZone: timeZone)))
                    row("Date", "sample.sheet.date", clock.now().formatted(Date.FormatStyle(date: .complete, time: .omitted, locale: locale, timeZone: timeZone)))
                    row("Time zone", "sample.sheet.timeZone", timeZone.identifier)
                    row("Process time zone", "sample.sheet.processTimeZone", TimeZone.current.identifier)
                    row("Locale", "sample.sheet.locale", locale.identifier)
                    row("Fixture", "sample.sheet.fixture", Self.describe(configuration.collectionLocation))
                    row("Reduce Motion", "sample.sheet.reduceMotion", reduceMotion ? "on" : "off")
                    row("Reduce Transparency", "sample.sheet.reduceTransparency", reduceTransparency ? "on" : "off")
                    #if !os(macOS)
                    row("Animations", "sample.sheet.animations", UIView.areAnimationsEnabled ? "on" : "off")
                    #endif
                    row("Ports", "sample.sheet.ports", ports)
                    row("Core features", "sample.sheet.coreFeatures", coreFeatures)
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("sample.sheet.done")
                }
                .padding()
            }
            #if os(macOS)
            .frame(minWidth: 520, minHeight: 560)
            #endif
            .task {
                do {
                    coreFeatures = try await Core.buildInfo().features.joined(separator: ",")
                } catch {
                    coreFeatures = "error: \(error)"
                }
            }
        }

        private var ports: String {
            let ankiConnect = configuration.ankiConnectPort.map(String.init) ?? "default"
            let migaku = configuration.migakuPort.map(String.init) ?? "default"
            return "ankiconnect \(ankiConnect), migaku \(migaku)"
        }

        private func row(_ title: String, _ identifier: String, _ value: String) -> some View {
            HStack(alignment: .firstTextBaseline) {
                Text(title)
                Spacer()
                Text(value)
                    .accessibilityIdentifier(identifier)
            }
        }

        /// Each file in the collection directory: relative path, size and the
        /// first 12 hex digits of its SHA-256, sorted; `none` when empty.
        static func describe(_ collection: CollectionLocation) -> String {
            let root = collection.directory.standardizedFileURL
            guard let enumerator = FileManager.default.enumerator(at: root, includingPropertiesForKeys: [.isRegularFileKey]) else {
                return "none"
            }
            var entries: [String] = []
            for case let url as URL in enumerator {
                guard (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true,
                      let data = try? Data(contentsOf: url)
                else { continue }
                let relative = url.standardizedFileURL.pathComponents.dropFirst(root.pathComponents.count).joined(separator: "/")
                let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined().prefix(12)
                entries.append("\(relative) \(data.count) \(digest)")
            }
            return entries.isEmpty ? "none" : entries.sorted().joined(separator: "; ")
        }
    }
}
#endif
