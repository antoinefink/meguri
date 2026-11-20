import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// The card page never scrolls sideways for a card that fits: no
    /// horizontal scroll bar at any width, with or without a vertical scroll
    /// bar that takes room. A card whose own content is wider scrolls
    /// sideways and stays reachable (native-interface-contract N16.4).
    @Suite struct HorizontalOverflowTests {
        /// The stock note types' card CSS (Meguri's own, `card.css`).
        static let stockCSS = """
            .card { font-family: system-ui, -apple-system, "Helvetica Neue", sans-serif; font-size: 1.3em; line-height: 1.45;
              text-align: center; color: var(--fg, #1d1d1f); background-color: var(--canvas, #f5f5f7); }
            .card.nightMode, .nightMode .card { color: var(--fg, #f5f5f7); background-color: var(--canvas, #1d1d1f); }
            .card .prompt, .card .response, .card .extra { margin: 0.4em auto; max-width: 40em; }
            """
        /// Front and back as the Basic and type-in templates render them,
        /// with a replay button, a long word, a table and a code block.
        static let front = """
            <style>\(stockCSS)</style><div class="prompt">Front pneumonoultramicroscopicsilicovolcanoconiosis \
            <a class="replay-button soundLink" href=#><svg class="playImage" viewBox="0 0 48 48"><rect x="3" y="3" width="42" \
            height="42" rx="12"/><path d="M18 14L34 24L18 34Z"/></svg></a></div>
            <center>
            <input type=text id=typeans autocapitalize="off" autocorrect="off" spellcheck="false" onkeypress="_typeAnsPress();"
               style="font-family: 'Arial'; font-size: 20px;">
            </center>
            """
        static func back(tall: Bool) -> String {
            let filler = tall ? String(repeating: "<p>A line of the answer.</p>", count: 60) : ""
            return """
                \(front)

                <hr id=answer>

                <div class="response">Back <table><tr><td>a</td><td>b</td></tr></table><pre>code</pre></div>\(filler)
                """
        }
        /// Scroll bars that take room, as with "Show scroll bars: Always" or
        /// a mouse attached, whatever this host's own setting.
        static let classicScrollBars = "::-webkit-scrollbar { width: 15px; height: 15px; }"

        static func metrics(_ page: TestPage) async throws -> (scroll: Int, client: Int, inner: Int) {
            let values = try await page.json([Int].self, """
                const e = document.scrollingElement;
                return JSON.stringify([e.scrollWidth, e.clientWidth, window.innerWidth]);
                """)
            return (values[0], values[1], values[2])
        }

        @Test(arguments: [320.0, 640.0, 999.0, 1000.0, 1600.0])
        func standardCardHasNoHorizontalOverflow(width: Double) async throws {
            for appearance in CardRuntime.Appearance.allCases {
                for classic in [false, true] {
                    for tall in [false, true] {
                        try await WebTestHarness.withPage(appearance: appearance, size: CGSize(width: width, height: 600), log: false) {
                            _, page async throws in
                            if classic {
                                try await page.run("const s = document.createElement('style'); s.textContent = css; document.head.appendChild(s);",
                                                   ["css": Self.classicScrollBars])
                            }
                            try await page.showPresentedQuestion(Self.front)
                            try await page.showPresentedAnswer(Self.back(tall: tall))
                            let m = try await Self.metrics(page)
                            #expect(m.scroll == m.client, "width \(width) \(appearance) classic \(classic) tall \(tall): \(m)")
                            if classic && tall {
                                // The case of the report: a vertical scroll bar takes room.
                                #expect(m.client < m.inner)
                            }
                        }
                    }
                }
            }
        }

        @Test func wideContentScrollsSideways() async throws {
            try await WebTestHarness.withPage(size: CGSize(width: 640, height: 600), log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    "<style>\(Self.stockCSS)</style><table style=\"width: 1200px\"><tr><td>wide</td></tr></table>")
                let m = try await Self.metrics(page)
                #expect(m.scroll > m.client)
                try await page.run("window.scrollTo({left: 10000, behavior: 'instant'});")
                #expect(try await page.string("window.scrollX") == String(m.scroll - m.client))
            }
        }
    }
}
