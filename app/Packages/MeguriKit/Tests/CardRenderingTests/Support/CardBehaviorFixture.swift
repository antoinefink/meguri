@testable import CardRendering
import Foundation
import CoreBridge
import QuartzCore
import Testing
import WebKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

final class BehaviorPresenter: CardBehaviorPresenting {
    var isAvailable = true
    var handledSchemes: Set<String> = ["https", "http", "mkdictionaries"]
    var schemeAvailability: [String: CardURLAvailability] = [:]
    var dialogs: [CardDialog] = []
    var replies: [@MainActor (CardDialogResult) -> Void] = []
    var opens: [CardOpenRequest] = []
    var attachments: [Data] = []
    var dismissals = 0
    func availability(of url: URL) -> CardURLAvailability {
        schemeAvailability[url.scheme?.lowercased() ?? ""] ?? (handledSchemes.contains(url.scheme?.lowercased() ?? "") ? .available : .unavailable)
    }
    func dialog(_ request: CardDialog, completion: @escaping @MainActor (CardDialogResult) -> Void) -> CardBehaviorPresentation? {
        guard isAvailable else { return nil }
        dialogs.append(request); replies.append(completion)
        return CardBehaviorPresentation { [weak self] in self?.dismissals += 1 }
    }
    func open(_ request: CardOpenRequest) -> CardBehaviorPresentation? { opens.append(request); return nil }
    func openAttachmentPDF(_ bytes: Data) -> CardBehaviorPresentation? {
        attachments.append(bytes)
        return CardBehaviorPresentation { [weak self] in self?.dismissals += 1 }
    }
}

final class CardBehaviorFixture {
    let media = MediaFixture()
    let pair: CardPagePair
    let window: SpikeWindow
    let presenter = BehaviorPresenter()
    let configuration = CardWebConfiguration(store: .nonPersistent())

    init(platform: CardRuntime.Platform = PlatformClasses.current, video: Bool = false,
         compare: @escaping (TypeAnswerDescriptor, String) async throws -> String = { _, text in "<code id=comparison>\(text)</code>" }) async throws {
        if video { media.add("/clip.mp4", try await TestMedia.mp4(), "video/mp4") }
        pair = CardPagePair(configuration: configuration, resolver: try await media.resolved(), platform: platform,
                            compare: compare)
        // The host owns its content rectangle; the controller's root view
        // must not undo a live card-rectangle resize during the test.
        let host = CardPlatformView(frame: pair.container.frame)
        host.addSubview(pair.container)
        window = SpikeWindow(host)
        // Pure fixture runs never send unsolicited native accessibility effects.
        for page in pair.pages { page.accessibilityEffects.isActive = { _ in false } }
        pair.behaviorPresenter = presenter
        try await pair.load()
    }
    func close() { pair.close(); window.close() }
    func present(_ payload: CardPayload) async throws {
        let update = try await pair.prepare(payload)
        let stream = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        pair.onCommitted = { committed, _ in if committed == update { stream.continuation.yield(()) } }
        defer { pair.onCommitted = nil; stream.continuation.finish() }
        CATransaction.begin()
        _ = try pair.present(update)
        CATransaction.commit()
        try await Spike.deadline("card behavior commit", timeout: .seconds(3)) {
            for await _ in stream.stream { return }
            throw Spike.Failure("missing card commit")
        }
        try await pair.current.waitForShown(update)
    }
    static func until(_ what: String, _ condition: () async throws -> Bool) async throws {
        let end = ContinuousClock.now.advanced(by: .seconds(3))
        while !(try await condition()) {
            guard ContinuousClock.now < end else { throw Spike.Failure(what) }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    func script(_ source: String) async throws { try await pair.current.run(source) }
    func bool(_ source: String) async throws -> Bool? {
        try await pair.current.webView.callAsyncJavaScript(source, contentWorld: .page) as? Bool
    }
    /// Own offscreen page snapshots, never host screen captures or baselines.
    func snapshot(_ name: String) async throws {
        guard let run = ProcessInfo.processInfo.environment["MG_RUN_DIR"] else { return }
        let directory = URL(fileURLWithPath: run).appending(path: "card-behavior-visual", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let image = try await pair.current.webView.takeSnapshot(configuration: nil)
        #if os(macOS)
        let tiff = try #require(image.tiffRepresentation)
        let bitmap = try #require(NSBitmapImageRep(data: tiff))
        let data = try #require(bitmap.representation(using: .png, properties: [:]))
        #else
        let data = try #require(image.pngData())
        #endif
        try data.write(to: directory.appending(path: name + ".png"))
    }
}
