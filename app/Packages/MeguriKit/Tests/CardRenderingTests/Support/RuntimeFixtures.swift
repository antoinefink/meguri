import CardRendering
import Foundation
import Instrumentation
import Testing

extension WebTestHarness {
    /// A harness with one open page, closed after `body`.
    static func withPage<T>(
        platform: CardRuntime.Platform = .mac, appearance: CardRuntime.Appearance = .light, reviewExtra: String = "",
        size: CGSize = WebTestHarness.size, log: Bool = true, _ body: (WebTestHarness, TestPage) async throws -> T
    ) async throws -> T {
        let harness = WebTestHarness(platform: platform, appearance: appearance, reviewExtra: reviewExtra)
        defer { harness.close() }
        let page = try await harness.openPage(size: size)
        if log { try await page.installLog() }
        return try await body(harness, page)
    }

    /// The `error` signals (C12) the pages sent.
    var diagnostics: [Signal] { signals("error") }

    /// Messages to U12's confirmation handler, parsed as its contract does.
    var confirmations: [VisibleConfirmation.Message] {
        signals.messages(to: CardRuntime.confirmationHandlerName).compactMap { VisibleConfirmation.Message(body: $0.body) }
    }
}

extension TestPage {
    /// Shows `question` in this page, presented, and waits until its shown
    /// phase has finished.
    func showPresentedQuestion(_ question: String, answer: String = "", bodyClass: String = "card card1") async throws {
        try await present()
        let shown = harness.signals("shown", from: self).count
        try await showQuestion(question, answer: answer, bodyClass: bodyClass)
        try await harness.signal("shown", count: shown + 1, from: self)
    }

    /// Shows `answer` in this presented page and waits until its shown phase
    /// has finished.
    func showPresentedAnswer(_ answer: String, bodyClass: String? = nil) async throws {
        let shown = harness.signals("shown", from: self).count
        try await showAnswer(answer, bodyClass: bodyClass)
        try await harness.signal("shown", count: shown + 1, from: self)
    }

    /// Waits until the log has at least `count` entries and returns it.
    func log(count: Int, timeout: Duration = .seconds(10)) async throws -> [String] {
        var entries: [String] = []
        try await harness.until("\(count) log entries", timeout: timeout) {
            entries = try await log()
            return entries.count >= count
        }
        return entries
    }
}

/// The synthetic cases under `Fixtures/runtime/` (C16).
enum RuntimeFixture {
    static func text(_ name: String) throws -> String {
        let dot = name.lastIndex(of: ".")!
        let url = try #require(
            Bundle.module.url(
                forResource: String(name[..<dot]), withExtension: String(name[name.index(after: dot)...]),
                subdirectory: "Fixtures/runtime"))
        return try String(contentsOf: url, encoding: .utf8)
    }
}
