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
                // Posted right after the signal, to another handler.
                try await harness.until("the visible confirmation") {
                    harness.confirmations.contains(.init(token: 11, cardID: 9, phase: .visible))
                }
            }
        }

        /// C6.6, C14, R1862, counted by the test's own frame clock: the
        /// each confirmation requires at least two frames after its trigger.
        @Test func confirmationsComeTwoFramesAfterWhatTheyConfirm() async throws {
            try await WebTestHarness.withPage(frameClock: true) { harness, page async throws in
                try await page.announce(card: 5, generation: 3)
                try await page.showQuestion("<p>five</p>")
                try await harness.signal("settled", from: page)
                let shown = try await page.frame(of: "qaVisible")
                let settled = try await page.frame(of: "\(CardRuntime.confirmationHandlerName):settled")
                #expect(settled - shown >= 2, "settled confirmed in frame \(settled), #qa visible in frame \(shown)")
                #expect(try await page.frame(of: "\(CardRuntime.signalHandlerName):settled") == settled)

                try await page.presentStamped(token: 4)
                try await harness.signal("visible", from: page)
                let presented = try await page.frame(of: "present")
                let visible = try await page.frame(of: "\(CardRuntime.confirmationHandlerName):visible")
                #expect(visible - presented >= 2, "visible confirmed in frame \(visible), presented in frame \(presented)")
                #expect(try await page.frame(of: "\(CardRuntime.signalHandlerName):visible") == visible)
                try await harness.until("the visible confirmation") {
                    harness.confirmations.contains(.init(token: 4, cardID: 5, phase: .visible))
                }
            }
        }

        /// The frame clock itself: it still stamps the interaction script's
        /// posts after the page collected its garbage. WebKit replaces a
        /// message handler object nothing holds, which dropped the clock's
        /// wrapper and failed the case above on a simulator.
        @Test func frameClockStampsAfterACollection() async throws {
            try await WebTestHarness.withPage(frameClock: true) { harness, page async throws in
                let finalized = try await page.runIsolated(
                    """
                    let finalized = 0;
                    const registry = new FinalizationRegistry(function () { finalized += 1; });
                    (function () { registry.register({ sentinel: true }, 1); })();
                    for (let round = 0; round < 400; round++) {
                      let garbage = [];
                      for (let i = 0; i < 20000; i++) garbage.push({ i: i, text: "g" + i });
                      garbage = null;
                      await new Promise(function (resolve) { setTimeout(resolve, 2); });
                    }
                    return finalized;
                    """)
                #expect(finalized as? Int == 1, "the page collected no garbage")
                try await page.announce(card: 2, generation: 1)
                try await page.showQuestion("<p>q</p>")
                try await harness.signal("settled", from: page)
                _ = try await page.frame(of: "\(CardRuntime.signalHandlerName):settled")
                _ = try await page.frame(of: "\(CardRuntime.confirmationHandlerName):settled")
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

        /// SU5 for videos whose media is not a `<source>` child: one with its
        /// own `src` (a real clip) and one with a `srcObject`. Telling either
        /// to reload as it is would select the same media again; released,
        /// both are back to no network activity and no data (R471).
        @Test func videosWithTheirOwnMediaAreUnloaded() async throws {
            let clip = try await TestMedia.mp4(seconds: 2, fps: 5, size: CGSize(width: 64, height: 48))
            try await WebTestHarness.withPage { harness, page async throws in
                harness.media.add("/_d.mp4", clip, "video/mp4")
                let first = """
                    <video id="d" src="_d.mp4" muted preload="auto"></video>
                    <video id="o" muted></video>
                    <script>
                    window.__d = document.getElementById("d");
                    window.__o = document.getElementById("o");
                    __o.srcObject = document.createElement("canvas").captureStream();
                    </script>
                    """
                try await page.showPresentedQuestion(first)
                try await harness.until("both videos have media") {
                    try await page.bool("__d.readyState >= 1 && __o.networkState !== 0")
                }
                try await page.showPresentedAnswer("<p>next</p>")
                // Long enough for a reload to select its media again.
                try await Task.sleep(for: .milliseconds(300))
                let state = try await page.string(
                    "[__d.paused, __d.networkState, __d.readyState, __o.paused, __o.networkState, __o.readyState].join()")
                #expect(state == "true,0,0,true,0,0")
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

        /// Not in a virtual machine: the test VM's simulators have no audio
        /// route, and an `<audio>` there never loads past its metadata, so
        /// it never autoplays (decision record s2, S2(a)).
        @Test(.disabled(if: runsInVirtualMachine(), "no audio route in a virtual machine"))
        func autoplayHeldUntilPresent() async throws {
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

        private struct ListenerArgumentResult: Decodable, Equatable {
            let trace: [String]
            let outcome: String
            let calls: Int
        }

        // Each run uses fresh caller objects for the wrapped method and an
        // unwrapped method from an about:blank realm, on sibling page elements.
        // Comparing dispatch as well as the registry catches false negatives;
        // comparing traces catches extra reads even when their values agree.
        private static let listenerArgumentProbe = #"""
            const frame = document.createElement("iframe");
            frame.hidden = true;
            frame.src = "about:blank";
            await new Promise(resolve => { frame.onload = resolve; document.body.appendChild(frame); });
            const native = frame.contentWindow.EventTarget.prototype;
            const methods = [EventTarget.prototype, native];
            const results = [];
            for (let i = 0; i < methods.length; i++) {
              const target = document.getElementById(i === 0 ? "probe" : "control");
              const add = methods[i].addEventListener;
              const remove = methods[i].removeEventListener;
              const trace = [];
              const sentinel = new Error("synthetic listener conversion failure");
              let calls = 0, reads = 0;
              const handler = () => { calls++; };
              let type = "keydown", options, operation = add, receiver = target, args;
              const dictionary = (capture, signal, callable = false) => {
                const value = callable ? function () {} : {};
                for (const key of ["capture", "once", "passive", "signal"]) {
                  Object.defineProperty(value, key, { get() {
                    trace.push(key + ":" + (this === value));
                    if (key === "capture") return capture();
                    if (key === "signal") return signal();
                    return false;
                  } });
                }
                return Object.freeze(value);
              };
              const tracedType = () => {
                const value = {};
                Object.defineProperty(value, "toString", { get() {
                  trace.push("toString.get:" + (this === value));
                  return function () {
                    trace.push("toString.call:" + (this === value));
                    return ++reads === 1 ? "keydown" : "untracked-synthetic-event";
                  };
                } });
                return Object.freeze(value);
              };
              if (scenario === "signal-changes" || scenario === "signal-throws-again") {
                const aborted = new AbortController();
                aborted.abort();
                options = dictionary(() => false, () => {
                  if (++reads === 1) return undefined;
                  if (scenario === "signal-throws-again") throw sentinel;
                  return aborted.signal;
                });
              } else if (scenario === "remove-capture-changes") {
                Reflect.apply(add, target, [type, handler, false]);
                operation = remove;
                options = dictionary(() => ++reads === 1, () => undefined);
              } else if (scenario === "type-changes" || scenario === "remove-type-changes") {
                if (scenario === "remove-type-changes") {
                  Reflect.apply(add, target, [type, handler]);
                  operation = remove;
                }
                type = tracedType();
              } else if (scenario === "string-hint" || scenario === "object-symbol") {
                type = { [Symbol.toPrimitive](hint) {
                  trace.push("primitive:" + hint + ":" + (this === type));
                  return scenario === "object-symbol" ? Symbol("synthetic") : "keydown";
                } };
              } else if (scenario === "fallback-type") {
                type = {
                  toString() { trace.push("toString:" + (this === type)); return {}; },
                  valueOf() { trace.push("valueOf:" + (this === type)); return "keydown"; }
                };
              } else if (scenario === "symbol") {
                type = Symbol("synthetic");
              } else if (scenario === "type-throws") {
                type = { [Symbol.toPrimitive]() { trace.push("type-throws"); throw sentinel; } };
              } else if (scenario === "options-throws") {
                type = tracedType();
                options = dictionary(() => { throw sentinel; }, () => undefined);
              } else if (scenario.startsWith("invalid-receiver") || scenario.startsWith("missing-")) {
                type = tracedType();
                options = dictionary(() => false, () => undefined);
                if (scenario.startsWith("invalid-receiver")) receiver = {};
                if (scenario.endsWith("remove")) operation = remove;
              } else if (scenario === "invalid-callback") {
                type = tracedType();
                options = dictionary(() => false, () => undefined);
              } else if (scenario === "callable-options") {
                options = dictionary(() => true, () => undefined, true);
              } else {
                options = {
                  "boolean-false": false, "boolean-true": true, "null": null,
                  "undefined": undefined, "zero": 0, "one": 1,
                  "empty-string": "", "string": "capture", "symbol-options": Symbol("capture"),
                  "bigint-options": 1n
                }[scenario];
              }
              args = [type, scenario === "invalid-callback" ? 7 : handler, options];
              if (scenario.startsWith("missing-type")) args = [];
              else if (scenario.startsWith("missing-callback")) args = [type];
              else if (scenario === "omitted-options") args = [type, handler];
              let outcome;
              try { outcome = "return:" + String(Reflect.apply(operation, receiver, args)); }
              catch (error) { outcome = error === sentinel ? "throw:sentinel" : error.name + ":" + error.message; }
              // Snapshot caller access before dispatch/cleanup can add noise.
              const access = trace.slice();
              target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true }));
              results.push({ trace: access, outcome, calls });
              target.__removeFalseListenerProbe = () => {
                Reflect.apply(remove, target, ["keydown", handler, false]);
                const before = calls;
                target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true }));
                return calls - before;
              };
              target.__cleanupListenerProbe = () => {
                Reflect.apply(remove, target, ["keydown", handler, false]);
                Reflect.apply(remove, target, ["keydown", handler, true]);
                const before = calls;
                target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true }));
                return calls - before;
              };
            }
            frame.remove();
            return JSON.stringify(results);
            """#

        @Test(arguments: [
            "signal-changes", "signal-throws-again", "remove-capture-changes", "type-changes", "remove-type-changes",
            "string-hint", "fallback-type", "callable-options", "boolean-false", "boolean-true",
            "null", "undefined", "omitted-options", "zero", "one", "empty-string", "string", "symbol-options", "bigint-options"
        ])
        func registryUsesTheArgumentsNativeConsumed(_ scenario: String) async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<div id="probe"></div><div id="control"></div>"#)
                let results = try await page.json([ListenerArgumentResult].self, Self.listenerArgumentProbe, ["scenario": scenario])
                #expect(results.count == 2)
                let wrapped = try #require(results.first)
                #expect(wrapped == results.last, "\(scenario): \(results)")
                #expect(wrapped.outcome == "return:undefined")
                let registered = scenario != "remove-type-changes"
                #expect(wrapped.calls == (registered ? 1 : 0))
                #expect(try await page.hasCardListener("keydown", at: "#probe") == registered)
                let afterFalse = try await page.json([Int].self, #"return JSON.stringify(["probe", "control"].map(id => document.getElementById(id).__removeFalseListenerProbe()));"#)
                let captured = ["boolean-true", "one", "string", "symbol-options", "bigint-options", "callable-options"].contains(scenario)
                #expect(afterFalse == (captured ? [1, 1] : [0, 0]))
                #expect(try await page.hasCardListener("keydown", at: "#probe") == captured)
                let afterCleanup = try await page.json([Int].self, #"return JSON.stringify(["probe", "control"].map(id => document.getElementById(id).__cleanupListenerProbe()));"#)
                #expect(afterCleanup == [0, 0])
                #expect(try await !page.hasCardListener("keydown", at: "#probe"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        @Test(arguments: [
            "type-throws", "options-throws", "symbol", "object-symbol", "invalid-callback",
            "invalid-receiver-add", "invalid-receiver-remove", "missing-type-add", "missing-type-remove",
            "missing-callback-add", "missing-callback-remove"
        ])
        func listenerFailuresKeepNativeConversionOrderAndDoNotRegister(_ scenario: String) async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<div id="probe"></div><div id="control"></div>"#)
                let results = try await page.json([ListenerArgumentResult].self, Self.listenerArgumentProbe, ["scenario": scenario])
                #expect(results.count == 2)
                let wrapped = try #require(results.first)
                #expect(wrapped == results.last, "\(scenario): \(results)")
                #expect(wrapped.outcome != "return:undefined")
                #expect(wrapped.calls == 0)
                #expect(try await !page.hasCardListener("keydown", at: "#probe"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// Indexed prototype hooks are installed only across the call under
        /// test. Probe storage and argument lists have no prototype, so the
        /// test itself cannot trigger those hooks.
        @Test func listenerArgumentsIgnoreInheritedIndexesBeforeNativeValidation() async throws {
            try await WebTestHarness.withPage { _, page async throws in
                let results = try await page.json([[String]].self, #"""
                    const frame = document.createElement("iframe");
                    frame.hidden = true;
                    frame.src = "about:blank";
                    await new Promise(resolve => { frame.onload = resolve; document.body.appendChild(frame); });
                    const native = frame.contentWindow.EventTarget.prototype;
                    const wrapped = EventTarget.prototype;
                    const results = [];
                    const define = Object.defineProperty, descriptor = Object.getOwnPropertyDescriptor;
                    const apply = Reflect.apply;
                    const target = document.createElement("div");
                    const callback = function () {};
                    for (const method of ["addEventListener", "removeEventListener"]) {
                      for (const count of [0, 1, 2, 3]) {
                        for (const invalid of [false, true]) {
                          const pair = [];
                          for (const api of [native, wrapped]) {
                            const probe = Object.create(null);
                            probe.trace = "";
                            const args = Object.create(null);
                            args.length = count;
                            if (count > 0) args[0] = { [Symbol.toPrimitive](hint) {
                              probe.trace += "type:" + hint + ";";
                              return "untracked-index-probe";
                            } };
                            if (count > 1) args[1] = callback;
                            if (count > 2) args[2] = { get capture() { probe.trace += "capture;"; return false; },
                              get once() { probe.trace += "once;"; return false; },
                              get passive() { probe.trace += "passive;"; return false; },
                              get signal() { probe.trace += "signal;"; return undefined; } };
                            const savedObject = Object.create(null), savedArray = Object.create(null);
                            for (const key of ["0", "2"]) {
                              savedObject[key] = descriptor(Object.prototype, key);
                              savedArray[key] = descriptor(Array.prototype, key);
                            }
                            const operation = api[method], receiver = invalid ? {} : target;
                            try {
                              for (const key of ["0", "2"]) {
                                define(Object.prototype, key, { configurable: true, get() {
                                  probe.trace += "object-get:" + key + ";";
                                  return undefined;
                                } });
                                define(Array.prototype, key, { configurable: true, set(value) {
                                  probe.trace += "array-set:" + key + ";";
                                  throw new Error("indexed array write");
                                } });
                              }
                              try { probe.outcome = "return:" + String(apply(operation, receiver, args)); }
                              catch (error) { probe.outcome = "throw:" + error.name; }
                            } finally {
                              for (const key of ["0", "2"]) {
                                if (savedArray[key]) define(Array.prototype, key, savedArray[key]);
                                else delete Array.prototype[key];
                                if (savedObject[key]) define(Object.prototype, key, savedObject[key]);
                                else delete Object.prototype[key];
                              }
                            }
                            pair.push(probe.outcome + ":" + probe.trace);
                            // Cleanup with pristine methods, after restoring prototypes.
                            apply(native.removeEventListener, target, ["untracked-index-probe", callback]);
                          }
                          results.push(pair);
                        }
                      }
                    }
                    frame.remove();
                    return JSON.stringify(results);
                    """#)
                #expect(results.count == 16)
                for pair in results {
                    #expect(pair.count == 2 && pair.first == pair.last, "native/wrapped: \(pair)")
                    #expect(pair.allSatisfy { !$0.contains("object-get:") && !$0.contains("array-set:") })
                }
            }
        }

        @Test(arguments: ["object", "array"])
        func trackedListenersIgnoreInheritedIndexes(_ prototype: String) async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(#"<div id="probe"></div><div id="control"></div>"#)
                let results = try await page.json([ListenerArgumentResult].self, #"""
                    const frame = document.createElement("iframe");
                    frame.hidden = true;
                    frame.src = "about:blank";
                    await new Promise(resolve => { frame.onload = resolve; document.body.appendChild(frame); });
                    const native = frame.contentWindow.EventTarget.prototype;
                    const wrapped = EventTarget.prototype;
                    const define = Object.defineProperty, descriptor = Object.getOwnPropertyDescriptor;
                    const apply = Reflect.apply;
                    const targetPrototype = prototype === "object" ? Object.prototype : Array.prototype;
                    const results = [];
                    for (const api of [wrapped, native]) {
                      const target = document.getElementById(api === wrapped ? "probe" : "control");
                      const probe = Object.create(null);
                      probe.trace = "";
                      probe.calls = 0;
                      const first = function () { probe.calls++; };
                      const second = function () { probe.calls++; };
                      const args = Object.create(null);
                      args.length = 2;
                      args[0] = "keydown";
                      args[1] = first;
                      const saved = descriptor(targetPrototype, "0");
                      const add = api.addEventListener, remove = api.removeEventListener;
                      let returned;
                      try {
                        define(targetPrototype, "0", { configurable: true,
                          get() { probe.trace += "get;"; return undefined; },
                          set(value) { probe.trace += "set;"; throw new Error("indexed entry write"); }
                        });
                        returned = apply(add, target, args);
                        args[1] = second;
                        returned = apply(add, target, args);
                      } catch (error) {
                        probe.error = error.name;
                      } finally {
                        if (saved) define(targetPrototype, "0", saved);
                        else delete targetPrototype[0];
                      }
                      probe.outcome = probe.error ? "throw:" + probe.error : "return:" + String(returned);
                      apply(api.dispatchEvent, target, [new KeyboardEvent("keydown", { bubbles: true })]);
                      results.push({ trace: [probe.trace], outcome: probe.outcome, calls: probe.calls });
                      target.__removeIndexedListenerProbe = function (last) {
                        apply(remove, target, ["keydown", last ? second : first]);
                        const before = probe.calls;
                        apply(api.dispatchEvent, target, [new KeyboardEvent("keydown", { bubbles: true })]);
                        return probe.calls - before;
                      };
                    }
                    frame.remove();
                    return JSON.stringify(results);
                    """#, ["prototype": prototype])
                #expect(results.count == 2)
                let wrapped = try #require(results.first)
                #expect(wrapped == results.last, "\(prototype): \(results)")
                #expect(wrapped.trace == [""] && wrapped.outcome == "return:undefined" && wrapped.calls == 2)
                #expect(try await page.hasCardListener("keydown", at: "#probe"))
                #expect(try await !page.hasCardListener("keydown", at: "#control"))
                let remaining = try await page.json([Int].self, #"return JSON.stringify(["probe", "control"].map(id => document.getElementById(id).__removeIndexedListenerProbe(false)));"#)
                #expect(remaining == [1, 1])
                #expect(try await page.hasCardListener("keydown", at: "#probe"))
                let removed = try await page.json([Int].self, #"return JSON.stringify(["probe", "control"].map(id => document.getElementById(id).__removeIndexedListenerProbe(true)));"#)
                #expect(removed == [0, 0])
                #expect(try await !page.hasCardListener("keydown", at: "#probe"))
                #expect(harness.diagnostics.isEmpty)
            }
        }

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

        /// A signal removes only the registration it came with: a listener
        /// removed and added again with a second signal stays (in WebKit and
        /// in the registry) when the first signal aborts.
        @Test func registryKeepsAListenerAddedAgainWithAnotherSignal() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <p id="p">x</p>
                    <script>
                    window.__h = function () { log("click"); };
                    window.__a = new AbortController();
                    window.__b = new AbortController();
                    document.addEventListener("click", __h, { signal: __a.signal });
                    document.removeEventListener("click", __h);
                    document.addEventListener("click", __h, { signal: __b.signal });
                    </script>
                    """)
                try await page.run("window.__a.abort();")
                try await page.run(#"document.getElementById("p").click();"#)
                #expect(try await page.log() == ["click"])
                #expect(try await page.hasCardListener("tap", at: "#p"))
                try await page.run("window.__b.abort();")
                #expect(try await !page.hasCardListener("tap", at: "#p"))
            }
        }

        /// WebKit ignores a second add of the same type, callback and capture,
        /// so aborting the second add's signal leaves the first registration.
        @Test func registryIgnoresTheSignalOfARepeatedAdd() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <p id="p">x</p>
                    <script>
                    window.__h = function () { log("key"); };
                    window.__a = new AbortController();
                    window.__b = new AbortController();
                    document.addEventListener("keydown", __h, { signal: __a.signal });
                    document.addEventListener("keydown", __h, { signal: __b.signal });
                    </script>
                    """)
                try await page.run("window.__b.abort();")
                try await page.run(#"document.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));"#)
                #expect(try await page.log() == ["key"])
                #expect(try await page.hasCardListener("keydown"))
                try await page.run("window.__a.abort();")
                #expect(try await !page.hasCardListener("keydown"))
            }
        }

        /// A `once` listener that fired is gone from WebKit, so adding it
        /// again registers it anew; the first add's signal must not take that
        /// registration out of the registry.
        @Test func registryKeepsAOnceListenerAddedAgainAfterItFired() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion(
                    """
                    <p id="p">x</p>
                    <script>
                    window.__h = function () { log("click"); };
                    window.__a = new AbortController();
                    window.__b = new AbortController();
                    document.addEventListener("click", __h, { once: true, signal: __a.signal });
                    </script>
                    """)
                try await page.run(
                    #"document.getElementById("p").click(); document.addEventListener("click", __h, { signal: __b.signal }); __a.abort();"#)
                try await page.run(#"document.getElementById("p").click();"#)
                #expect(try await page.log() == ["click", "click"])
                #expect(try await page.hasCardListener("tap", at: "#p"))
                try await page.run("window.__b.abort();")
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
