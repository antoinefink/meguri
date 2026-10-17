import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// Script semantics templates rely on (specs/render/card-runtime.md C6.5,
    /// C11, C12, C16.2, C16.7; S9, S10; R468, R469, R503, R518–R522; AE15).
    @Suite struct ScriptSemanticsTests {
        // MARK: SS1, R468

        @Test func repeatedConstThrowsOnTheSecondSide() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let side = "<script>const x = 1; window.__x = (window.__x||0) + 1;</script>"
                try await page.showPresentedQuestion(side)
                try await page.showPresentedAnswer(side)
                #expect(try await page.string("window.__x") == "1")
                let errors = harness.diagnostics
                #expect(errors.count == 1)
                let error = try #require(errors.first)
                #expect((error["message"] as? String)?.hasPrefix("SyntaxError") == true, "\(error.body)")
                #expect(error["kind"] as? String == "exception")
                #expect(error["phase"] as? String == "script" && error["index"] as? Int == 1)
                #expect(error.side == "answer")
            }
        }

        // MARK: SS2

        @Test func varCounterIncrementsAcrossSidesAndCards() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let side = #"<script>var n = (typeof n === "number" ? n : 0) + 1; log(n)</script>"#
                try await page.showPresentedQuestion(side)
                try await page.showPresentedAnswer(side)
                try await page.showPresentedQuestion(side, bodyClass: "card card2")
                try await page.showPresentedAnswer(side)
                try await page.showPresentedQuestion(side)
                #expect(try await page.log() == ["1", "2", "3", "4", "5"])
            }
        }

        // MARK: SS3, C2.4

        @Test func documentIsCompleteOnEverySide() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let side = """
                    <script>
                    document.addEventListener("DOMContentLoaded", function () { log("dcl"); });
                    window.addEventListener("load", function () { log("load"); });
                    log(document.readyState);
                    </script>
                    """
                try await page.showPresentedQuestion(side)
                try await page.showPresentedAnswer(side)
                try await Task.sleep(for: .milliseconds(300))
                #expect(try await page.log() == ["complete", "complete"])
            }
        }

        // MARK: SS4, R519

        @Test func currentScriptDatasetIsReadable() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<script data-k="v">log(document.currentScript.dataset.k)</script>"#)
                #expect(try await page.log() == ["v"])
            }
        }

        // MARK: SS5, R519

        @Test func onerrorAttributeFiresForAMissingSource() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<script src="_missing.js" onerror="log('E')"></script><script>log('next')</script>"#)
                #expect(try await page.log() == ["E", "next"])
            }
        }

        // MARK: SS6, R467

        @Test func srcScriptRunsBeforeTheNextScript() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_lib.js", text: "window.__lib = 1;")
                harness.media.delay("/_lib.js", by: .milliseconds(150))
                try await page.showPresentedQuestion(#"<script src="_lib.js"></script><script>log(typeof window.__lib)</script>"#)
                #expect(try await page.log() == ["number"])
            }
        }

        // MARK: SS7, R521, R522

        @Test func anExceptionStopsOnlyItsScript() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<script>null.x</script><script>log(2)</script>")
                #expect(try await page.log() == ["2"])
                let errors = harness.diagnostics
                #expect(errors.count == 1)
                let error = try #require(errors.first)
                #expect(error["kind"] as? String == "exception")
                #expect((error["message"] as? String)?.hasPrefix("TypeError: ") == true, "\(error.body)")
                #expect(error["phase"] as? String == "script" && error["index"] as? Int == 1)
                #expect(error["line"] as? Int == 1 && error["source"] as? String == "inline")
                #expect(error.side == "question")
            }
        }

        // MARK: SS8, C6.5

        @Test func promiseJobsRunBetweenScriptsButTimersDoNot() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<script>Promise.resolve().then(function () { log("m"); }); setTimeout(function () { log("t"); });</script><script>log("s2")</script>"#)
                #expect(try await page.log(count: 3) == ["m", "s2", "t"])
            }
        }

        // MARK: SS9, R520

        @Test func dynamicImportResolvesRelativeAndRootPaths() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_m.js", text: "export const v = 7;")
                try await page.showPresentedQuestion(
                    #"<script>import('./_m.js').then(function (m) { log(m.v); }); import('/_m.js').then(function (m) { log(m.v); });</script>"#)
                #expect(try await page.log(count: 2) == ["7", "7"])
            }
        }

        // MARK: SS10, R469

        @Test func frontSideScriptsRunAgainOnTheAnswerWithNewInputs() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let front = #"<input id=fi><script>log(document.getElementById('fi')); window.__inputs = (window.__inputs || []).concat([document.getElementById('fi')]);</script>"#
                try await page.showPresentedQuestion(front)
                try await page.showPresentedAnswer(front + #"<hr id=answer>"#)
                #expect(try await page.log() == ["[object HTMLInputElement]", "[object HTMLInputElement]"])
                #expect(try await page.bool("window.__inputs.length === 2 && window.__inputs[0] !== window.__inputs[1]"))
            }
        }

        // MARK: SS11, R518

        @Test func scriptsRunInSloppyMode() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<script>undeclared = 5; log(this === window)</script>")
                #expect(try await page.log() == ["true"])
                #expect(try await page.string("window.undeclared") == "5")
            }
        }

        // MARK: SS12, D9

        @Test func unfetchedScriptDoesNotBlock() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.hold("/_t.js")
                harness.media.hold("/_n.js")
                try await page.showPresentedQuestion(
                    #"<script type="text/x-tmpl" src="_t.js"></script><script nomodule src="_n.js"></script><script>log("after")</script>"#)
                #expect(try await page.log() == ["after"])
            }
        }

        // MARK: SS12, C6.5, D9: the forms of type and language

        /// `type` and `language` values a `src` script can carry: padded with
        /// whitespace the HTML standard strips from `type` (space, tab, line
        /// feed) and with whitespace it keeps (no-break space, vertical tab),
        /// in upper case, with parameters, legacy language names, and
        /// `language` next to a `type`.
        static let scriptTypeVariants = [
            #"type=" text/javascript ""#,
            "type=\"\ttext/javascript\n\"",
            #"type="TEXT/JavaScript""#,
            #"type="&#xA0;text/javascript""#,
            #"type="text/javascript&#x0B;""#,
            #"type="text/javascript; charset=utf-8""#,
            #"type="javascript""#,
            #"type="  ""#,
            #"type=" module ""#,
            "type=\"\tmodule\n\"",
            #"type="MODULE""#,
            #"type="&#xA0;module""#,
            #"type="module&#x0B;""#,
            #"type="module" nomodule"#,
            #"language="javascript""#,
            #"language="JavaScript""#,
            #"language=" javascript""#,
            #"language="javascript ""#,
            #"language="javascript1.5""#,
            #"language="javascript1.6""#,
            #"language="javascript1.7""#,
            #"language="livescript""#,
            #"language="ecmascript""#,
            #"language="jscript""#,
            #"language="vbscript""#,
            #"language="module""#,
            #"language="""#,
            #"language="javascript" nomodule"#,
            #"type="" language="vbscript""#,
            #"type="text/x-tmpl" language="javascript""#,
        ]

        /// C6.5's waiting rule against the engine itself: the walk waits for a
        /// `src` script exactly when the engine fetches it, which the scheme
        /// handler sees. Waiting for one the engine skips would stop the page
        /// for good (D9); not waiting for one it fetches would run the next
        /// script first.
        @Test func walkWaitsExactlyForTheScriptsTheEngineFetches() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                var side = ""
                for (index, attributes) in Self.scriptTypeVariants.enumerated() {
                    harness.media.add("/_k\(index).js", text: #"log("ran\#(index)");"#)
                    harness.media.delay("/_k\(index).js", by: .milliseconds(40))
                    side += #"<script \#(attributes) src="_k\#(index).js"></script><script>log("next\#(index)")</script>"# + "\n"
                }
                try await page.present()
                try await page.showQuestion(side)
                do {
                    try await harness.signal("shown", from: page, timeout: .seconds(5))
                } catch {
                    let log = try await page.log()
                    let stalled = Self.scriptTypeVariants.indices.first { !log.contains("next\($0)") }.map { Self.scriptTypeVariants[$0] }
                    Issue.record("the walk stopped at <script \(stalled ?? "?") src>: \(log)")
                    return
                }
                let fetched = Self.scriptTypeVariants.indices.filter { !harness.media.requests(for: "/_k\($0).js").isEmpty }
                try await harness.until("every fetched script ran") {
                    let log = try await page.log()
                    return fetched.allSatisfy { log.contains("ran\($0)") }
                }
                let log = try await page.log()
                for (index, attributes) in Self.scriptTypeVariants.enumerated() {
                    let ran = log.firstIndex(of: "ran\(index)")
                    let next = try #require(log.firstIndex(of: "next\(index)"))
                    if fetched.contains(index) {
                        #expect(ran.map { $0 < next } == true, "<script \(attributes) src> was fetched but not waited for: \(log)")
                    } else {
                        #expect(ran == nil, "<script \(attributes) src> ran without a request: \(log)")
                    }
                }
                print("SS12 engine fetched: \(fetched.map { Self.scriptTypeVariants[$0] })")
            }
        }

        // MARK: SS13, C6.5

        @Test func aScriptAddedAheadOfTheWalkIsReplacedToo() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let side = """
                    <script>
                    (function () {
                      var s = document.createElement("script");
                      s.textContent = "window.__d=(window.__d||0)+1";
                      document.getElementById("qa").appendChild(s);
                    })();
                    </script>
                    """
                try await page.showPresentedQuestion(side)
                #expect(try await page.string("window.__d") == "2")
            }
        }

        // MARK: SS14, R518

        @Test func moduleScriptsRunAsModules() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<script type="module">log(document.currentScript)</script>"#)
                #expect(try await page.log(count: 1) == ["null"])
            }
        }

        // MARK: SS15, C7.4, D10

        @Test func remoteScriptWaitIsCapped() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let remote = "\(WebTestHarness.remoteScheme)://other/slow.js"
                try await page.showPresentedQuestion(
                    #"<script>window.__w = performance.now();</script><script src="\#(remote)"></script><script>log(performance.now() - window.__w)</script>"#)
                let waited = try #require(Double(try await page.log(count: 1)[0]))
                #expect(waited >= 750 && waited < 1_400, "the walk went on \(waited) ms after it started")
                #expect(!harness.remote.requests(for: "/slow.js").isEmpty)
            }
        }

        // MARK: SS16, D21

        @Test func svgScriptRunsWithDecodedText() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<svg><script>window.__s = 1 &amp;&amp; 2; log("svg")</script></svg>"#)
                #expect(try await page.log() == ["svg"])
                #expect(try await page.string("window.__s") == "2")
            }
        }

        // MARK: SS17, D22

        @Test func unusualAttributeNameIsCopied() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<script a"b="v">log(document.currentScript.getAttribute('a"b'))</script><script>log("next")</script>"#)
                #expect(try await page.log() == ["v", "next"])
                #expect(harness.signals("updateFailed").isEmpty)
            }
        }

        // MARK: SS18

        @Test func scriptsInTemplateContentsDoNotRun() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    "<template><script>window.__t = 1</script></template><script>log(typeof window.__t)</script>")
                #expect(try await page.log() == ["undefined"])
            }
        }

        // MARK: SS19, S9, R503

        @Test func sessionStorageLastsAsLongAsThePage() async throws {
            let harness = WebTestHarness()
            defer { harness.close() }
            let first = try await harness.openPage()
            try await first.installLog()
            try await first.showPresentedQuestion(try RuntimeFixture.text("s9-persistence-question.html"))
            try await first.showPresentedAnswer(try RuntimeFixture.text("s9-persistence-answer.html"))
            #expect(try await first.log() == ["object", "undefined", "undefined", "v"])
            // A second page, as for the next card.
            let second = try await harness.openPage()
            try await second.installLog()
            try await second.showPresentedQuestion(try RuntimeFixture.text("s9-persistence-answer.html"))
            #expect(try await second.log() == ["null"])
        }

        // MARK: C12

        @Test func diagnosticsCarryKindPhaseAndSource() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_bad.js", text: "window.__bad = 1;\nundefinedFunction();")
                try await page.showPresentedQuestion(
                    """
                    <script>window.onerror = function (message) { log("onerror"); };</script>
                    <script src="_bad.js"></script>
                    <script>
                    Promise.reject(new RangeError("unhandled"));
                    setTimeout(function () { throw new Error("from a timer"); });
                    </script>
                    """)
                try await harness.until("three diagnostics") { harness.diagnostics.count >= 3 }
                let byKind = Dictionary(grouping: harness.diagnostics) { "\($0["kind"] as? String ?? "")/\($0["phase"] as? String ?? "")" }
                let script = try #require(byKind["exception/script"]?.first)
                #expect(script["index"] as? Int == 2 && script["line"] as? Int == 2)
                #expect((script["source"] as? String)?.hasSuffix("/_bad.js") == true)
                #expect((script["message"] as? String)?.hasPrefix("ReferenceError: ") == true)
                let rejection = try #require(byKind["rejection/later"]?.first)
                #expect(rejection["message"] as? String == "RangeError: unhandled" && rejection["line"] == nil)
                let timer = try #require(byKind["exception/later"]?.first)
                #expect(timer["message"] as? String == "Error: from a timer" && timer["source"] as? String == "inline")
                // The card's own handler still saw both exceptions.
                try await harness.until("the card's onerror ran twice") { try await page.log().count >= 2 }
                #expect(try await page.log() == ["onerror", "onerror"])
            }
        }

        // MARK: C6.5: the walk's order

        @Test func moduleSourcesAreAwaited() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_mod.js", text: "window.__mod = 1;")
                harness.media.delay("/_mod.js", by: .milliseconds(150))
                try await page.showPresentedQuestion(#"<script type="module" src="_mod.js"></script><script>log(typeof window.__mod)</script>"#)
                #expect(try await page.log() == ["number"])
            }
        }


        @Test func aScriptThatRemovesItselfMakesTheWalkSkipTheNext() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<script>log(1); document.currentScript.remove();</script><script>log(2)</script><script>log(3)</script>"#)
                #expect(try await page.log() == ["1", "3"])
            }
        }

        @Test func attributesAreCopiedInOrder() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<script data-b="2" id="s" data-a="1">log(Array.prototype.map.call(document.currentScript.attributes, function (a) { return a.name; }).join(","))</script>"#)
                #expect(try await page.log() == ["data-b,id,data-a"])
            }
        }

        // MARK: AE15 (C16.7)

        @Test func migakuStyleBackRunsItsLaterScriptsAfterTheFirstThrows() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let back = try RuntimeFixture.text("ae15-back.html")
                for card in 1...2 {
                    try await page.showPresentedQuestion("<p>front</p>", bodyClass: "card card\(card)")
                    try await page.showPresentedAnswer(back)
                    #expect(try await page.string("document.querySelectorAll('#qa .word').length") == "2")
                    #expect(try await page.string("document.querySelectorAll('#qa .word svg.pitch').length") == "2")
                    // What the side shows (innerText leaves out the scripts' own text).
                    let text = try await page.string(#"document.getElementById("qa").innerText"#)
                    #expect(!text.contains("[") && !text.contains(";h]") && !text.contains("かんじ;"), "\(text)")
                }
                // Scripts 2 and 3 ran on both cards; script 2 took the
                // `complete` branch both times.
                #expect(try await page.log() == ["pitch:complete", "support", "pitch:complete", "support"])
                let errors = harness.diagnostics
                #expect(errors.count == 2)
                for error in errors {
                    #expect((error["message"] as? String)?.hasPrefix("TypeError: ") == true)
                    #expect(error["phase"] as? String == "script" && error["index"] as? Int == 1)
                    #expect(error["line"] as? Int == 1 && error.side == "answer")
                }
            }
        }
    }
}
