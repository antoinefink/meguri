import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// Type-in and Enter (specs/render/card-runtime.md C6.4, C10.3, C10.4,
    /// C16.6; R508, R534–R536; AE17 IME part). Events are synthetic and
    /// dispatched on the question's own elements.
    @Suite struct TypeInRuntimeTests {
        /// A question without `{{type:}}` whose field calls `_typeAnsPress`
        /// from its key handler, as templates do.
        static let question = #"<p>q</p><input id="k" onkeypress="_typeAnsPress()">"#

        /// Dispatches `events` (`"keydown 13"`, `"keypress Enter"`,
        /// `"compositionstart"`, `"keydown 229 composing"`, ...) on `#k`.
        static func dispatch(_ page: TestPage, _ events: [String]) async throws {
            try await page.run(
                """
                var target = document.getElementById("k");
                events.forEach(function (spec) {
                  var parts = spec.split(" ");
                  var type = parts[0], event;
                  if (type.indexOf("composition") === 0) {
                    event = new CompositionEvent(type, { bubbles: true, data: "" });
                  } else {
                    var code = parts[1] === "Enter" ? 13 : Number(parts[1]);
                    var key = parts[1] === "Enter" || code === 13 ? "Enter" : (code === 229 ? "Process" : parts[1]);
                    event = new KeyboardEvent(type, { bubbles: true, cancelable: true, key: key, isComposing: parts[2] === "composing" });
                    var keyCode = isNaN(code) ? key.charCodeAt(0) : code;
                    Object.defineProperty(event, "keyCode", { value: keyCode });
                    Object.defineProperty(event, "which", { value: keyCode });
                  }
                  target.dispatchEvent(event);
                });
                """, ["events": events])
        }

        /// The reveal requests the page made (C10.4: `command` with `reveal`).
        static func reveals(_ harness: WebTestHarness) -> Int {
            harness.signals("command").filter { $0["reveal"] as? Bool == true }.count
        }

        /// Shows the question, presented, dispatches `events` and returns how
        /// many reveals they requested.
        static func revealsAfter(_ events: [String], on page: TestPage, _ harness: WebTestHarness) async throws -> Int {
            let before = reveals(harness)
            try await dispatch(page, events)
            // Signals travel through the interaction world: give them time.
            try await Task.sleep(for: .milliseconds(150))
            return reveals(harness) - before
        }

        // MARK: TI1, D14

        @Test func enterRevealsWithoutBridge() async throws {
            for platform in [CardRuntime.Platform.mac, .iPhone] {
                try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                    try await page.announce(card: 3, generation: 1)
                    try await page.showPresentedQuestion(Self.question)
                    #expect(try await Self.revealsAfter(["keydown Enter", "keypress Enter"], on: page, harness) == 1)
                    let reveal = try #require(harness.signals("command").first)
                    #expect(reveal.side == "question" && reveal.card == 3)
                }
            }
        }

        // MARK: TI2–TI6, D13, AE17 IME part

        @Test func imeEnterDoesNotReveal() async throws {
            let rows: [(events: [String], reveals: Int)] = [
                (["compositionstart", "keypress Enter"], 0),
                (["compositionstart", "keydown 229", "compositionend", "keypress Enter"], 0),
                (["compositionstart", "compositionend", "keydown 229", "keypress Enter"], 0),
                (["compositionstart", "compositionend", "keydown Enter", "keypress Enter"], 1),
                (["compositionstart", "compositionend", "keyup Enter", "keydown Enter", "keypress Enter"], 1),
                // The event itself flags IME input.
                (["keydown 229", "keypress Enter composing"], 0),
            ]
            for row in rows {
                try await WebTestHarness.withPage(log: false) { harness, page async throws in
                    try await page.showPresentedQuestion(Self.question)
                    #expect(try await Self.revealsAfter(row.events, on: page, harness) == row.reveals, "\(row.events)")
                }
            }
        }

        /// AE17: the Enter that commits the composition does not reveal; the
        /// next one does.
        @Test func commitEnterThenEnterRevealsOnce() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(Self.question)
                let committing = ["compositionstart", "keydown 229 composing", "keyup 229", "keydown 229 composing", "compositionend", "keypress Enter"]
                #expect(try await Self.revealsAfter(committing, on: page, harness) == 0)
                #expect(try await Self.revealsAfter(["keyup Enter", "keydown Enter", "keypress Enter"], on: page, harness) == 1)
            }
        }

        // MARK: TI7

        @Test func otherKeysDoNothing() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(Self.question)
                #expect(try await Self.revealsAfter(["keydown a", "keypress a"], on: page, harness) == 0)
            }
        }

        // MARK: TI8

        @Test func typeAnsPressOutsideAnEventThrows() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(Self.question)
                let failureName = try await page.run("try { _typeAnsPress(); return 'none'; } catch (e) { return e.name; }")
                #expect(failureName as? String == "TypeError")
            }
        }

        // MARK: TI9

        @Test func enterOnTheAnswerDoesNotReveal() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(Self.question)
                try await page.showPresentedAnswer(Self.question)
                #expect(try await Self.revealsAfter(["keydown Enter", "keypress Enter"], on: page, harness) == 0)
            }
        }

        // MARK: TI10, R534

        @Test func typeansIsFocusedAtPresentation() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showQuestion(#"<p>q</p><input id="typeans">"#)
                try await harness.signal("settled", from: page)
                #expect(try await page.string("document.activeElement && document.activeElement.id") != "typeans")
                try await page.present()
                try await harness.signal("shown", from: page)
                #expect(try await page.string("document.activeElement.id") == "typeans")
            }
        }

        // MARK: TI11, R536

        @Test func typedAnswerOutlivesTheQuestionsInput() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<input id="typeans">"#)
                try await page.run(
                    #"var t = document.getElementById("typeans"); t.value = "abc"; t.dispatchEvent(new InputEvent("input", { bubbles: true }));"#)
                #expect(try await page.string("getTypedAnswer()") == "abc")
                let typed = try await harness.signal("typedText", from: page)
                #expect(typed["text"] as? String == "abc")
                try await page.showPresentedAnswer(#"<input id="typeans" value="new"><hr id="answer">"#)
                #expect(try await page.string("getTypedAnswer()") == "abc")
            }
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                #expect(try await page.string("getTypedAnswer()") == "null")
                try await page.showPresentedQuestion("<p>no type-in</p>")
                #expect(try await page.string("getTypedAnswer()") == "null")
            }
        }

        // MARK: TI12

        @Test func typeansThatIsNotAFieldHasNoTypedAnswer() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<div id="typeans">x</div>"#)
                #expect(try await page.string("getTypedAnswer()") == "null")
            }
        }

        // MARK: C14: interaction signals

        @Test func focusCompositionAndSelectionAreReported() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p id="p">some text</p><input id="typeans"><textarea id="t"></textarea>"#)
                let focusIn = try await harness.signal("focus", from: page)
                #expect(focusIn["editable"] as? Bool == true && focusIn["typeans"] as? Bool == true)
                try await page.run(#"document.getElementById("t").focus();"#)
                let other = try await harness.signal("focus", count: 2, from: page)
                #expect(other["editable"] as? Bool == true && other["typeans"] as? Bool == false)
                try await page.run(#"document.getElementById("t").blur();"#)
                let out = try await harness.signal("focus", count: 3, from: page)
                #expect(out["editable"] as? Bool == false)

                try await page.run(#"document.getElementById("t").dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));"#)
                #expect(try await harness.signal("composition", from: page)["active"] as? Bool == true)
                try await page.run(#"document.getElementById("t").dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));"#)
                #expect(try await harness.signal("composition", count: 2, from: page)["active"] as? Bool == false)

                try await page.run(#"getSelection().selectAllChildren(document.getElementById("p"));"#)
                #expect(try await harness.signal("selection", from: page)["selected"] as? Bool == true)
                try await page.run("getSelection().removeAllRanges();")
                #expect(try await harness.signal("selection", count: 2, from: page)["selected"] as? Bool == false)
            }
        }

        @Test func failedMediaLoadsAreReported() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<img src="_missing.png"><p>x</p>"#)
                let failed = try await harness.signal("mediaError", from: page)
                #expect(failed["tag"] as? String == "img")
                #expect((failed["url"] as? String)?.hasSuffix("/_missing.png") == true)
                #expect(failed.side == "question")
                // The preload's own copies are not reported.
                try await Task.sleep(for: .milliseconds(200))
                #expect(harness.signals("mediaError").count == 1)
            }
        }
    }
}
