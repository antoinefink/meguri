import Foundation
import Testing

/// Review tap zones (U180): the runtime's `tapTarget` question. A tap
/// listener on the touched element or an ancestor below `<body>` makes it a
/// card control; listeners on the whole page (window, document, `<html>`,
/// `<body>`) do not. With no reply from the runtime the query says yes.
extension WebKitSuites.CardRuntimeSuites {
    @Suite struct TapTargetQueryTests {
        private static let side = #"<div id="outer"><span id="inner">word</span></div><p id="plain">plain</p>"#
            + #"<div id="attr" onclick=""><b id="attrChild">x</b></div><div id="host"></div>"#

        private func tapTarget(_ page: TestPage, _ selector: String) async throws -> Bool {
            try await page.runIsolated("""
                const target = selector === "#shadowChild"
                  ? document.getElementById("host").shadowRoot.getElementById("shadowChild")
                  : document.querySelector(selector);
                const query = new CustomEvent("meguri:query", { detail: "tapTarget", bubbles: true, cancelable: true, composed: true });
                return target.dispatchEvent(query);
                """, ["selector": selector]) as? Bool ?? true
        }

        @Test func plainContentHasNoTapListener() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(Self.side)
                #expect(try await !tapTarget(page, "#plain"))
                #expect(try await !tapTarget(page, "#inner"))
                #expect(try await !tapTarget(page, "body"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        @Test(arguments: ["touchstart", "touchend", "pointerdown", "pointerup", "mousedown", "mouseup", "click"])
        func anElementListenerMakesItAndItsDescendantsControls(_ type: String) async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion(Self.side)
                _ = try await page.run("document.getElementById('outer').addEventListener(type, function () {});", ["type": type])
                #expect(try await tapTarget(page, "#inner"))
                #expect(try await tapTarget(page, "#outer"))
                #expect(try await !tapTarget(page, "#plain"))
            }
        }

        @Test func handlerAttributesAndPropertiesCount() async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion(Self.side)
                #expect(try await tapTarget(page, "#attrChild"))
                _ = try await page.run("document.getElementById('plain').onpointerup = function () {};")
                #expect(try await tapTarget(page, "#plain"))
            }
        }

        @Test func wholePageListenersDoNotMakeAnElementAControl() async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion(Self.side)
                _ = try await page.run("""
                    window.addEventListener('click', function () {});
                    document.addEventListener('touchend', function () {});
                    document.documentElement.addEventListener('pointerup', function () {});
                    document.body.addEventListener('mousedown', function () {});
                    document.body.onclick = function () {};
                    """)
                #expect(try await !tapTarget(page, "#plain"))
                // The C13 question for the fast path still sees them.
                #expect(try await page.hasCardListener("tap", at: "#plain"))
            }
        }

        @Test func shadowRootsAreCrossedToTheirHost() async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion(Self.side)
                _ = try await page.run("""
                    const host = document.getElementById('host');
                    host.attachShadow({ mode: 'open' }).innerHTML = '<i id="shadowChild">s</i>';
                    host.addEventListener('click', function () {});
                    """)
                #expect(try await tapTarget(page, "#shadowChild"))
            }
        }

        @Test func removedListenersStopCounting() async throws {
            try await WebTestHarness.withPage { _, page async throws in
                try await page.showPresentedQuestion(Self.side)
                _ = try await page.run("""
                    const f = function () {};
                    const outer = document.getElementById('outer');
                    outer.addEventListener('click', f);
                    outer.removeEventListener('click', f);
                    """)
                #expect(try await !tapTarget(page, "#inner"))
            }
        }
    }
}
