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
                #expect(try await page.string("document.getElementById('m').dataset.requireIntact") == "true")
                #expect(try await page.string("Array.from(MathJax.startup.document.math)[0].math") == "x^2")
                #expect(try await page.bool("document.querySelector('#m mjx-assistive-mml math') !== null"))
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"))
                try await page.run("await MathJax.startup.promise; MathJax.typesetClear([document.getElementById('m')]);")
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 0"))
                for path in ["https://unbundled.invalid/component.js", "meguri-media://other/_anki/mathjax/tex-svg.js",
                             "../runtime.js", "%2e%2e/runtime.js", "tex-svg.js?remote=1", "tex-svg.js#fragment",
                             "meguri-media://user:password@media/_anki/mathjax/tex-svg.js"] {
                    let refused = try await page.run("try { await MathJax.config.loader.require(path); return false; } catch (_) { return true; }", ["path": path])
                    #expect(refused as? Bool == true, "Loader must refuse \(path)")
                }
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
            }
        }

        @Test func dollarsAndOrdinaryCardsDoNotLoadMath() async throws {
            try await WebTestHarness.withPage(log: false) { harness, page async throws in
                try await page.showPresentedQuestion(#"<p id="m">$x^2$ and $$y$$ cost $5</p>"#)
                #expect(try await page.string("document.getElementById('m').textContent") == "$x^2$ and $$y$$ cost $5")
                #expect(try await page.bool("document.querySelector('mjx-container') === null"))
                #expect(!harness.media.requests.contains { $0.path.contains("/mathjax/") })
                #expect(harness.remote.requests.isEmpty)
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
                      document.getElementById('hook').dataset.rendered = String(document.querySelectorAll('#qa mjx-container svg').length);
                    });
                    </script>
                    """#)
                #expect(try await page.string("document.getElementById('hook').dataset.rendered") == "5")
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
                #expect(try await page.bool("document.querySelector('#answer mjx-container svg path') !== null && !document.getElementById('inline')"))
                #expect(try await page.bool("Array.from(MathJax.startup.document.math).length === 1"))
                try await page.showPresentedQuestion("<p>Next ordinary card</p>")
                #expect(try await page.bool("document.querySelector('#qa mjx-container') === null && Array.from(MathJax.startup.document.math).length === 0"))
                #expect(harness.diagnostics.isEmpty, "\(harness.diagnostics.map(\.body))")
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
                try await page.showPresentedQuestion(#"<p id="m">\(x\)</p>"#)
                #expect(try await page.string("document.getElementById('qa').style.opacity") == "1")
                #expect(harness.diagnostics.contains { $0["kind"] as? String == "math" })
                #expect(try await page.bool("typeof MathJax.typeset === 'function' && typeof MathJax.typesetPromise === 'function' && typeof MathJax.typesetClear === 'function'"))
                #expect(try await page.bool("await MathJax.startup.promise.then(function () { return false; }, function () { return true; })"))
                #expect(try await page.bool("await MathJax.typesetPromise().then(function () { return false; }, function () { return true; })"))
                #expect(try await page.bool("require('anki/reviewer').onUpdateHook === onUpdateHook"))
                try await page.showPresentedAnswer("<p>Still reviewable</p>")
                #expect(try await page.string("document.getElementById('qa').textContent") == "Still reviewable")
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
