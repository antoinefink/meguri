import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// The page-world surface (specs/render/card-runtime.md C9, C10, C16.3;
    /// S15; R505–R517, R563, R2038; AE18 page part).
    @Suite struct GlobalsSurfaceTests {
        /// C9.1, per platform.
        static func allowlist(_ platform: CardRuntime.Platform) -> Set<String> {
            var names: Set<String> = [
                "$", "jQuery", "anki", "require", "onUpdateHook", "onShownHook", "_showQuestion", "_showAnswer",
                "_updateQA", "_drawFlag", "_drawMark", "getTypedAnswer", "_typeAnsPress", "MathJax",
            ]
            if platform == .mac { names.formUnion(["ankiPlatform", "pycmd", "bridgeCommand"]) }
            return names
        }

        /// C9.2 (R516, R517): other clients' globals, never defined.
        static let neverDefined = [
            "AnkiDroidJS", "showAnswer", "buttonAnswerEase1", "buttonAnswerEase2", "buttonAnswerEase3", "buttonAnswerEase4",
            "study", "setInnerHTML", "_queueAction", "_emulateMobile", "_blockDefaultDragDropBehavior", "qt", "QWebChannel",
        ]

        static let windowNames = "return JSON.stringify(Object.getOwnPropertyNames(window));"

        /// Five sides whose scripts use jQuery and the hooks and define no
        /// globals of their own.
        static let busySides = (1...5).map { index in
            """
            <p class="n">side \(index)</p>
            <script>
            (function () {
              $("#qa .n").css("color", "rgb(1, 2, 3)").attr("data-i", "\(index)");
              $(document).on("click.side", function () {});
              onUpdateHook.push(function () { return $.Deferred().resolve().promise(); });
              onShownHook.push(function () { require("anki/reviewer").onShownHook.length; });
            })();
            </script>
            """
        }

        // MARK: GS1, D8

        @Test(arguments: CardRuntime.Platform.allCases)
        func globalsMatchAllowlist(platform: CardRuntime.Platform) async throws {
            let baseline = WebTestHarness(platform: platform, runtime: false)
            defer { baseline.close() }
            let baselineNames = Set(try await baseline.openPage().json([String].self, Self.windowNames))

            let harness = WebTestHarness(platform: platform)
            defer { harness.close() }
            let page = try await harness.openPage()
            let fresh = Set(try await page.json([String].self, Self.windowNames))
            #expect(fresh.subtracting(baselineNames) == Self.allowlist(platform))
            #expect(baselineNames.subtracting(fresh).isEmpty)

            for (index, side) in Self.busySides.enumerated() {
                if index.isMultiple(of: 2) {
                    try await page.showPresentedQuestion(side)
                } else {
                    try await page.showPresentedAnswer(side)
                }
            }
            #expect(try await page.string(##"$("#qa .n").attr("data-i")"##) == "5")
            let after = Set(try await page.json([String].self, Self.windowNames))
            #expect(after.subtracting(baselineNames) == Self.allowlist(platform))
            #expect(baselineNames.subtracting(after).isEmpty)
            #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
        }

        /// C9.1: assignment for everything but `pycmd` and `bridgeCommand`,
        /// which are as by `var`.
        @Test func globalsAreDefinedAsAssignmentsOrAsByVar() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                struct Descriptor: Decodable, Equatable {
                    let writable: Bool
                    let enumerable: Bool
                    let configurable: Bool
                }
                let descriptors = try await page.json(
                    [String: Descriptor].self,
                    """
                    var result = {};
                    names.forEach(function (name) {
                      var d = Object.getOwnPropertyDescriptor(window, name);
                      result[name] = { writable: d.writable, enumerable: d.enumerable, configurable: d.configurable };
                    });
                    return JSON.stringify(result);
                    """, ["names": Array(Self.allowlist(.mac))])
                for (name, descriptor) in descriptors {
                    let byVar = name == "pycmd" || name == "bridgeCommand"
                    #expect(descriptor == Descriptor(writable: true, enumerable: true, configurable: !byVar), "\(name)")
                }
                #expect(try await page.bool("window.pycmd === window.bridgeCommand && window.$ === window.jQuery"))
            }
        }

        /// C9.4: the runtime creates no global lexical bindings: its top level
        /// is one expression statement that calls a function expression.
        @Test(arguments: ["runtime.js", "image-occlusion.js"])
        func runtimeDeclaresNothingAtItsTopLevel(asset: String) throws {
            let source = String(decoding: try #require(CardRuntime.asset(at: asset)).data, as: UTF8.self)
            var lines = source.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
            // The leading comment block.
            while let first = lines.first, first.hasPrefix("//") || first.isEmpty { lines.removeFirst() }
            while let last = lines.last, last.isEmpty { lines.removeLast() }
            #expect(lines.first == "(function () {")
            #expect(lines.last == "})();")
            // Everything between is indented: nothing else starts at column 0.
            let body = lines.dropFirst().dropLast()
            #expect(body.allSatisfy { $0.isEmpty || $0.hasPrefix("  ") }, "\(body.filter { !$0.isEmpty && !$0.hasPrefix("  ") })")
        }

        // MARK: GS2, R517, D8

        @Test(arguments: CardRuntime.Platform.allCases)
        func otherClientsGlobalsAreNeverDefined(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                try await page.showPresentedQuestion("<p>x</p>")
                let names = Self.neverDefined
                let types = try await page.json(
                    [String].self, "return JSON.stringify(names.map(function (n) { return typeof window[n]; }));", ["names": names])
                #expect(types == Array(repeating: "undefined", count: names.count))
            }
        }

        /// C9.2: Meguri's page scripts do not use these names even for their
        /// own locals.
        @Test func pageScriptsNeverNameOtherClientsGlobals() throws {
            let runtime = String(decoding: try #require(CardRuntime.asset(at: "runtime.js")).data, as: UTF8.self)
            let occlusion = String(decoding: try #require(CardRuntime.asset(at: "image-occlusion.js")).data, as: UTF8.self)
            for (script, source) in [("runtime.js", runtime), ("image-occlusion.js", occlusion), ("interaction.js", CardRuntime.interactionScript)] {
                for name in Self.neverDefined {
                    let pattern = try Regex("(^|[^A-Za-z0-9_$])" + NSRegularExpression.escapedPattern(for: name) + "([^A-Za-z0-9_$]|$)")
                    #expect(!source.contains(pattern), "\(script) names \(name)")
                }
            }
        }

        // MARK: GS3

        @Test func cardsMayShadowAssignedGlobals() async throws {
            try await WebTestHarness.withPage { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>")
                try await page.showPresentedAnswer("<script>let $ = 1; log($)</script>")
                #expect(try await page.log() == ["1"])
                #expect(harness.diagnostics.isEmpty)
            }
        }

        @Test func letPycmdThrowsOnTheMacOnly() async throws {
            try await WebTestHarness.withPage(platform: .mac) { harness, page async throws in
                try await page.showPresentedQuestion("<script>let pycmd = 1; log('ran')</script>")
                #expect(try await page.log().isEmpty)
                #expect((harness.diagnostics.first?["message"] as? String)?.hasPrefix("SyntaxError") == true)
            }
            try await WebTestHarness.withPage(platform: .iPhone) { harness, page async throws in
                try await page.showPresentedQuestion("<script>let pycmd = 1; log('ran')</script>")
                #expect(try await page.log() == ["ran"])
                #expect(harness.diagnostics.isEmpty)
            }
        }

        // MARK: GS4, C10.7

        @Test(arguments: CardRuntime.Platform.allCases)
        func requireServesAnkisPackages(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                struct Result: Decodable {
                    let reviewerHooks: Bool
                    let sameObject: Bool
                    let unknown: String
                    let packages: [String]
                    let has: [Bool]
                    let packagesRequire: Bool
                }
                let result = try await page.json(
                    Result.self,
                    """
                    var unknown;
                    try { require("x"); unknown = "no error"; } catch (e) { unknown = e.constructor.name + ": " + e.message; }
                    var p = require("anki/packages");
                    return JSON.stringify({
                      reviewerHooks: require("anki/reviewer").onShownHook === onShownHook && require("anki/reviewer").onUpdateHook === onUpdateHook,
                      sameObject: require("anki/reviewer") === require("anki/reviewer"),
                      unknown: unknown,
                      packages: p.listPackages(),
                      has: [p.hasPackages(), p.hasPackages("anki/reviewer"), p.hasPackages("anki/reviewer", "x"), p.hasPackages("anki/bridgecommand")],
                      packagesRequire: p.require === require,
                    });
                    """)
                #expect(result.reviewerHooks && result.sameObject && result.packagesRequire)
                #expect(result.unknown == #"Error: Cannot require "x" at runtime."#)
                if platform == .mac {
                    #expect(result.packages == ["anki/packages", "anki/bridgecommand", "anki/reviewer"])
                    #expect(result.has == [true, true, false, true])
                } else {
                    #expect(result.packages == ["anki/packages", "anki/reviewer"])
                    #expect(result.has == [true, true, false, false])
                }
            }
        }

        // MARK: GS4b, C10.7

        /// `require("anki/bridgecommand").bridgeCommand(command, callback)`
        /// calls the page's current global `bridgeCommand`, looked up at call
        /// time, with exactly those two arguments, and returns `undefined`.
        @Test func bridgecommandPackageCallsTheCurrentGlobal() async throws {
            try await WebTestHarness.withPage(platform: .mac) { harness, page async throws in
                try await page.announce(card: 8, generation: 1)
                try await page.showPresentedQuestion("<p>q</p>")
                let returned = try await page.run(
                    """
                    var r = require("anki/bridgecommand").bridgeCommand("ans", function () { log("cb:" + arguments.length + ":" + arguments[0]); });
                    log("after");
                    return typeof r;
                    """)
                #expect(returned as? String == "undefined")
                #expect(try await page.log(count: 2) == ["after", "cb:1:null"])
                try await Task.sleep(for: .milliseconds(100))
                #expect(try await page.log().count == 2)
                let command = try await harness.signal("command", from: page)
                #expect(command["command"] as? String == "ans" && command.side == "question" && command.card == 8)

                let replaced = try await page.run(
                    """
                    window.__log = [];
                    var bc = require("anki/bridgecommand").bridgeCommand;
                    window.bridgeCommand = function () {
                      for (var i = 0; i < arguments.length; i++) log(arguments[i]);
                      return 7;
                    };
                    return typeof bc("x");
                    """)
                #expect(replaced as? String == "undefined")
                #expect(try await page.log() == ["x", "undefined"])
                #expect(harness.signals("command").count == 1)
            }
        }

        // MARK: GS4c, C10.7, C15

        /// Names every object inherits are not packages.
        @Test(arguments: CardRuntime.Platform.allCases)
        func inheritedNamesAreNotPackages(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                struct Result: Decodable {
                    let required: [String]
                    let has: [Bool]
                }
                let result = try await page.json(
                    Result.self,
                    """
                    function attempt(name) {
                      try { require(name); return "no error"; } catch (e) { return e.constructor.name + ": " + e.message; }
                    }
                    var p = require("anki/packages");
                    return JSON.stringify({
                      required: [attempt("toString"), attempt("constructor"), attempt("__proto__")],
                      has: [p.hasPackages("constructor"), p.hasPackages("toString"), p.hasPackages("anki/reviewer", "__proto__"), p.hasPackages()],
                    });
                    """)
                #expect(result.required == [
                    #"Error: Cannot require "toString" at runtime."#,
                    #"Error: Cannot require "constructor" at runtime."#,
                    #"Error: Cannot require "__proto__" at runtime."#,
                ])
                #expect(result.has == [false, false, false, true])
            }
        }

        // MARK: GS5, AE18, R506, R507

        @Test func pycmdPostsTheCommandAndCallsBackLaterWithNull() async throws {
            try await WebTestHarness.withPage(platform: .mac) { harness, page async throws in
                try await page.announce(card: 5, generation: 1)
                try await page.showPresentedQuestion("<p>q</p>")
                let returned = try await page.run(
                    """
                    var r = pycmd("ans", function () { log("cb:" + arguments.length + ":" + arguments[0]); });
                    log("after");
                    return r;
                    """)
                #expect(returned as? Bool == false)
                #expect(try await page.log(count: 2) == ["after", "cb:1:null"])
                try await Task.sleep(for: .milliseconds(100))
                #expect(try await page.log().count == 2)
                let command = try await harness.signal("command", from: page)
                #expect(command["command"] as? String == "ans" && command.side == "question" && command.card == 5)
            }
        }

        // MARK: GS6, D4

        @Test func pycmdIgnoresOtherCommands() async throws {
            try await WebTestHarness.withPage(platform: .mac) { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>")
                let result = try await page.run(
                    #"var a = pycmd("ease3"); var b = bridgeCommand("play:q:1"); var c = pycmd(42, "not a function"); return [a, b, c].join(",");"#)
                #expect(result as? String == "false,false,false")
                try await harness.signal("command", count: 3, from: page)
                #expect(harness.signals("command").map { $0["command"] as? String } == ["ease3", "play:q:1", "42"])
                #expect(harness.diagnostics.isEmpty)
            }
        }

        /// D17: the page passes `ans` from the answer side on, with its side;
        /// native ignores it (U67).
        @Test func ansOnAnswerIgnored() async throws {
            try await WebTestHarness.withPage(platform: .mac) { harness, page async throws in
                try await page.showPresentedQuestion("<p>q</p>")
                try await page.showPresentedAnswer(#"<p>a</p><script>pycmd("ans");</script>"#)
                let command = try await harness.signal("command", from: page)
                #expect(command["command"] as? String == "ans" && command.side == "answer")
                // No update was started by it.
                try await Task.sleep(for: .milliseconds(200))
                #expect(harness.signals("shown").count == 2)
            }
        }

        // MARK: GS7, D5

        @Test(arguments: [CardRuntime.Platform.iPhone, .iPad])
        func iosHasNoDesktopBridge(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                let result = try await page.run(
                    """
                    var didReject;
                    try { require("anki/bridgecommand"); didReject = false; } catch (e) { didReject = true; }
                    return [typeof pycmd, typeof bridgeCommand, typeof ankiPlatform, String(didReject)].join(",");
                    """)
                #expect(result as? String == "undefined,undefined,undefined,true")
            }
        }

        // MARK: GS8

        @Test func macHasTheDesktopPlatformAndNoWebkitNamespace() async throws {
            try await WebTestHarness.withPage(platform: .mac, log: false) { harness, page async throws in
                #expect(try await page.string("window.ankiPlatform") == "desktop")
                #expect(try await page.string("typeof webkit") == "undefined")
            }
        }

        // MARK: GS9, GS10: offline MathJax (owner supersedes C10.8/D6)

        @Test func reviewExtraMathRequestSurvivesTheFirstSide() async throws {
            let extra = #"""
                <aside id="pageMath">\(s+1\)</aside><script>
                window.__pageMath = MathJax.typesetPromise([document.getElementById('pageMath')]);
                // Arrive in the same script turn, before the engine can load.
                _showQuestion('<p id="current">First ordinary side</p>', '', 'card');
                </script>
                """#
            try await WebTestHarness.withPage(reviewExtra: extra, log: false) { harness, page async throws in
                try await page.present()
                try await harness.signal("shown", count: 1, from: page)
                try await page.run("await __pageMath;")
                #expect(try await page.bool("document.querySelector('#pageMath svg path') !== null && MathJax.startup.document.getMathItemsWithin([document.getElementById('pageMath')]).length === 1"))
                #expect(try await page.string("document.getElementById('qa').textContent") == "First ordinary side")
                #expect(try await page.bool("!document.querySelector('#qa mjx-container')"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func mathJaxTemplateAPIProducesRealDOM() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <p id="m">\(x^2\)</p><script>
                    (function () {
                      var api = MathJax;
                      var cardRequire = require;
                      onUpdateHook.push(function () { return api.typesetPromise([document.getElementById('m')]); });
                      onUpdateHook.push(async function () {
                        await MathJax.startup.promise;
                        await api.typesetPromise([document.getElementById('m')]);
                        document.getElementById('m').dataset.requireIntact = String(
                          require === cardRequire && require('anki/reviewer').onUpdateHook === onUpdateHook);
                      });
                    })();
                    </script>
                    """#)
                #expect(try await page.bool("document.querySelector('#m mjx-container svg path') !== null"))
                #expect(try await page.bool("MathJax.version === '4.1.3'"))
                #expect(try await page.bool("MathJax.startup.document.inputJax[0].options.packages.slice().sort().join(',') === 'ams,autoload,base,mhchem,require'"))
                #expect(try await page.string("document.getElementById('m').dataset.requireIntact") == "true")
                #expect(try await page.string("Array.from(MathJax.startup.document.math)[0].math") == "x^2")
                #expect(try await page.bool("document.querySelector('#m mjx-assistive-mml math') !== null"))
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"))
                try await page.run("await MathJax.startup.promise; MathJax.typesetClear([document.getElementById('m')]);")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 0"))
                let requestsBeforeRefusals = harness.media.requests.count
                try await page.run("""
                    window.__loaderScripts = [];
                    window.__loaderObserver = new MutationObserver(function (records) {
                      records.forEach(function (record) {
                        record.addedNodes.forEach(function (node) {
                          // Mutation records retain scripts even after the loader removes them.
                          if (node.nodeName === 'SCRIPT') __loaderScripts.push(node.src);
                        });
                      });
                    });
                    __loaderObserver.observe(document.head, { childList: true });
                    """)
                for path in ["https://unbundled.invalid/component.js", "meguri-media://other/_anki/mathjax/tex-svg.js",
                             "../runtime.js", "%2e%2e/runtime.js", "tex-svg.js?remote=1", "tex-svg.js#fragment",
                             "meguri-media://user:password@media/_anki/mathjax/tex-svg.js"] {
                    let reason = try await page.run("""
                        try { await MathJax.config.loader.require(path); return 'no error'; }
                        catch (error) { return error.name + ': ' + error.message; }
                        """, ["path": path])
                    #expect(reason as? String == "Error: Unbundled MathJax component refused", "Loader must refuse \(path)")
                    #expect(try await page.bool("__loaderScripts.length === 0"), "Loader must not insert a script for \(path)")
                }
                try await page.run("""
                    await MathJax.config.loader.require('font/svg/dynamic/fraktur.js');
                    await MathJax.typesetPromise([document.getElementById('m')]);
                    """)
                #expect(try await page.bool("document.querySelector('#m mjx-container svg path') !== null"))
                let scripts = try await page.json([String].self, "__loaderObserver.disconnect(); return JSON.stringify(__loaderScripts);")
                #expect(scripts == ["meguri-media://media/_anki/mathjax/font/svg/dynamic/fraktur.js"])
                #expect(harness.media.requests.dropFirst(requestsBeforeRefusals).map(\.path) == ["/_anki/mathjax/font/svg/dynamic/fraktur.js"])
                #expect(harness.remote.requests.isEmpty)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [false, true])
        func warmMathTypesetsInsideWhenReady(awaitResult: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p><script>window.__mathAPI = MathJax;</script>"#)
                try await page.run("""
                    var ready = MathJax.whenReady;
                    window.__templateReadyCalls = 0;
                    MathJax.whenReady = function (callback) {
                      __templateReadyCalls++;
                      return ready.call(this, callback);
                    };
                    """)
                let callback = awaitResult
                    ? "async function () { await __mathAPI.typesetPromise([m]); }"
                    : "function () { return MathJax.typesetPromise([m]); }"
                try await page.showPresentedAnswer("""
                    <p id="m">\\(x+2\\)</p><script>
                    onUpdateHook.push(function () {
                      var m = document.getElementById('m');
                      return MathJax.whenReady(\(callback)).then(function () {
                        m.dataset.ready = String(!!m.querySelector('svg path'));
                      });
                    });
                    </script>
                    """)
                #expect(try await page.string("document.getElementById('m').dataset.ready") == "true")
                #expect(try await page.bool("__templateReadyCalls === 1"))
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func templateMathButtonAfterRevealGetsAFreshDeadline() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p><script>window.__mathAPI = MathJax;</script>"#)
                try await page.showPresentedAnswer(#"""
                    <p id="m"></p><button id="render">Show formula</button><script>
                    document.getElementById('render').onclick = function () {
                      var m = document.getElementById('m');
                      m.textContent = '\\(𝔄\\)';
                      window.__buttonMath = Promise.all([
                        MathJax.startup.promise, __mathAPI.typesetPromise([m])
                      ]);
                    };
                    </script>
                    """#)
                // A normal card button remains usable long after reveal. The
                // deferred glyph makes an expired face deadline observable.
                harness.media.delay("/_anki/mathjax/font/svg/dynamic/fraktur.js", by: .milliseconds(100))
                try await Task.sleep(for: .seconds(9))
                try await page.run("document.getElementById('render').click(); await __buttonMath;")
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && document.getElementById('qa').style.opacity === '1'"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func cleanupTimeoutConsumesTheCurrentMathFaceBudget() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                try await page.run("""
                    var held = new Promise(function (resolve) { window.__releaseCleanup = resolve; });
                    MathJax.whenReady(function () { window.__cleanupBlocked = true; return held; });
                    """)
                try await harness.until("cleanup queue blocked") { try await page.bool("window.__cleanupBlocked === true") }
                try await page.announce(card: 93, generation: 2)
                // The harness's ten-second shown deadline admits one eight-
                // second cleanup wait, but cannot admit a second math wait.
                try await page.showPresentedAnswer(#"<p id="current">\(y+1\)</p>"#)
                #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && !document.querySelector('#current svg')"))
                #expect(harness.diagnostics.contains {
                    $0.card == 93 && $0["kind"] as? String == "math"
                        && ($0["message"] as? String)?.contains("timed out") == true
                })
                try await page.run("__releaseCleanup(); await MathJax.whenReady(function () {});")
                #expect(try await page.bool("document.querySelector('#current svg path') !== null && MathJax.startup.document.getMathItemsWithin([document.getElementById('current')]).length === 1"))
                try await page.showPresentedQuestion(#"<p id="next">\(z+1\)</p>"#)
                #expect(try await page.bool("document.querySelector('#next svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"))
            }
        }

        @Test func upstreamReadyWorkCannotHoldCardReplacement() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                try await page.run("""
                    window.__oldQA = document.getElementById('qa');
                    window.__oldTarget = __oldQA.firstElementChild;
                    window.__rootEvents = 0;
                    __oldQA.addEventListener('card-root-check', function () { __rootEvents++; });
                    var released = new Promise(function (resolve) { window.__releaseMath = resolve; });
                    MathJax.whenReady(function () {
                      window.__upstreamEntered = true;
                      return released.then(function () { __oldTarget.dataset.finished = 'true'; });
                    });
                    """)
                try await harness.until("upstream ready callback") { try await page.bool("window.__upstreamEntered === true") }
                try await page.announce(card: 89, generation: 2)
                try await page.showPresentedAnswer("<p id='current'>Still reviewable</p>")
                #expect(try await page.bool("__oldQA.isConnected && document.getElementById('qa') === __oldQA && !__oldTarget.isConnected && __oldQA.style.opacity === '1'"))
                try await page.run("document.getElementById('qa').dispatchEvent(new Event('card-root-check'));")
                #expect(try await page.bool("__rootEvents === 1"))
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && $0.card == 89 && $0.generation == 2
                        && ($0["message"] as? String)?.contains("timed out") == true
                })
                let diagnostics = harness.diagnostics.count
                try await page.run("__releaseMath(); await MathJax.whenReady(function () {});")
                #expect(try await page.string("document.getElementById('qa').textContent") == "Still reviewable")
                #expect(try await page.bool("__oldTarget.dataset.finished === 'true' && !__oldQA.dataset.finished && Array.from(MathJax.startup.document.math).length === 0"))
                try await page.announce(card: 90, generation: 3)
                try await page.showPresentedQuestion(#"<p id="next">\(y+1\)</p>"#)
                #expect(try await page.bool("document.querySelector('#next svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"))
                try await page.run("document.getElementById('qa').dispatchEvent(new Event('card-root-check'));")
                #expect(try await page.bool("document.getElementById('qa') === __oldQA && __rootEvents === 2"))
                #expect(harness.diagnostics.count == diagnostics)
            }
        }

        @Test func bundledLoaderUsesCapturedURLGetters() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                let captured = try await page.run("""
                    await MathJax.startup.promise;
                    var names = ['protocol', 'host', 'search', 'hash', 'username', 'password', 'pathname', 'href'];
                    var saved = names.map(function (name) { return Object.getOwnPropertyDescriptor(URL.prototype, name); });
                    var reads = 0;
                    names.forEach(function (name, index) {
                      Object.defineProperty(URL.prototype, name, { configurable: true, get: function () {
                        reads++; return saved[index].get.call(this);
                      } });
                    });
                    try {
                      await MathJax.config.loader.require('font/svg/dynamic/fraktur.js');
                      return reads === 0;
                    } finally {
                      names.forEach(function (name, index) { Object.defineProperty(URL.prototype, name, saved[index]); });
                    }
                    """)
                #expect(captured as? Bool == true)
                #expect(harness.media.requests(for: "/_anki/mathjax/font/svg/dynamic/fraktur.js").count == 1)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func authoredMathScopeAndSideCleanup() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <p id="m">\(x+1\)</p><script>
                    var sibling = document.createElement('div');
                    sibling.id = 'authoredMath';
                    sibling.textContent = '\\(y+1\\)';
                    document.body.appendChild(sibling);
                    onUpdateHook.push(function () { return MathJax.typesetPromise(); });
                    </script>
                    """#)
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && document.querySelector('#authoredMath svg path') !== null"))
                try await page.showPresentedAnswer(#"""
                    <p id="answer">\(x+2\)</p><p id="automatic">\(x+3\)</p><script>
                    MathJax.typesetClear([document.getElementById('authoredMath')]);
                    document.getElementById('authoredMath').textContent = '\\(y+2\\)';
                    MathJax.typeset([document.getElementById('answer')]);
                    document.getElementById('answer').dataset.targeted = String(!document.querySelector('#authoredMath mjx-container'));
                    </script>
                    """#)
                #expect(try await page.string("document.getElementById('answer').dataset.targeted") == "true")
                #expect(try await page.bool("document.querySelector('#authoredMath mjx-container') === null && document.querySelector('#answer svg path') !== null && document.querySelector('#automatic svg path') !== null"))
                try await page.run("MathJax.typeset();")
                #expect(try await page.bool("document.querySelector('#authoredMath svg path') !== null && Array.from(MathJax.startup.document.math).length === 3"))
                try await page.showPresentedQuestion("<p>Next ordinary card</p>")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1 && Array.from(MathJax.startup.document.math)[0].math === 'y+2'"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [#"\pu{9.8 m/s2}"#, #"\ce{\pu{9.8 m/s2}}"#])
        func physicalUnitsRetainSmallInputParsing(units: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion("<p id='unit'>\\(\(units)\\)</p><p id='chem'>\\(\\ce{2H2 + O2 -> 2H2O}\\)</p>")
                #expect(try await page.bool("document.querySelector('#unit svg path') !== null && document.querySelector('#chem svg path') !== null && !document.querySelector('#qa [data-mml-node=merror]')"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [
            #"\ce{\ce{H2O}}"#, #"\ce{A ->[\ce{H2O}] B}"#,
            #"\ce{A ->[\ce{H+}][\ce{OH-}] B}"#,
            #"\ce{^{14}_{6}C -> ^{14}_{7}N + e-}"#,
            #"\ce{\ce{^{14}_{6}C}}"#,
            #"\ce{A ->[\ce{H2O ->[heat] H2O}] B}"#,
        ])
        func ordinaryNestedChemistryAndIsotopesRender(formula: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion("<p>\\(\(formula)\\)</p><p>\\(x^2+1\\)</p>")
                try await MathRenderingProbe.expect(in: page.webView, count: 2, label: formula)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test func shortNestedUnitsKeepTheirOwnAllowanceInALongerReaction() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let formula = #"\ce{\text{glucose} ->[\text{hexokinase}][\ce{ATP -> ADP}] \text{glucose-6-phosphate} ->[\text{phosphoglucose isomerase}] \text{fructose-6-phosphate} ->[\text{phosphofructokinase}][\ce{ATP -> ADP}] \text{fructose-1,6-bisphosphate} ->[\text{aldolase}][\pu{310 K}] \text{triose phosphates}}"#
                #expect(formula.count > 256)
                try await page.showPresentedQuestion("<p>\\(\(formula)\\)</p>")
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: "reaction with short unit")
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [#"\sideset{_a^b}{_c^d}{\sum}_{n=1}^{k} n"#,
                          #"\sideset{}{'}{\prod}_{n=1}^{k} n"#])
        func ordinaryAMSSidesetsRender(formula: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion("<p>\\[\(formula)\\]</p>")
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: formula)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [false, true])
        func templateCallsBeforeStartupRetainThePackagePolicy(synchronous: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let method = synchronous ? "typeset" : "typesetPromise"
                try await page.showPresentedQuestion(#"""
                    <p id="good">\(\require{ams}\sideset{}{'}{\sum} + \ce{A ->[\ce{H2O}] B}\)</p>
                    <p id="unsupported">\(\require{cancel}\cancel{x}\)</p>
                    <script>
                    var initialMathAPI = MathJax;
                    onUpdateHook.push(function () { return initialMathAPI.METHOD([document.getElementById('qa')]); });
                    </script>
                    """#.replacingOccurrences(of: "METHOD", with: method))
                try await page.run("await MathJax.startup.promise; await initialMathAPI.typesetPromise([document.getElementById('qa')]);")
                #expect(try await page.bool("!!document.querySelector('#good svg path') && !!document.querySelector('#good mjx-assistive-mml math') && !document.querySelector('#good [data-meguri-math-error]')"))
                #expect(try await page.string("document.querySelector('#unsupported code').textContent") == #"\(\require{cancel}\cancel{x}\)"#)
                #expect(try await page.bool("MathJax.startup.document.inputJax[0].options.packages.slice().sort().join(',') === 'ams,autoload,base,mhchem,require'"))
                #expect(!harness.media.requests.contains { $0.path.hasSuffix("/cancel.js") })
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" })
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test(arguments: ["ce", "pu"])
        func mhchemFractionBudgetPreservesReview(command: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let tex = "\\\(command)"
                try await page.showPresentedQuestion("<p>\\(\(tex){\\text{a/b/c/d/e/f}}\\)</p>")
                #expect(try await page.bool("document.querySelector('#qa [data-meguri-math-error]') !== null && document.getElementById('qa').style.opacity === '1'"))
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && ($0["message"] as? String)?.contains("MeguriUnitLimit") == true
                }, "\(harness.diagnostics.map(\.body))")
                try await page.showPresentedAnswer(#"<p>\(\ce{2H2 + O2 -> 2H2O}\)</p>"#)
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: "chemistry after budget refusal")
            }
        }

        @Test func warmMathTypesetProducesDOMSynchronously() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p><script>window.__mathAPI = MathJax;</script>"#)
                try await page.announce(card: 87, generation: 2)
                try await page.showPresentedAnswer(#"""
                    <p id="m">\(x+1\)</p><p id="retained">\(x+1\)</p><p id="later">\(x+1\)</p>
                    <script>
                    onUpdateHook.push(function () {
                      var m = document.getElementById('m');
                      var result = MathJax.typeset([m]);
                      m.dataset.immediate = String(result === undefined && !!m.querySelector('svg path'));
                      __mathAPI.typeset([document.getElementById('retained')]);
                      m.dataset.retained = String(!!document.querySelector('#retained svg path'));
                      m.dataset.scoped = String(!document.querySelector('#later mjx-container'));
                    });
                    </script>
                    """#)
                for attribute in ["immediate", "retained", "scoped"] {
                    #expect(try await page.string("document.getElementById('m').dataset.\(attribute)") == "true")
                }
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
                // Ordinary TeX errors still report the requesting side.
                try await page.run(#"""
                    var m = document.getElementById('m');
                    MathJax.typesetClear([m]);
                    m.textContent = '\\(\\frac{1}\\)';
                    MathJax.typeset([m]);
                    """#)
                #expect(try await page.bool("document.querySelector('#m [data-meguri-math-error]') !== null"))
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && $0.card == 87 && $0.generation == 2 && $0.side == "answer"
                        && ($0["message"] as? String)?.contains("Missing") == true
                }, "\(harness.diagnostics.map(\.body))")
            }
        }

        // Every formerly shipped optional package, including those compiled
        // into tex-svg.js, must be refused before any component request.
        @Test(arguments: [
            "action", "amscd", "autoload", "bbm", "bboldx", "bbox", "begingroup", "boldsymbol",
            "braket", "bussproofs", "cancel", "cases", "centernot", "color", "colortbl", "colorv2",
            "configmacros", "dsfont", "empheq", "enclose", "extpfeil", "fontsizev3", "gensymb", "html",
            "mathtools", "newcommand", "noerrors", "noundefined", "physics", "require", "tagformat",
            "textcomp", "textmacros", "unicode", "units", "upgreek", "verb",
        ])
        func optionalPackageRequireIsRefused(package: String) async throws {
            try await checkUnsupportedMath(#"\(\require{"# + package + #"}x+1\)"#)
        }

        @Test(arguments: [
            #"\(\cancel{x}\)"#, #"\[\begin{CD}A @>f>> B\end{CD}\]"#,
            #"\(\bbox[red]{x}\)"#, #"\(\boldsymbol{x}\)"#, #"\(\bra{x}\)"#,
            #"\(\color{red}x\)"#, #"\(\enclose{circle}{x}\)"#, #"\(\style{color:red}{x}\)"#,
            #"\(\unicode{65}\)"#, #"\(\verb|x|\)"#, #"\(\overbracket{x+y}\)"#,
            #"\(\mqty{\imat{2}}\)"#, #"\(\dmat{a,b}\)"#, #"\(\braket{x}{y}\)"#,
            #"\(\def\cardmath{x}\cardmath\)"#, #"\(\let\cardmath\ce\cardmath{H2O}\)"#,
            #"\(\newcommand{\cardmath}{x}\cardmath\)"#, #"\(\undefinedcardcommand{x}\)"#,
            #"\(\renewcommand{\cardmath}{x}\cardmath\)"#,
        ])
        func unsupportedCommandsKeepOriginalSourceAndNeighbors(source: String) async throws {
            try await checkUnsupportedMath(source)
        }

        private func checkUnsupportedMath(_ source: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion("""
                    <p id="before">\\(x+1\\)</p><p id="bad">\(source)</p>
                    <p id="after">\\[\\begin{cases}x &amp; x>0\\\\0 &amp; x=0\\end{cases}\\]</p>
                    <textarea id="field">\(source)</textarea><button id="action">Continue studying</button>
                    <script>document.getElementById('action').onclick = function () { this.textContent = 'Ready'; };</script>
                    """)
                let observed = try await page.run("""
                    var bad = document.getElementById('bad');
                    var failure = bad.querySelector('[data-meguri-math-error]');
                    var code = failure && failure.querySelector('code');
                    return !!failure && failure.getAttribute('role') === 'group' &&
                      failure.getAttribute('aria-label') === 'Formula could not be rendered' &&
                      failure.querySelector('strong').textContent.includes('Formula could not be rendered') &&
                      code.textContent === source && code.getBoundingClientRect().height > 0 &&
                      getComputedStyle(code).visibility === 'visible' && !bad.querySelector('svg') &&
                      !!document.querySelector('#before svg path') && !!document.querySelector('#after svg path') &&
                      !!document.querySelector('#before mjx-assistive-mml math') &&
                      !!document.querySelector('#after mjx-assistive-mml math') &&
                      document.getElementById('field').value === source &&
                      document.getElementById('qa').style.opacity === '1';
                    """, ["source": source])
                #expect(observed as? Bool == true, "\(source); \(harness.mathRequestDetails)")
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" })
                #expect(!harness.media.requests.contains {
                    $0.path.contains("/input/tex/extensions/") && !$0.path.hasSuffix("/mhchem.js")
                }, "\(harness.mathRequestDetails)")
                try await page.run("document.getElementById('action').click(); await MathJax.typesetPromise();")
                #expect(try await page.string("document.getElementById('action').textContent") == "Ready")
                #expect(try await page.string("document.querySelector('#bad code').textContent") == source)
                try await page.showPresentedAnswer(#"<p>\(\ce{2H2 + O2 -> 2H2O}\)</p>"#)
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: "allowed answer after refusal")
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test func optionalComponentRoutesAreRefusedBeforeFetching() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                for component in ["cancel", "newcommand", "textmacros", "noundefined", "configmacros"] {
                    let path = "input/tex/extensions/" + component + ".js"
                    let refused = try await page.run("""
                        try { await MathJax.config.loader.require(path); return false; }
                        catch (_) { return true; }
                        """, ["path": path])
                    #expect(refused as? Bool == true)
                    #expect(harness.media.requests(for: "/_anki/mathjax/" + path).isEmpty)
                }
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test func coreAMSAndChemistryUseOnlyAllowedPackages() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <p>\(\frac{a^2+b^2}{2}+\sqrt{x}+\text{area}\)</p>
                    <div>\[\require{ams}\begin{align}a&=b+c\\d&=e-f\end{align}\]</div>
                    <div>\[f(x)=\begin{cases}x & x>0\\0 & x=0\end{cases}\]</div>
                    <p>\(\require{mhchem}\ce{2H2 + O2 -> 2H2O}\)</p>
                    <p>\(\pu{9.8 m/s2}\)</p>
                    """#)
                try await MathRenderingProbe.expect(in: page.webView, count: 5, label: "release math policy")
                #expect(try await page.bool("MathJax.startup.document.inputJax[0].options.packages.slice().sort().join(',') === 'ams,autoload,base,mhchem,require'"))
                #expect(try await page.bool("MathJax.config.startup.output === 'svg'"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test func onlyDelimitedMathIsScannedOnMixedFaces() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let literal = #"\begin{aligned}a&=b\end{aligned} \ref{sample} \eqref{sample} $x$ $$y$$"#
                let face = #"<p id="literal">"# + literal + #"</p><p id="inline">\(x+1\)</p><div id="aligned">\[\begin{aligned}a&=b\\c&=d\end{aligned}\]</div>"#
                for answer in [false, true] {
                    if answer { try await page.showPresentedAnswer(face) }
                    else { try await page.showPresentedQuestion(face) }
                    try await page.run("MathJax.typeset();")
                    #expect(try await page.string("document.getElementById('literal').textContent") == literal)
                    #expect(try await page.bool("document.querySelector('#literal mjx-container') === null && document.querySelector('#inline svg path') !== null && document.querySelector('#aligned svg path') !== null"))
                    #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 2 && !document.querySelector('#qa [data-mml-node=merror]')"))
                }
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func dollarsAndOrdinaryCardsDoNotLoadMath() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <p id="m">$x^2$ and $$y$$ cost $5</p>
                    <script>void '\\(literal script\\)';</script>
                    <script type="text/plain">\(literal data\)</script>
                    <style>/* \(literal CSS\) */</style><textarea>\(literal source\)</textarea>
                    <pre>\[literal block\]</pre><code>\(literal code\)</code>
                    <noscript>\(fallback\)</noscript><select><option>\(choice\)</option></select>
                    """#)
                #expect(try await page.string("document.getElementById('m').textContent") == "$x^2$ and $$y$$ cost $5")
                #expect(try await page.bool("document.querySelector('mjx-container') === null"))
                #expect(!harness.media.requests.contains { $0.path.contains("/mathjax/") })
                #expect(harness.remote.requests.isEmpty)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [
            #"<p>Ordinary <strong>nested</strong> study text.</p>"#,
            #"<p>\(unfinished</p>"#, #"<p>\[unfinished</p>"#,
            #"<p>\(mismatched\]</p>"#, #"<p>\(x\\)</p>"#,
            #"<p>\({x\)</p>"#, #"<p>\(x<span>+1</span>\)</p>"#,
            #"<p>\(x</p><p>\)</p>"#, #"<p>\(x</p><p>\[y</p>"#,
            #"<p>\<br>(x\)</p>"#,
            #"<div class='mathjax_ignore'><span class='mathjax_process'>plain</span>\(ignored\)</div>"#,
        ])
        func unmatchedOrSeparateTextGroupsDoNotLoadMath(html: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(html)
                #expect(!harness.media.requests.contains { $0.path.contains("/mathjax/") })
                #expect(try await page.bool("!document.querySelector('#qa mjx-container') && document.getElementById('qa').style.opacity === '1'"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [
            #"<article><section><div><p>\<wbr>(x<!-- join -->+1\<!-- join -->)</p></div></section></article>"#,
            #"<p>\[x<br>+1\]</p>"#, #"<p>\(\{x\)</p>"#, #"<p>\\(y\)</p>"#,
            #"<p>\(\text{a)b}+1\)</p>"#, #"<p>\(unfinished \[z\]</p>"#,
            #"<p>\({unfinished \(x\)</p>"#,
            #"""
            <script>
            onUpdateHook.push(function () {
              var p = document.createElement('p');
              ['\\', '(x', '+1', '\\', ')'].forEach(function (s) { p.appendChild(document.createTextNode(s)); });
              document.getElementById('qa').appendChild(p);
            });
            </script>
            """#,
        ])
        func lazyMathMatchesDelimitersAcrossIncludedNodesAndNestedMarkup(html: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(html)
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: "matched DOM text group")
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [false, true])
        func supersededPublicTypesetPromiseRejectsWithCancellation(documentWide: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.announce(card: 94, generation: 1)
                try await page.showPresentedQuestion(#"<p id="old">\(x+1\)</p>"#)
                try await page.run("""
                    var held = new Promise(function (resolve) { window.__releaseRequest = resolve; });
                    MathJax.whenReady(function () { window.__requestBlocked = true; return held; });
                    """)
                try await harness.until("public queue blocked") { try await page.bool("window.__requestBlocked === true") }
                let targets = documentWide ? "" : "[document.getElementById('old')]"
                try await page.run("""
                    window.__requestResult = MathJax.typesetPromise(\(targets)).then(
                      function () { return 'resolved'; }, function (error) { return error.name; });
                    """)
                try await page.announce(card: 95, generation: 2)
                try await page.showAnswer("<p id='current'>Current card</p>")
                try await harness.until("replacement inserted cleanup barrier") { try await page.bool("document.getElementById('old') === null") }
                try await page.run("__releaseRequest(); window.__requestStatus = await __requestResult;")
                try await harness.signal("shown", count: 2, from: page)
                #expect(try await page.string("__requestStatus") == "AbortError")
                #expect(try await page.string("document.getElementById('current').textContent") == "Current card")
                try await page.run(#"""
                    document.getElementById('current').textContent = '\\(y+1\\)';
                    await MathJax.typesetPromise([document.getElementById('current')]);
                    """#)
                #expect(try await page.bool("document.querySelector('#current svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func texLabelsFollowRemovedSidesAndRetainPageTargets() async throws {
            let extra = #"""
                <aside id="pageMath">
                <p>\[\begin{align}s&=1\label{pageEquation}\end{align}\]</p>
                <p id="pageRef">\(\ref{pageEquation}\)</p></aside><script>
                MathJax.tex.tags = 'ams';
                window.__pageMath = MathJax.typesetPromise([document.getElementById('pageMath')]);
                </script>
                """#
            try await WebTestHarness.withPage(reviewExtra: extra, log: false) { harness, page async throws in
                try await page.run("""
                    await __pageMath;
                    window.__pageItems = MathJax.startup.document.getMathItemsWithin([document.getElementById('pageMath')]);
                    window.__pageSVG = document.querySelector('#pageMath svg');
                    window.__pageTarget = document.getElementById(decodeURIComponent(document.querySelector('#pageRef svg a').getAttribute('href').split('#')[1]));
                    """)
                #expect(try await page.bool("__pageItems.length === 2 && __pageSVG.querySelector('path') && __pageTarget && document.getElementById('pageMath').contains(__pageTarget)"))
                let front = #"<p>\[\begin{align}x&=1\label{shared}\end{align}\]</p>"#
                try await page.announce(card: 101, generation: 1)
                try await page.showPresentedQuestion(front)
                // FrontSide repeats the question verbatim. Keep the exact native
                // identity: release is tied to the removed side, not a new card.
                try await page.showPresentedAnswer(front + #"<p id="questionRef">\(\ref{shared}\)</p><p>\[\begin{align}y&=2\label{answerOnly}\end{align}\]</p><p id="answerRef">\(\ref{answerOnly}\)</p><p id="currentPageRef">\(\ref{pageEquation}\)</p>"#)
                #expect(try await page.string("document.querySelector('#questionRef mjx-assistive-mml math').textContent.trim()") == "2")
                #expect(try await page.string("document.querySelector('#answerRef mjx-assistive-mml math').textContent.trim()") == "3")
                #expect(try await page.string("document.querySelector('#currentPageRef mjx-assistive-mml math').textContent.trim()") == "1")
                #expect(try await page.bool("!document.querySelector('#qa [data-mml-node=merror]')"))
                try await page.announce(card: 102, generation: 2)
                try await page.showPresentedQuestion(#"<p>\[\begin{align}z&=3\label{shared}\end{align}\]</p><p id="newRef">\(\ref{shared}\)</p><p id="expiredRef">\(\ref{answerOnly}\)</p><p id="currentPageRef">\(\ref{pageEquation}\)</p>"#)
                #expect(try await page.string("document.querySelector('#newRef mjx-assistive-mml math').textContent.trim()") == "2")
                #expect(try await page.string("document.querySelector('#expiredRef mjx-assistive-mml math').textContent.trim()") == "???")
                #expect(try await page.string("document.querySelector('#currentPageRef mjx-assistive-mml math').textContent.trim()") == "1")
                #expect(try await page.bool("!document.querySelector('#qa [data-mml-node=merror]') && MathJax.startup.document.getMathItemsWithin([document.getElementById('pageMath')]).length === 2 && MathJax.startup.document.getMathItemsWithin([document.getElementById('pageMath')]).every(function (item, i) { return item === __pageItems[i]; }) && document.querySelector('#pageMath svg') === __pageSVG && __pageSVG.isConnected && __pageTarget.isConnected && document.getElementById(__pageTarget.id) === __pageTarget && document.getElementById(decodeURIComponent(document.querySelector('#currentPageRef svg a').getAttribute('href').split('#')[1])) === __pageTarget"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func declaredOperatorsFollowCardScopeAndRetainPageDefinitions() async throws {
            let extra = #"""
                <aside id="pageOperator">\(\DeclareMathOperator{\pageTrace}{tr}\pageTrace(A)\)</aside><script>
                window.__pageOperator = MathJax.typesetPromise([document.getElementById('pageOperator')]);
                </script>
                """#
            try await WebTestHarness.withPage(reviewExtra: extra, log: false) { harness, page async throws in
                try await page.run("await __pageOperator; window.__operatorItem = MathJax.startup.document.getMathItemsWithin([document.getElementById('pageOperator')])[0]; window.__operatorSVG = document.querySelector('#pageOperator svg');")
                try await page.announce(card: 111, generation: 1)
                try await page.showPresentedQuestion(#"<p id="operator">\(\DeclareMathOperator{\cardRank}{rank}\cardRank(A)\)</p>"#)
                #expect(try await page.string("document.querySelector('#operator mjx-assistive-mml mi').textContent") == "rank")
                try await page.announce(card: 111, generation: 2)
                try await page.showPresentedAnswer(#"<p id="operator">\(\cardRank(B)\)</p><p id="pageUse">\(\pageTrace(B)\)</p>"#)
                #expect(try await page.string("document.querySelector('#operator mjx-assistive-mml mi').textContent") == "rank")
                #expect(try await page.string("document.querySelector('#pageUse mjx-assistive-mml mi').textContent") == "tr")
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
                try await page.announce(card: 112, generation: 3)
                try await page.showPresentedQuestion(#"<p id="expired">\(\cardRank(C)\)</p><p id="pageUse">\(\pageTrace(C)\)</p><p id="stock">\(\sin(x)\)</p>"#)
                // The expired declaration uses the approved source fallback;
                // page declarations and stock operators still render normally.
                #expect(try await page.string("document.querySelector('#expired [data-meguri-math-error] code').textContent") == #"\(\cardRank(C)\)"#)
                #expect(try await page.bool("document.querySelector('#expired [data-meguri-math-error][role=group]').getAttribute('aria-label') === 'Formula could not be rendered' && document.querySelector('#expired strong').textContent === 'Formula could not be rendered: ' && !document.querySelector('#expired mjx-container')"))
                #expect(try await page.string("document.querySelector('#pageUse mjx-assistive-mml mi').textContent") == "tr")
                #expect(try await page.string("document.querySelector('#stock mjx-assistive-mml mi').textContent") == "sin")
                #expect(try await page.bool("document.querySelector('#pageUse svg path') && document.querySelector('#stock svg path') && !document.querySelector('#qa [data-mml-node=merror]') && MathJax.startup.document.getMathItemsWithin([document.getElementById('pageOperator')])[0] === __operatorItem && document.querySelector('#pageOperator svg') === __operatorSVG && __operatorSVG.isConnected"))
                #expect(harness.diagnostics.count == 1, "\(harness.diagnostics.map(\.body))")
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && $0["phase"] as? String == "typeset"
                        && $0.card == 112 && $0.generation == 3 && $0.side == "question"
                        && ($0["message"] as? String)?.contains("Undefined control sequence \\cardRank") == true
                }, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func supersededTypesettingCannotRevealTheReplacement() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                let path = "/_anki/mathjax/font/svg/dynamic/fraktur.js"
                harness.media.hold(path)
                try await page.run(#"""
                    window.__oldUpdate = _updateQA('<p>\\(𝔄\\)</p>', '', function () {}, function () {});
                    """#)
                try await harness.until("glyph component requested") { !harness.media.requests(for: path).isEmpty }
                try await page.run("""
                    window.__newUpdate = _updateQA('<p id="current">Current</p>', '', function () {
                      return new Promise(function (resolve) { window.__releaseNew = resolve; });
                    }, function () {});
                    """)
                try await harness.until("replacement hook entered") { try await page.bool("typeof __releaseNew === 'function'") }
                harness.media.release(path)
                try await page.run("await __oldUpdate;")
                #expect(try await page.bool("document.getElementById('current') !== null && document.getElementById('qa').style.opacity === '0' && !document.querySelector('#qa mjx-container')"))
                #expect(harness.signals("shown", from: page).count == 1)
                try await page.run("__releaseNew(); await __newUpdate;")
                #expect(harness.signals("shown", from: page).count == 2)
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func lazyMathHonorsExplicitProcessClassOnSkippedTags() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <pre class="sample mathjax_process"><code class="mathjax_process" id="m">\(x+1\)</code></pre>
                    <code id="literal">\(y+1\)</code>
                    """#)
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && !document.querySelector('#literal mjx-container')"))
                #expect(harness.media.requests.contains { $0.path == "/_anki/mathjax/tex-svg.js" })
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [false, true])
        func lazyMathMatchesUpstreamIgnoreAndProcessRules(process: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let enabled = process ? #"<code class="mathjax_process mathjax_ignore" id="enabled">\(x+1\)</code>"# : ""
                try await page.showPresentedQuestion(#"""
                    <div class="mathjax_ignore"><span id="ignored">\(a+1\)</span>\#(enabled)</div>
                    <pre id="skippedPre"><span class="mathjax_process">\(skipped\)</span></pre>
                    <code id="tabCode" class="sample&#9;mathjax_process">\(tab-separated\)</code>
                    <div id="handled" data-MJX="true" class="mathjax_process">\(already-handled\)</div>
                    """#)
                #expect(harness.media.requests.contains { $0.path == "/_anki/mathjax/tex-svg.js" } == process)
                for afterStartup in [false, true] {
                    if afterStartup {
                        try await page.run("await MathJax.startup.promise; await MathJax.typesetPromise([document.getElementById('qa')]);")
                    }
                    try await MathRenderingProbe.expect(in: page.webView, count: process ? 1 : 0, label: "process=\(process), afterStartup=\(afterStartup)")
                    if process {
                        #expect(try await page.bool("document.querySelector('#enabled mjx-container svg path') !== null"))
                    }
                    #expect(try await page.string("document.getElementById('ignored').textContent") == #"\(a+1\)"#)
                    #expect(try await page.string("document.getElementById('skippedPre').textContent") == #"\(skipped\)"#)
                    #expect(try await page.string("document.getElementById('tabCode').textContent") == #"\(tab-separated\)"#)
                    #expect(try await page.string("document.getElementById('handled').textContent") == #"\(already-handled\)"#)
                    #expect(try await page.bool("!document.querySelector('#ignored mjx-container, #skippedPre mjx-container, #tabCode mjx-container, #handled mjx-container')"))
                }
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: CardRuntime.Platform.allCases)
        func mathFinishesBeforeShownAndReplacementClearsItems(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform, log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <div id="inline">\(x^2+1\)</div>
                    <div id="ams">\[\begin{aligned}a&=b+c\\d&=e-f\end{aligned}\]</div>
                    <div id="chem">\(\ce{2H2 + O2 -> 2H2O}\)</div>
                    <div id="glyph">\(\mathfrak{M}\)</div>
                    <p id="dollars">$x$ and $$y$$</p><div id="hook"></div>
                    <script>
                    onUpdateHook.push(function () { document.getElementById('hook').textContent = '\\(z^3\\)'; });
                    onShownHook.push(function () {
                      var observed = \#(MathRenderingProbe.snapshot);
                      document.getElementById('hook').dataset.rendered = String(observed.count);
                      document.getElementById('hook').dataset.mathValid = String(observed.valid);
                      document.getElementById('hook').dataset.mathDetails = JSON.stringify(observed);
                    });
                    </script>
                    """#)
                let shownDetails = try await page.string("document.getElementById('hook').dataset.mathDetails")
                #expect(try await page.string("document.getElementById('hook').dataset.rendered") == "5", "onShownHook: \(shownDetails)")
                #expect(try await page.string("document.getElementById('hook').dataset.mathValid") == "true", "onShownHook: \(shownDetails)")
                #expect(try await page.bool("document.querySelector('#ams mjx-container[display=\"true\"]') !== null"))
                #expect(try await page.bool("document.querySelector('#chem svg path') !== null && document.querySelector('#glyph svg path') !== null"))
                #expect(try await page.bool("document.querySelector('#qa [data-mml-node=merror]') === null"))
                #expect(try await page.string("document.getElementById('dollars').textContent") == "$x$ and $$y$$")
                #expect(harness.media.requests.contains { $0.path.hasPrefix("/_anki/mathjax/font/svg/dynamic/") })
                #expect(harness.media.requests.contains { $0.path == "/_anki/mathjax/input/tex/extensions/mhchem.js" })
                #expect(harness.media.requests.contains { $0.path == "/_anki/mathjax/fonts/mathjax-mhchem-font-extension/svg.js" })
                #expect(try await page.bool("performance.getEntriesByType('resource').every(function (entry) { return entry.name.startsWith('meguri-media://media/'); })"))
                #expect(harness.remote.requests.isEmpty)
                #expect(!harness.media.requests.contains { $0.path.contains("/sre/") || $0.path.contains("/speech") || $0.path.contains("/explorer") })
                try await page.showPresentedAnswer(#"<p id="answer">\[\int_0^1 x^2\,dx=\frac{1}{3}\]</p>"#)
                try await MathRenderingProbe.expect(in: page.webView, count: 1, label: "replacement answer", display: true)
                #expect(try await page.bool("document.querySelector('#answer mjx-container svg path') !== null && !document.getElementById('inline')"))
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"))
                try await page.showPresentedQuestion("<p>Next ordinary card</p>")
                #expect(try await page.bool("document.querySelector('#qa mjx-container') === null && Array.from(MathJax.startup.document.math).length === 0"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: ["alignat", "alignat*", "alignedat", "alignedat[t]", "xalignat", "xalignat*", "xxalignat"])
        func amsAlignmentPairsAreBoundedBeforeAllocation(variant: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let name = variant == "alignedat[t]" ? "alignedat" : variant
                let placement = variant == "alignedat[t]" ? "[t]" : ""
                func side(pairs: Int) -> String {
                    let count = String(pairs)
                    let environment = "\\begin{\(name)}\(placement){\(count)}a&=b&c&=d\\end{\(name)}"
                    let tex = environment
                    return "<div id='m'>\\[\(tex)\\]</div>"
                }
                // Ordinary two-pair inputs retain their normal rendering.
                try await page.showPresentedQuestion(side(pairs: 2))
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && !document.querySelector('#m [data-mml-node=merror]')"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")

                // A modest declaration just beyond the cap, with no large body.
                try await page.announce(card: 83, generation: 2)
                try await page.showPresentedAnswer(side(pairs: 65))
                #expect(try await page.bool("document.querySelector('#m [data-meguri-math-error]') !== null && document.getElementById('qa').style.opacity === '1'"))
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && $0.card == 83 && $0.generation == 2 && $0.side == "answer"
                        && ($0["message"] as? String)?.contains("MeguriAlignmentLimit") == true
                }, "\(harness.diagnostics.map(\.body))")

                try await page.announce(card: 84, generation: 3)
                try await page.showPresentedQuestion(side(pairs: 2))
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && !document.querySelector('#m [data-mml-node=merror]')"))
                #expect(!harness.diagnostics.contains { $0.card == 84 })
                #expect(harness.remote.requests.isEmpty)
            }
        }

        @Test func mathIgnoresSavedMenuSettingsWithoutChangingThem() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let settings = #"{"enrich":true,"speech":true,"braille":true,"assistiveMml":false,"inTabOrder":true,"renderer":"CHTML"}"#
                try await page.run("localStorage.setItem('MathJax-Menu-Settings', settings);", ["settings": settings])
                try await page.showPresentedQuestion(#"<p id="m">\(x+1\)</p><textarea id="source">\(x+1\)</textarea>"#)
                #expect(try await page.string("localStorage.getItem('MathJax-Menu-Settings')") == settings)
                #expect(try await page.bool("MathJax.config.startup.typeset === false && MathJax.startup.document.options.enableMenu === false"))
                #expect(try await page.bool("(function (s) { return !s.enrich && !s.speech && !s.braille && s.assistiveMml && !s.inTabOrder && s.renderer === 'SVG'; })(MathJax.startup.document.menu.settings)"))
                #expect(try await page.bool("document.querySelector('#m mjx-container svg path') !== null && document.querySelector('#m mjx-assistive-mml math') !== null"))
                #expect(try await page.string("document.getElementById('source').value") == #"\(x+1\)"#)
                #expect(try await page.bool("performance.getEntriesByType('resource').every(function (entry) { return entry.name.startsWith('meguri-media://media/'); })"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: ["entry-timeout", "entry-exception", "extension-timeout"])
        func missingBundledMathStillRevealsAndAcceptsTheNextSide(failure: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                switch failure {
                case "entry-exception":
                    harness.media.add("/_anki/mathjax/tex-svg.js", text: "throw new Error('Synthetic MathJax evaluation failure');")
                case "extension-timeout":
                    harness.media.hold("/_anki/mathjax/a11y/assistive-mml.js")
                default:
                    harness.media.hold("/_anki/mathjax/tex-svg.js")
                }
                try await page.showPresentedQuestion(#"""
                    <p id="m">\(x\)</p><script>
                    onUpdateHook.push(async function () {
                      var results = await Promise.allSettled([MathJax.startup.promise, MathJax.typesetPromise()]);
                      window.__mathRejected = results.every(function (result) { return result.status === 'rejected'; });
                    });
                    </script>
                    """#)
                #expect(try await page.string("document.getElementById('qa').style.opacity") == "1")
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" })
                if failure == "entry-exception" {
                    #expect(harness.diagnostics.contains {
                        ($0["message"] as? String)?.contains("Synthetic MathJax evaluation failure") == true
                            && ($0["source"] as? String)?.hasSuffix("/mathjax/tex-svg.js") == true
                    })
                }
                #expect(try await page.bool("typeof MathJax.typeset === 'function' && typeof MathJax.typesetPromise === 'function' && typeof MathJax.typesetClear === 'function'"))
                #expect(try await page.bool("window.__mathRejected === true"))
                #expect(try await page.bool("require('anki/reviewer').onUpdateHook === onUpdateHook"))
                try await page.showPresentedAnswer("<p>Still reviewable</p>")
                #expect(try await page.string("document.getElementById('qa').textContent") == "Still reviewable")
            }
        }

        /// Release the actual held URL task only after the deadline and page
        /// reuse. Startup and in-render glyph dependencies cover both sides
        /// of the engine's readiness boundary.
        @Test(arguments: ["entry", "startup-extension", "glyph"])
        func lateMathDeliveryCannotTouchTheReusedSide(load: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let path: String
                let math: String
                switch load {
                case "startup-extension":
                    path = "/_anki/mathjax/a11y/assistive-mml.js"
                    math = #"\(x\)"#
                case "glyph":
                    path = "/_anki/mathjax/font/svg/dynamic/fraktur.js"
                    // U+1D504 (120068) is explicit in this chunk's normal table.
                    math = #"\(𝔄\)"#
                default:
                    path = "/_anki/mathjax/tex-svg.js"
                    math = #"\(x\)"#
                }
                harness.media.hold(path)
                try await page.announce(card: 71, generation: 1)
                try await page.showPresentedQuestion("""
                    <p id="m">\(math)</p><script>
                    window.__mathAPI = MathJax;
                    window.__cardRequire = require;
                    window.__stableQA = document.getElementById('qa');
                    window.__rootEvents = 0;
                    __stableQA.addEventListener('card-root-check', function () { __rootEvents++; });
                    // Includes a retained facade and a second queued request.
                    onUpdateHook.push(async function () {
                      __mathAPI.typeset([document.getElementById('m')]);
                      await Promise.allSettled([
                        __mathAPI.typesetPromise(), __mathAPI.typesetPromise([document.getElementById('m')])
                      ]);
                    });
                    </script>
                    """)
                #expect(harness.media.requests(for: path).count == 1, "\(harness.mathRequestDetails)")
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" && $0.card == 71 && $0.generation == 1 }, "\(harness.mathRequestDetails)")
                try await page.announce(card: 72, generation: 2)
                try await page.showPresentedAnswer("<p id='current'>Still reviewable</p>")
                let diagnostics = harness.diagnostics.count
                harness.media.release(path)
                try await page.run("await MathJax.startup.promise; await MathJax.whenReady(function () {});")
                #expect(try await page.string("document.getElementById('qa').textContent") == "Still reviewable", "\(harness.mathRequestDetails)")
                #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && !document.querySelector('#qa mjx-container')"), "\(harness.mathRequestDetails)")
                try await page.run("document.getElementById('qa').dispatchEvent(new Event('card-root-check'));")
                #expect(try await page.bool("document.getElementById('qa') === __stableQA && __rootEvents === 1"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 0"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("require === __cardRequire && require('anki/reviewer').onUpdateHook === onUpdateHook"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("typeof MathJax.typeset === 'function' && typeof MathJax.typesetPromise === 'function' && typeof MathJax.typesetClear === 'function'"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("MathJax.config.startup.typeset === false && MathJax.config.options.enableMenu === false"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("await MathJax.config.loader.require('https://unbundled.invalid/component.js').then(function () { return false; }, function () { return true; })"), "\(harness.mathRequestDetails)")
                #expect(harness.diagnostics.count == diagnostics, "\(harness.diagnostics.map(\.body)); \(harness.mathRequestDetails)")
                #expect(harness.remote.requests.isEmpty, "\(harness.mathRequestDetails)")
                #expect(try await page.bool("performance.getEntriesByType('resource').every(function (entry) { return entry.name.startsWith('meguri-media://media/'); })"), "\(harness.mathRequestDetails)")
                // Startup can recover, and the retained template API still works.
                try await page.announce(card: 73, generation: 3)
                try await page.showPresentedQuestion(#"""
                    <p id="next">\(z+1\)</p><script>
                    onUpdateHook.push(function () { return __mathAPI.typesetPromise([document.getElementById('next')]); });
                    </script>
                    """#)
                #expect(try await page.bool("document.querySelector('#next mjx-container svg path') !== null"), "\(harness.mathRequestDetails)")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"), "\(harness.mathRequestDetails)")
                try await page.run("document.getElementById('qa').dispatchEvent(new Event('card-root-check'));")
                #expect(try await page.bool("document.getElementById('qa') === __stableQA && __rootEvents === 2"), "\(harness.mathRequestDetails)")
                #expect(harness.diagnostics.count == diagnostics, "\(harness.mathRequestDetails)")
            }
        }

        @Test(arguments: ["tex-svg.js", "font/svg/dynamic/fraktur.js"], [false, true])
        func lateMathStillRendersTheCurrentSide(component: String, startupHook: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let path = "/_anki/mathjax/" + component
                harness.media.hold(path)
                try await page.announce(card: 91, generation: 1)
                let hook = startupHook ? "<script>onUpdateHook.push(function () { return MathJax.startup.promise; });</script>" : ""
                try await page.showPresentedQuestion(#"<p id="m">\(𝔄\)</p>"# + hook)
                #expect(harness.media.requests(for: path).count == 1, "\(harness.mathRequestDetails)")
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" && $0.card == 91 }, "\(harness.mathRequestDetails)")
                #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && !document.querySelector('#m svg path')"), "\(harness.mathRequestDetails)")
                harness.media.release(path)
                try await page.run("await MathJax.startup.promise; await MathJax.whenReady(function () {});")
                #expect(try await page.bool("document.querySelector('#m svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"), "\(harness.mathRequestDetails)")
                try await page.showPresentedAnswer(#"<p id="next">\(z+1\)</p>"#)
                #expect(try await page.bool("document.querySelector('#next svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"), "\(harness.mathRequestDetails)")
                #expect(harness.remote.requests.isEmpty, "\(harness.mathRequestDetails)")
            }
        }

        @Test func mathReplacementPreservesCustomElementCallbacks() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"""
                    <study-note id="outgoing"></study-note><p>\(x+1\)</p>
                    <script>
                    window.__cardCallbacks = { connected: 0, disconnected: 0 };
                    window.__replacementOrder = [];
                    customElements.define('study-note', class extends HTMLElement {
                      connectedCallback() { __cardCallbacks.connected++; }
                      disconnectedCallback() {
                        __cardCallbacks.disconnected++;
                        __replacementOrder.push('disconnect');
                        var replacement = document.getElementById('replacement');
                        __cardCallbacks.text = replacement && replacement.textContent;
                        __cardCallbacks.next = replacement && replacement.nextSibling.id;
                        __cardCallbacks.detachedSibling = this.nextSibling === null;
                        if (replacement) replacement.appendChild(this);
                      }
                    });
                    </script>
                    """#)
                #expect(try await page.bool("MathJax.version === '4.1.3' && __cardCallbacks.connected === 1"))
                try await page.showPresentedAnswer(#"""
                    <p id="replacement">Replacement</p><p id="following">\(y+1\)</p>
                    <script>__replacementOrder.push('script');</script>
                    """#)
                #expect(try await page.string("__cardCallbacks.text") == "Replacement")
                #expect(try await page.string("__cardCallbacks.next") == "following")
                #expect(try await page.bool("__cardCallbacks.detachedSibling && __cardCallbacks.connected === 2 && __cardCallbacks.disconnected === 1"))
                #expect(try await page.bool("document.getElementById('outgoing').parentNode === document.getElementById('replacement')"))
                #expect(try await page.string("__replacementOrder.join(',')") == "disconnect,script")
                #expect(try await page.bool("document.querySelector('#following svg path') !== null && Array.from(MathJax.startup.document.math).length === 1"))
                try await page.showPresentedQuestion("<p>Next ordinary card</p>")
                #expect(try await page.bool("__cardCallbacks.connected === 2 && __cardCallbacks.disconnected === 2 && Array.from(MathJax.startup.document.math).length === 0"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func mathReplacementPreservesAuthoredMarkupAndFallbackSource() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                try await page.announce(card: 123, generation: 2)
                try await page.showPresentedAnswer(#"""
                    <p id="formatted" style="color: rgb(1, 2, 3)">Study <em data-note="kept">A &amp; B</em></p>
                    <code id="literal">\\(literal\\)</code><p id="bad">\(\frac{1}\)</p>
                    """#)
                #expect(try await page.string("document.getElementById('formatted').innerHTML") == #"Study <em data-note="kept">A &amp; B</em>"#)
                #expect(try await page.string("getComputedStyle(document.getElementById('formatted')).color") == "rgb(1, 2, 3)")
                #expect(try await page.string("document.getElementById('literal').textContent") == #"\\(literal\\)"#)
                #expect(try await page.string("document.querySelector('#bad [data-meguri-math-error] code').textContent") == #"\(\frac{1}\)"#)
                #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && !document.querySelector('#literal mjx-container')"))
                #expect(harness.diagnostics.count == 1, "\(harness.diagnostics.map(\.body))")
                #expect(harness.diagnostics.allSatisfy {
                    $0["kind"] as? String == "math" && $0.card == 123 && $0.generation == 2 && $0.side == "answer"
                })
            }
        }

        @Test func plainFollowingCardReleasesTheStaleMathScanTarget() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p id="old">\(x+1\)</p>"#)
                try await page.run("window.__oldMathTarget = document.getElementById('old');")
                try await page.showPresentedAnswer("<p>Plain following card</p>")
                #expect(try await page.bool("MathJax.startup.document.options.elements === null && Array.from(MathJax.startup.document.math).length === 0 && !__oldMathTarget.isConnected"))
                #expect(try await page.string("document.getElementById('qa').textContent") == "Plain following card")
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test(arguments: [false, true])
        func staleMathCleanupPreservesExistingSiblingItems(documentWide: Bool) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                try await page.run(#"""
                    var sibling = document.createElement('aside');
                    sibling.id = 'sibling'; sibling.textContent = '\\(s+1\\)';
                    document.body.appendChild(sibling);
                    await MathJax.typesetPromise([sibling]);
                    window.__siblingItem = MathJax.startup.document.getMathItemsWithin([sibling])[0];
                    window.__siblingSVG = sibling.querySelector('svg');
                    """#)
                let path = "/_anki/mathjax/font/svg/dynamic/fraktur.js"
                harness.media.hold(path)
                let targets = documentWide ? "" : "[document.getElementById('qa')]"
                try await page.showPresentedAnswer("""
                    <p id='stale'>\\(𝔄\\)</p><script>
                    var added = document.createElement('aside');
                    added.id = 'templateSibling'; added.textContent = '\\\\(t+1\\\\)';
                    document.body.appendChild(added);
                    onUpdateHook.push(function () { return MathJax.typesetPromise(\(targets)); });
                    </script>
                    """)
                #expect(harness.media.requests(for: path).count == 1, "\(harness.mathRequestDetails)")
                try await page.run("window.__staleTarget = document.getElementById('stale');")
                try await page.showPresentedQuestion("""
                    <p id='current'>Current card</p>
                    <style>#qa > p { color: rgb(1, 2, 3); }</style>
                    """)
                // Cover both our unchanged target and an authored replacement
                // while the old render still holds the cleanup queue.
                if documentWide {
                    try await page.run("window.__authoredScanTarget = [document.getElementById('sibling')]; MathJax.startup.document.options.elements = __authoredScanTarget;")
                }
                harness.media.release(path)
                try await page.run("await MathJax.whenReady(function () {});")
                let scanTarget = documentWide ? "__authoredScanTarget" : "null"
                #expect(try await page.bool("MathJax.startup.document.options.elements === \(scanTarget)"), "\(harness.mathRequestDetails)")
                #expect(try await page.string("document.getElementById('current').textContent") == "Current card", "\(harness.mathRequestDetails)")
                #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && !document.querySelector('#qa mjx-container') && getComputedStyle(document.getElementById('current')).color === 'rgb(1, 2, 3)'"), "\(harness.mathRequestDetails)")
                let retainedCount = documentWide ? 2 : 1
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === \(retainedCount) && MathJax.startup.document.getMathItemsWithin([document.getElementById('sibling')])[0] === __siblingItem && document.querySelector('#sibling svg') === __siblingSVG && __siblingSVG.isConnected && !__staleTarget.isConnected && MathJax.startup.document.getMathItemsWithin([__staleTarget]).length === 0"), "\(harness.mathRequestDetails)")
                let templateRendered = documentWide ? "true" : "false"
                #expect(try await page.bool("(document.querySelector('#templateSibling svg path') !== null) === \(templateRendered) && MathJax.startup.document.getMathItemsWithin([document.getElementById('templateSibling')]).length === \(retainedCount - 1)"), "\(harness.mathRequestDetails)")
                // A later document render still retains the exact sibling item.
                try await page.showPresentedAnswer(#"<p id="next">\(n+1\)</p>"#)
                try await page.run("await MathJax.typesetPromise();")
                #expect(try await page.bool("document.querySelector('#next svg path') !== null && Array.from(MathJax.startup.document.math).length === 3 && Array.from(MathJax.startup.document.math).includes(__siblingItem) && document.querySelector('#sibling svg') === __siblingSVG && document.querySelector('#templateSibling svg path') !== null && MathJax.startup.document.getMathItemsWithin([document.getElementById('templateSibling')]).length === 1"), "\(harness.mathRequestDetails)")
                #expect(harness.remote.requests.isEmpty, "\(harness.mathRequestDetails)")
            }
        }

        @Test func lateMathEntrypointEvaluationErrorsAreNotAttributedToTheNewSide() async throws {
            try await checkLateMathEvaluationError(component: "tex-svg.js")
        }

        @Test func lateMathAssistiveMMLEvaluationErrorsAreNotAttributedToTheNewSide() async throws {
            try await checkLateMathEvaluationError(component: "a11y/assistive-mml.js")
        }

        private func checkLateMathEvaluationError(component: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                var stage = "announce held math question"
                do {
                    let path = "/_anki/mathjax/" + component
                    harness.media.hold(path)
                    try await page.announce(card: 74, generation: 1)
                    stage = "present held math question and await shown"
                    try await page.showPresentedQuestion(#"<p>\(x\)</p>"#)
                    #expect(harness.media.requests(for: path).count == 1)
                    #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" && $0.card == 74 })
                    stage = "announce ordinary answer"
                    try await page.announce(card: 75, generation: 2)
                    stage = "show ordinary answer and await shown while component is held"
                    try await page.showPresentedAnswer("<p>Current ordinary side</p>")
                    let diagnostics = harness.diagnostics.count
                    harness.media.add(path, text: "window.__lateEvaluated = true; throw new Error('Synthetic late component exception');")
                    stage = "release component and observe late execution"
                    harness.media.release(path)
                    try await harness.until("late component execution") { try await page.bool("window.__lateEvaluated === true") }
                    stage = "verify ordinary answer and retained APIs after late exception"
                    #expect(try await page.string("document.getElementById('qa').textContent") == "Current ordinary side")
                    #expect(try await page.bool("document.getElementById('qa').style.opacity === '1' && typeof MathJax.typesetPromise === 'function'"))
                    #expect(try await page.bool("require('anki/reviewer').onUpdateHook === onUpdateHook"))
                    #expect(harness.diagnostics.count == diagnostics, "\(harness.diagnostics.map(\.body))")
                    #expect(harness.remote.requests.isEmpty)
                } catch {
                    throw HarnessFailure("\(component), stage: \(stage): \(error); \(harness.mathRequestDetails); diagnostics: \(harness.diagnostics.map(\.body))")
                }
            }
        }

        @Test(arguments: ["malformed", "glyph"])
        func resolvedMathErrorsReportTheirOriginAndKeepReviewWorking(failure: String) async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p>\(x\)</p>"#)
                #expect(harness.diagnostics.isEmpty, "\(harness.mathRequestDetails)")
                let math: String
                let source: String
                switch failure {
                case "glyph":
                    math = #"\(𝔄\)"#
                    source = "font/svg/dynamic/fraktur.js"
                    harness.media.respondNotFound("/_anki/mathjax/" + source)
                default:
                    math = #"\(\frac{1}\)"#
                    source = "tex-svg.js"
                }
                try await page.announce(card: 81, generation: 9)
                try await page.showPresentedAnswer("<p id='bad'>\(math)</p>")
                #expect(try await page.string("document.getElementById('qa').style.opacity") == "1", "\(harness.mathRequestDetails)")
                #expect(harness.diagnostics.contains {
                    $0["kind"] as? String == "math" && $0.card == 81 && $0.generation == 9 && $0.side == "answer"
                        && ($0["source"] as? String)?.hasSuffix(source) == true
                }, "\(harness.diagnostics.map(\.body)); \(harness.mathRequestDetails)")
                if failure == "malformed" {
                    #expect(try await page.bool("document.querySelector('#bad [data-meguri-math-error]') !== null"), "\(harness.mathRequestDetails)")
                    #expect(harness.diagnostics.contains { ($0["message"] as? String)?.contains("Missing") == true }, "\(harness.mathRequestDetails)")
                    #expect(!harness.diagnostics.contains { ($0["message"] as? String)?.contains("[object Object]") == true }, "\(harness.mathRequestDetails)")
                } else {
                    #expect(harness.media.requests.contains { $0.path == "/_anki/mathjax/" + source }, "\(harness.mathRequestDetails)")
                }
                #expect(harness.signals("mediaError", from: page).isEmpty, "Loader failures belong to math diagnostics")
                try await page.announce(card: 82, generation: 10)
                try await page.showPresentedQuestion("<p>Next ordinary card</p>")
                #expect(try await page.string("document.getElementById('qa').textContent") == "Next ordinary card", "\(harness.mathRequestDetails)")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 0"), "\(harness.mathRequestDetails)")
                #expect(!harness.diagnostics.contains { $0.card == 82 }, "\(harness.mathRequestDetails)")
            }
        }

        @Test func loaderResourceErrorsExcludeOnlyRuntimeOwnedScripts() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.announce(card: 121, generation: 1)
                try await page.showPresentedQuestion(#"<p>\(x+1\)</p>"#)
                let path = "/_anki/mathjax/font/svg/dynamic/fraktur.js"
                harness.media.hold(path)
                harness.media.respondNotFound(path)
                try await page.announce(card: 121, generation: 2)
                try await page.showAnswer(#"<p>\(𝔄\)</p>"#)
                try await harness.until("loader requested held component") { !harness.media.requests(for: path).isEmpty }
                #expect(try await page.bool("document.querySelector('script[src$=\"/font/svg/dynamic/fraktur.js\"]').hasAttribute('data-meguri-preload')"))
                try await page.announce(card: 122, generation: 3)
                try await page.showPresentedQuestion("<p id='current'>Replacement card</p>")
                harness.media.release(path)
                try await page.run("await MathJax.whenReady(function () {});")
                #expect(harness.signals("mediaError", from: page).isEmpty)
                #expect(!harness.diagnostics.contains { $0.card == 122 })
                // An authored script at the very same URL is still media.
                try await page.run("""
                    window.__authoredDone = new Promise(function (resolve) {
                      var script = document.createElement('script');
                      script.onerror = resolve; script.src = '\(path)';
                      document.getElementById('qa').appendChild(script);
                    });
                    var image = document.createElement('img');
                    image.src = '/missing-math-fixture.png'; document.getElementById('qa').appendChild(image);
                    await __authoredDone;
                    """)
                try await harness.until("authored script and image errors") { harness.signals("mediaError", from: page).count == 2 }
                let media = harness.signals("mediaError", from: page)
                #expect(media.allSatisfy { $0.card == 122 && $0.generation == 3 && $0.side == "question" })
                #expect(media.contains { $0["tag"] as? String == "script" && ($0["url"] as? String)?.hasSuffix(path) == true })
                #expect(media.contains { $0["tag"] as? String == "img" && ($0["url"] as? String)?.hasSuffix("/missing-math-fixture.png") == true })
                #expect(try await page.string("document.getElementById('current').textContent") == "Replacement card")
                #expect(!harness.diagnostics.contains { $0.card == 122 })
            }
        }

        // MARK: GS11, R514

        @Test func jQueryIsTheUpstreamRelease() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                #expect(try await page.string("$.fn.jquery") == "3.7.1")
                #expect(try await page.bool("$ === jQuery"))
            }
        }

        // MARK: GS12, R512

        @Test func mutateNextCardStatesResolvesWithoutCallingTheTransform() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                let result = try await page.run(
                    """
                    var called = false;
                    var value = await anki.mutateNextCardStates("k", function () { called = true; });
                    return [String(value), String(called), Object.keys(anki).join("|")].join(",");
                    """)
                #expect(result as? String == "undefined,false,mutateNextCardStates|imageOcclusion|setupImageCloze")
            }
        }

        // MARK: GS13, AE18 page part

        @Test(arguments: [CardRuntime.Platform.mac, .iPhone])
        func ankingStyleFrontRevealsOnTheMacOnly(platform: CardRuntime.Platform) async throws {
            try await WebTestHarness.withPage(platform: platform) { harness, page async throws in
                let front = """
                    <p>front</p>
                    <script>
                    if (typeof pycmd !== "undefined") {
                      document.getElementById("qa").style.display = "none";
                      pycmd("ans");
                    }
                    </script>
                    """
                try await page.showPresentedQuestion(front)
                if platform == .mac {
                    let command = try await harness.signal("command", from: page)
                    #expect(command["command"] as? String == "ans")
                } else {
                    try await Task.sleep(for: .milliseconds(200))
                    #expect(harness.signals("command").isEmpty)
                    #expect(try await page.string(#"getComputedStyle(document.getElementById("qa")).display"#) == "block")
                    #expect(try await page.string(#"document.getElementById("qa").textContent.trim()"#).hasPrefix("front"))
                }
                #expect(harness.diagnostics.isEmpty)
            }
        }

        // MARK: GS14, D16

        @Test func documentHasNoTitle() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                #expect(try await page.string("document.title") == "")
                #expect(try await page.string("document.documentElement.hasAttribute('lang')") == "false")
            }
        }

        // MARK: GS15, S15, R511

        @Test func ankiMobileBridgeExistsOnIOSOnly() async throws {
            try await WebTestHarness.withPage(platform: .iPhone, log: false) { harness, page async throws in
                try await page.showPresentedQuestion(try RuntimeFixture.text("s15-ankimobile-front.html"))
                #expect(try await page.string("typeof webkit.messageHandlers.cb.postMessage") == "function")
                #expect(try await page.string("typeof AnkiDroidJS") == "undefined")
                try await page.run(#"document.getElementById("tap").click();"#)
                try await harness.until("the cb message") { !harness.ankiMobile.received.isEmpty }
                #expect(harness.ankiMobile.received.first?["value"] as? String == #"{"scheme":"ankitap","msg":"midCenter"}"#)
                // No other handler Meguri registers is visible here.
                #expect(try await page.string("typeof webkit.messageHandlers.meguriCard") == "undefined")
                #expect(try await page.string("typeof webkit.messageHandlers.meguriVisible") == "undefined")
            }
            try await WebTestHarness.withPage(platform: .mac, log: false) { harness, page async throws in
                #expect(try await page.string("typeof webkit") == "undefined")
                #expect(try await page.string("typeof AnkiDroidJS") == "undefined")
            }
        }

        // MARK: C10.1, C10.2, R478

        @Test func flagAndMarkAreDrawnInTheirFixedElements() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                struct State: Decodable, Equatable {
                    let flagHidden: Bool
                    let flagColor: String
                    let markHidden: Bool
                    let flagText: String
                    let markText: String
                }
                let state = """
                    var f = document.getElementById("_flag"), m = document.getElementById("_mark");
                    return JSON.stringify({ flagHidden: f.hidden, flagColor: f.style.color, markHidden: m.hidden, flagText: f.textContent, markText: m.textContent });
                    """
                #expect(try await page.json(State.self, state) == State(flagHidden: true, flagColor: "", markHidden: true, flagText: "⚑", markText: "★"))
                try await page.run("_drawFlag(3); _drawMark(true);")
                #expect(try await page.json(State.self, state) == State(flagHidden: false, flagColor: "var(--flag-3)", markHidden: false, flagText: "⚑", markText: "★"))
                #expect(try await page.string(#"getComputedStyle(document.getElementById("_flag")).color"#) == "rgb(74, 222, 128)")
                try await page.run("_drawFlag(0); _drawMark(0);")
                #expect(try await page.json(State.self, state) == State(flagHidden: true, flagColor: "var(--flag-0)", markHidden: true, flagText: "⚑", markText: "★"))
                #expect(try await page.string("String(_drawFlag(1)) + String(_drawMark(1))") == "undefinedundefined")
            }
        }
    }
}
