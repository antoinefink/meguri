import CardRendering
import Foundation
import Instrumentation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// Side updates (specs/render/card-runtime.md C6–C8, C16.1; R466, R467,
    /// R471, R472).
    @Suite struct SideUpdateTests {
        // MARK: SU1, D7: the R467 order, settled beneath, shown hooks at present

        @Test func shownHooksWaitForPresent() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_su_b.js", text: #"log("B");"#)
                let question = """
                    <script>
                    log("A");
                    onUpdateHook.push(function () {
                      var qa = document.getElementById("qa");
                      log("U-start(" + document.body.className + ", " + qa.style.opacity + ", " + window.scrollY + ")");
                      return new Promise(function (resolve) { setTimeout(function () { log("U-end"); resolve(); }, 50); });
                    });
                    onShownHook.push(function () { log("S"); });
                    </script>
                    <script src="_su_b.js"></script>
                    <script>log("C(" + (window.__log.indexOf("B") >= 0) + ")");</script>
                    <div style="height: 2000px">tall</div>
                    """
                // A scrolled page, so the question's update callback has
                // something to reset.
                try await page.run("document.body.style.minHeight = '3000px'; window.scrollTo(0, 500);")
                try await page.announce(card: 42, generation: 7)
                try await page.showQuestion(question, bodyClass: "card card2")
                let settled = try await harness.signal("settled", from: page)
                // Read at once: the update hook's promise settled before this.
                let atSettled = try await page.log()
                #expect(atSettled == ["A", "B", "C(true)", "U-start(card card2, 0, 0)", "U-end"])
                #expect(try await page.string(#"document.getElementById("qa").style.opacity"#) == "1")
                #expect(settled.side == "question" && settled.card == 42 && settled.generation == 7)
                #expect(harness.confirmations.contains(.init(token: 7, cardID: 42, phase: .settled)))
                // Unpresented: the shown hook waits.
                try await Task.sleep(for: .milliseconds(300))
                #expect(try await page.log() == atSettled)
                #expect(harness.signals("shown").isEmpty)
                try await page.present(token: 3)
                let shown = try await harness.signal("shown", from: page)
                #expect(try await page.log() == atSettled + ["S"])
                #expect(shown.side == "question" && shown.card == 42 && shown.generation == 7)
            }
        }

        /// R1862: two frames after presentation the interaction script posts
        /// U12's visible confirmation naming the card.
        @Test func visibleConfirmationNamesThePresentedCard() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.announce(card: 9, generation: 2)
                try await page.showQuestion("<p>nine</p>")
                try await harness.signal("settled", from: page)
                #expect(harness.signals("visible").isEmpty)
                try await page.present(token: 11)
                let visible = try await harness.signal("visible", from: page)
                #expect(visible.card == 9 && visible.side == "question" && visible["token"] as? Int == 11)
                #expect(harness.confirmations.contains(.init(token: 11, cardID: 9, phase: .visible)))
            }
        }

        // MARK: SU2

        @Test func presentedAnswerRunsShownHooksAtOnce() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>")
                try await page.showAnswer(#"<p>a</p><script>onShownHook.push(function () { log("S-answer"); });</script>"#)
                let shown = try await harness.signal("shown", count: 2, from: page)
                #expect(try await page.log() == ["S-answer"])
                #expect(shown.side == "answer")
                #expect(harness.signals("settled").isEmpty)
            }
        }

        // MARK: SU3, R466

        @Test func queuedUpdatesNeverInterleave() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.present()
                let first = """
                    <script>
                    log("Q1");
                    onShownHook.push(function () {
                      return new Promise(function (resolve) { setTimeout(function () { log("Q1-shown"); resolve(); }, 100); });
                    });
                    </script>
                    """
                let second = #"<script>log("Q2"); onShownHook.push(function () { log("Q2-shown"); });</script>"#
                try await page.run(
                    #"_showQuestion(a, "", "card card1"); _showQuestion(b, "", "card card1");"#, ["a": first, "b": second])
                #expect(try await page.log(count: 4) == ["Q1", "Q1-shown", "Q2", "Q2-shown"])
            }
        }

        // MARK: SU4

        @Test func supersededUpdateNeverRunsItsShownPhase() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<input id="typeans" value="zero">"#)
                #expect(try await page.string("getTypedAnswer()") == "zero")
                try await page.unpresent()
                try await page.showQuestion(#"<input id="typeans" value="one"><script>onShownHook.push(function () { log("S1"); });</script>"#)
                try await harness.signal("settled", from: page)
                try await page.showQuestion(#"<p id="q2">two</p><script>onShownHook.push(function () { log("S2"); });</script>"#)
                try await harness.signal("settled", count: 2, from: page)
                #expect(try await page.bool(#"document.getElementById("q2") !== null"#))
                #expect(try await page.string("getTypedAnswer()") == "zero")
                try await page.present()
                try await harness.signal("shown", count: 2, from: page)
                #expect(try await page.log() == ["S2"])
                // The second question had no #typeans.
                #expect(try await page.string("getTypedAnswer()") == "null")
            }
        }

        // MARK: SU5, R471

        @Test func videosAreReleasedBeforeTheNextSide() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let first = """
                    <video autoplay muted><source src="_v.mp4"></video>
                    <script>
                    window.__v = document.querySelector("video");
                    __v.addEventListener("emptied", function () { log("emptied"); });
                    log("side1");
                    </script>
                    """
                try await page.showPresentedQuestion(first)
                try await page.showPresentedAnswer("<script>log(__v.paused); log(__v.childElementCount);</script>")
                let log = try await page.log(count: 4)
                #expect(Array(log.prefix(1)) == ["side1"])
                #expect(log.contains("true") && log.contains("0"))
                #expect(log.filter { $0 == "emptied" }.count == 1)
                #expect(log.firstIndex(of: "emptied")! > log.firstIndex(of: "side1")!)
            }
        }

        // MARK: SU6, R472

        @Test func reviewExtraRunsOncePerPageAfterQA() async throws {
            let extra = #"<div id="rx"></div><script>window.__rx=(window.__rx||0)+1</script>"#
            try await WebTestHarness.withPage(reviewExtra: extra) { harness, page async throws in
                try await page.showPresentedQuestion("<p>1</p>")
                try await page.showPresentedAnswer("<p>2</p>")
                try await page.showPresentedQuestion("<p>3</p>")
                #expect(try await page.string(#"document.getElementById("qa").nextElementSibling.id"#) == "rx")
                #expect(try await page.string("window.__rx") == "1")
            }
        }

        // MARK: SU7, C8.2, D7

        @Test func autoplayHeldUntilPresent() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_tone.wav", TestMedia.silentWAV(seconds: 3), "audio/wav")
                try await page.showQuestion(#"<audio id="tone" src="_tone.wav" autoplay loop></audio>"#)
                try await harness.signal("settled", from: page)
                // It tried to play: autoplay is allowed for card pages.
                try await harness.until("the audio loaded") {
                    try await page.bool(#"document.getElementById("tone").readyState >= 2"#)
                }
                for _ in 0..<5 {
                    #expect(try await page.bool(#"document.getElementById("tone").paused"#))
                    try await Task.sleep(for: .milliseconds(60))
                }
                try await page.present()
                try await harness.until("the held audio played") {
                    try await page.bool(#"!document.getElementById("tone").paused"#)
                }
                try await page.unpresent()
                try await harness.until("the audio paused") {
                    try await page.bool(#"document.getElementById("tone").paused"#)
                }
            }
        }

        // MARK: SU8, C6.7, D11

        @Test func failedUpdateDoesNotBlockQueue() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.present()
                try await page.announce(card: 1, generation: 1)
                try await page.showQuestion(#"<script>document.getElementById("qa").remove();</script>"#)
                try await page.announce(card: 2, generation: 2)
                try await page.showQuestion("<p>next</p>")
                let second = try await harness.signal("updateFailed", count: 2, from: page)
                let first = harness.signals("updateFailed")[0]
                #expect(first.card == 1 && second.card == 2)
                #expect(!((first["error"] as? String) ?? "").isEmpty)
            }
        }

        // MARK: SU9, C6.4, D12

        @Test func hookFailureRecorded() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                let question = """
                    <script>
                    window.onerror = function () { log("onerror"); };
                    window.addEventListener("unhandledrejection", function () { log("unhandled"); });
                    onUpdateHook.push(function () { throw new Error("boom"); });
                    onUpdateHook.push(function () { return Promise.reject(new Error("nope")); });
                    onUpdateHook.push(function () { log("third"); });
                    onShownHook.push(function () { throw new TypeError("late"); });
                    onShownHook.push(function () { log("shown-next"); });
                    </script>
                    """
                try await page.showPresentedQuestion(question)
                try await Task.sleep(for: .milliseconds(200))
                #expect(try await page.log() == ["third", "shown-next"])
                let hooks = harness.diagnostics.filter { $0["kind"] as? String == "hook" }
                #expect(hooks.map { $0["message"] as? String } == ["Error: boom", "Error: nope", "TypeError: late"])
                #expect(hooks.map { $0["phase"] as? String } == ["update-hook", "update-hook", "shown-hook"])
                #expect(hooks.allSatisfy { $0.side == "question" && $0["line"] == nil })
                #expect(harness.diagnostics.count == 3)
            }
        }

        // MARK: SU10, C6.6, D18

        @Test func settleWaitsForFonts() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_f.ttf", TestMedia.font(), "font/ttf")
                // Longer than the preload budget (800 ms): the font is still
                // loading when the side is inserted.
                harness.media.delay("/_f.ttf", by: .milliseconds(1_200))
                let question = """
                    <style>@font-face { font-family: F; src: url("_f.ttf"); font-display: block; }</style>
                    <p style="font-family: F">Meguri</p>
                    <script>log(performance.now() - window.__t0);</script>
                    """
                try await page.run(#"window.__t0 = performance.now(); _showQuestion(q, "", "card card1");"#, ["q": question])
                try await harness.signal("settled", from: page)
                #expect(try await page.bool(#"document.fonts.check("12px F")"#))
                #expect(harness.media.requests(for: "/_f.ttf").count >= 1)
                // C7.2: a new font makes the preload budget 800 ms.
                let inserted = try #require(Double(try await page.log()[0]))
                #expect(inserted >= 790 && inserted < 1_300, "inserted \(inserted) ms after the call")
            }
        }

        // MARK: SU11, C7.2

        @Test func imagePreloadWaitsTwoHundredMilliseconds() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_big.png", TestMedia.png(), "image/png")
                harness.media.delay("/_big.png", by: .seconds(1))
                try await page.present()
                try await page.run(
                    #"window.__t0 = performance.now(); _showQuestion(q, "", "card card1");"#,
                    ["q": #"<img src="_big.png"><script>log(performance.now() - window.__t0);</script>"#])
                let waited = try #require(Double(try await page.log(count: 1)[0]))
                #expect(waited >= 190 && waited < 600, "inserted \(waited) ms after the call")
            }
        }

        /// C7.2: a stylesheet makes the budget 500 ms.
        @Test func stylesheetPreloadWaitsFiveHundredMilliseconds() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_s.css", text: "p { color: red; }", "text/css")
                harness.media.delay("/_s.css", by: .seconds(2))
                try await page.present()
                try await page.run(
                    #"window.__t0 = performance.now(); _showQuestion(q, "", "card card1");"#,
                    ["q": #"<link rel="stylesheet" href="_s.css"><script>log(performance.now() - window.__t0);</script>"#])
                let waited = try #require(Double(try await page.log(count: 1)[0]))
                #expect(waited >= 490 && waited < 900, "inserted \(waited) ms after the call")
            }
        }

        // MARK: SU12, C13

        @Test func registryReportsAScriptKeyListener() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<p>a</p><script>document.addEventListener("keydown", function () {});</script>"#)
                #expect(try await page.hasCardListener("key"))
                #expect(try await page.hasCardListener("keydown"))
                #expect(try await !page.hasCardListener("tap", at: "p"))
            }
        }

        @Test func registryReportsAnOnclickAttributeOnTheTargetsAncestors() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<div id="t" onclick=""><span id="tc">child</span></div><p id="p">x</p>"#)
                #expect(try await page.hasCardListener("tap", at: "#tc"))
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        @Test func registryReportsNothingForAPlainSide() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<p id="p">plain</p><input id="i">"#)
                #expect(try await !page.hasCardListener("tap", at: "#p"))
                #expect(try await !page.hasCardListener("key"))
                // Focus in an input: the runtime's own key tracking on window
                // never counts.
                try await page.run(#"document.getElementById("i").focus();"#)
                #expect(try await !page.hasCardListener("key"))
            }
        }

        @Test func registryFollowsHandlerProperties() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>x</p><script>document.onkeydown = function () {};</script>")
                #expect(try await page.hasCardListener("key"))
                try await page.run("document.onkeydown = null;")
                #expect(try await !page.hasCardListener("key"))
            }
        }

        @Test func registryKeepsWindowListenersAcrossCardsUntilRemoved() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<p>n</p><script>window.__f = function () {}; window.addEventListener("touchstart", window.__f);</script>"#)
                try await page.showPresentedQuestion(#"<p id="p">n+1</p>"#, bodyClass: "card card2")
                #expect(try await page.hasCardListener("tap", at: "#p"))
                try await page.run(#"window.removeEventListener("touchstart", window.__f);"#)
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        @Test func registryCountsElementListenersOnlyWhileTheirSideIsShown() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<div id="d"><p id="p">x</p></div><script>document.getElementById("d").addEventListener("click", function () {});</script>"#)
                #expect(try await page.hasCardListener("tap", at: "#p"))
                try await page.showPresentedAnswer(#"<div id="d"><p id="p">y</p></div>"#)
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        @Test func registryForgetsListenersAbortedBySignal() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    #"<p id="p">x</p><script>window.__c = new AbortController(); document.addEventListener("click", function () {}, { signal: __c.signal });</script>"#)
                #expect(try await page.hasCardListener("tap", at: "#p"))
                try await page.run("window.__c.abort();")
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        // MARK: SU13, C6.6, D18

        @Test func settleWaitsForImageDecode() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_slow.png", TestMedia.png(width: 64, height: 64), "image/png")
                harness.media.delay("/_slow.png", by: .milliseconds(600))
                try await page.showQuestion(#"<img id="slow" src="_slow.png">"#)
                try await harness.signal("settled", from: page)
                #expect(try await page.bool(#"document.getElementById("slow").complete"#))
                #expect(try await page.string(#"document.getElementById("slow").naturalWidth"#) == "64")
            }
        }

        @Test func settleSkipsLazyImages() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.hold("/_never.png")
                let start = ProcessInfo.processInfo.systemUptime
                try await page.showQuestion(#"<div style="height: 5000px"></div><img loading="lazy" src="_never.png">"#)
                let settled = try await harness.signal("settled", from: page)
                // 200 ms of preload budget (C7.2), then no image wait.
                let elapsed = (settled.time - start) * 1000
                #expect(elapsed < 650, "settled \(elapsed) ms after the call")
            }
        }

        @Test func settleWaitsAtMostEightHundredMilliseconds() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.hold("/_never.png")
                let start = ProcessInfo.processInfo.systemUptime
                try await page.showQuestion(#"<img src="_never.png">"#)
                let settled = try await harness.signal("settled", from: page)
                // 200 ms of preload budget, then 800 ms of settling.
                let elapsed = (settled.time - start) * 1000
                #expect(elapsed >= 990 && elapsed < 1_700, "settled \(elapsed) ms after the call")
            }
        }

        // MARK: SU14

        @Test func presentationWhileSettlingRunsShownAtOnce() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.hold("/_never.png")
                try await page.showQuestion(
                    #"<img src="_never.png"><script>onUpdateHook.push(function () { log("U"); }); onShownHook.push(function () { log("S"); });</script>"#)
                _ = try await page.log(count: 1)
                try await Task.sleep(for: .milliseconds(100))
                let presented = ProcessInfo.processInfo.systemUptime
                try await page.present()
                let shown = try await harness.signal("shown", from: page)
                #expect((shown.time - presented) * 1000 < 300)
                #expect(try await page.log() == ["U", "S"])
                try await Task.sleep(for: .seconds(1))
                #expect(harness.signals("settled").isEmpty)
            }
        }

        // MARK: C6.8

        /// The identity reaches the update even when a card script wrapped
        /// `_showQuestion` and calls the original with Anki's arguments.
        @Test func identityReachesAWrappedShowQuestion() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <script>
                    (function () {
                      var original = window._showQuestion;
                      window._showQuestion = function (q, a, bodyclass) { log("wrapped"); return original(q, a, bodyclass); };
                    })();
                    </script>
                    """)
                try await page.announce(card: 77, generation: 5)
                try await page.showQuestion("<p>next</p>", bodyClass: "card card2")
                let shown = try await harness.signal("shown", count: 2, from: page)
                #expect(try await page.log() == ["wrapped"])
                #expect(shown.card == 77 && shown.generation == 5)
            }
        }

        // MARK: C6.4: the answer's update callback

        @Test func answerScrollsToTheAnswerMarker() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>")
                try await page.showPresentedAnswer(
                    #"<div style="height: 3000px">front</div><hr id="answer"><div style="height: 3000px">back</div>"#)
                try await harness.until("scrolled to #answer") {
                    try await page.bool(#"Math.abs(document.getElementById("answer").getBoundingClientRect().top) < 2"#)
                }
            }
        }

        @Test func answerKeepsTheQuestionsBodyClassesUnlessGivenSome() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>", bodyClass: "card card3 isMac")
                try await page.showPresentedAnswer("<p>a</p>")
                #expect(try await page.string("document.body.className") == "card card3 isMac")
                try await page.showPresentedAnswer("<p>a</p>", bodyClass: "card card1 preview")
                #expect(try await page.string("document.body.className") == "card card1 preview")
            }
        }

        /// C7.1: a font URL the page asked for once is never waited for
        /// again.
        @Test func aFontAskedForOnceIsNotWaitedForAgain() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_g.ttf", TestMedia.font(), "font/ttf")
                harness.media.delay("/_g.ttf", by: .milliseconds(300))
                let side = """
                    <style>@font-face { font-family: G; src: local(Nothing), url("_g.ttf") format("truetype"); }</style>
                    <p style="font-family: G">x</p><script>log(performance.now() - window.__t0);</script>
                    """
                try await page.present()
                for _ in 0..<2 {
                    try await page.run(#"window.__t0 = performance.now(); _showQuestion(q, "", "card card1");"#, ["q": side])
                }
                let times = try await page.log(count: 2).compactMap(Double.init)
                #expect(times.count == 2)
                #expect(times[0] >= 290, "first side inserted \(times[0]) ms after its call")
                #expect(times[1] < 150 + times[0], "the second side waited for the font again: \(times)")
            }
        }

        // MARK: C6.4: the hook arrays

        @Test func hooksAreResetInPlaceForEverySide() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <script>
                    window.__arrays = [onUpdateHook, onShownHook];
                    onUpdateHook.push(function () { log("U1"); });
                    onShownHook.push(function () { log("S1"); });
                    onShownHook.push(function () { onShownHook.push(function () { log("appended"); }); });
                    </script>
                    """)
                try await page.showPresentedAnswer(
                    """
                    <script>
                    log(String(onUpdateHook === window.__arrays[0] && onShownHook === window.__arrays[1]) + ":" + onUpdateHook.length + ":" + onShownHook.length);
                    window.onUpdateHook = [function () { log("replaced"); }];
                    require("anki/reviewer").onShownHook = [function () { log("replaced too"); }];
                    </script>
                    """)
                #expect(try await page.log() == ["U1", "S1", "appended", "true:1:1"])
            }
        }

        // MARK: C6.1: _updateQA

        @Test func updateQARunsAtOnceWithItsOwnCallbacks() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.present()
                try await page.run(
                    """
                    await _updateQA("<p id='u'>u</p><script>log('script')</script>", "ignored",
                      function () { log("onupdate:" + document.getElementById("qa").style.opacity); },
                      function () { log("onshown:" + document.getElementById("qa").style.opacity); });
                    log("resolved");
                    """)
                #expect(try await page.log() == ["script", "onupdate:0", "onshown:1", "resolved"])
                #expect(try await page.bool(#"document.getElementById("u") !== null"#))
            }
        }

        // MARK: C3.1

        @Test func scriptsSeeThePreviousBodyClassesDuringInsertion() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>1</p>", bodyClass: "card card1")
                try await page.showPresentedQuestion(
                    "<script>log(document.body.className); onUpdateHook.push(function () { log(document.body.className); });</script>",
                    bodyClass: "card card2")
                #expect(try await page.log() == ["card card1", "card card2"])
            }
        }

        // MARK: C13

        @Test func registryFollowsAncestorsAcrossShadowRoots() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <div id="host"></div><p id="p">x</p>
                    <script>
                    (function () {
                      var host = document.getElementById("host");
                      var root = host.attachShadow({ mode: "open" });
                      root.innerHTML = "<span id='inner'>in</span>";
                      host.addEventListener("pointerdown", function () {});
                    })();
                    </script>
                    """)
                let inner = try await page.runIsolated(
                    #"return meguriCard.hasCardListener("tap", document.getElementById("host").shadowRoot.getElementById("inner"));"#)
                #expect(inner as? Bool == true)
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        @Test func registryCountsTheSameListenerOnce() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <p id="p">x</p>
                    <script>
                    window.__h = function () {};
                    document.addEventListener("keyup", window.__h);
                    document.addEventListener("keyup", window.__h, false);
                    document.addEventListener("keyup", window.__h, { capture: true });
                    </script>
                    """)
                #expect(try await page.hasCardListener("keyup"))
                try await page.run(#"document.removeEventListener("keyup", window.__h);"#)
                #expect(try await page.hasCardListener("keyup"))
                try await page.run(#"document.removeEventListener("keyup", window.__h, true);"#)
                #expect(try await !page.hasCardListener("keyup"))
            }
        }

        // MARK: C7.5

        @Test func warmingLoadsImagesWithoutInserting() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_warm.png", TestMedia.png(), "image/png")
                try await page.warm(#"<img src="_warm.png"><script>log("ran")</script>"#)
                try await harness.until("the image was requested") { !harness.media.requests(for: "/_warm.png").isEmpty }
                try await Task.sleep(for: .milliseconds(100))
                #expect(try await page.log().isEmpty)
                #expect(try await page.string(#"document.getElementById("qa").innerHTML"#) == "")
            }
        }
    }
}
