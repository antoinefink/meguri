import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// The base stylesheet (specs/render/card-runtime.md C4, C16.4;
    /// R479–R487). Expected values are the spec's tables.
    @Suite struct BaseCSSTests {
        /// C4.1: light and dark values, as text.
        static let colourTokens: [(name: String, light: String, dark: String)] = [
            ("--fg", "#020202", "#fcfcfc"),
            ("--fg-subtle", "#737373", "#858585"),
            ("--fg-disabled", "#858585", "#737373"),
            ("--fg-faint", "#afafaf", "#545454"),
            ("--fg-link", "#1d4ed8", "#bfdbfe"),
            ("--canvas", "#f5f5f5", "#2c2c2c"),
            ("--canvas-elevated", "white", "#363636"),
            ("--canvas-inset", "white", "#2c2c2c"),
            ("--canvas-overlay", "#fcfcfc", "#2c2c2c"),
            ("--canvas-code", "white", "#252525"),
            ("--canvas-glass", "rgba(255, 255, 255, 0.4)", "rgba(54, 54, 54, 0.4)"),
            ("--border", "#c4c4c4", "#202020"),
            ("--border-subtle", "#e4e4e4", "#252525"),
            ("--border-strong", "#858585", "#020202"),
            ("--border-focus", "#3b82f6", "#3b82f6"),
            ("--button-bg", "#fcfcfc", "rgb(25.1176470588%, 25.1176470588%, 25.1176470588%)"),
            ("--button-gradient-start", "white", "rgb(29.0588235294%, 29.0588235294%, 29.0588235294%)"),
            ("--button-gradient-end", "#fcfcfc", "rgb(25.1176470588%, 25.1176470588%, 25.1176470588%)"),
            ("--button-hover-border", "#999999", "#141414"),
            ("--button-disabled", "rgba(214, 214, 214, 0.5)", "rgba(69, 69, 69, 0.5)"),
            (
                "--button-primary-bg", "rgb(18.7843137255%, 41.8823529412%, 92.5490196078%)",
                "rgb(15.0392156863%, 32.3333333333%, 81.0392156863%)"
            ),
            ("--button-primary-gradient-start", "#3b82f6", "rgb(18.3921568627%, 40.2745098039%, 88.2745098039%)"),
            (
                "--button-primary-gradient-end", "rgb(18.7843137255%, 41.8823529412%, 92.5490196078%)",
                "rgb(15.0392156863%, 32.3333333333%, 81.0392156863%)"
            ),
            ("--button-primary-disabled", "#93c5fd", "rgb(26.8039215686%, 51.862745098%, 92.8039215686%)"),
            ("--scrollbar-bg", "#d6d6d6", "#454545"),
            ("--scrollbar-bg-hover", "#c4c4c4", "#545454"),
            ("--scrollbar-bg-active", "#afafaf", "#636363"),
            ("--shadow", "#c4c4c4", "#141414"),
            ("--shadow-inset", "#454545", "#202020"),
            ("--shadow-subtle", "#737373", "#363636"),
            ("--shadow-focus", "#6366f1", "#6366f1"),
            ("--accent-card", "#60a5fa", "#93c5fd"),
            ("--accent-note", "#22c55e", "#4ade80"),
            ("--accent-danger", "#ef4444", "#f87171"),
            ("--flag-1", "#ef4444", "#f87171"),
            ("--flag-2", "#fb923c", "#fdba74"),
            ("--flag-3", "#4ade80", "#86efac"),
            ("--flag-4", "#3b82f6", "#60a5fa"),
            ("--flag-5", "#e879f9", "#f0abfc"),
            ("--flag-6", "#2dd4bf", "#5eead4"),
            ("--flag-7", "#a855f7", "#c084fc"),
            ("--state-new", "#3b82f6", "#93c5fd"),
            ("--state-learn", "#dc2626", "#f87171"),
            ("--state-review", "#16a34a", "#22c55e"),
            ("--state-buried", "#f59e0b", "#92400e"),
            ("--state-suspended", "#facc15", "#fef9c3"),
            ("--state-marked", "#6366f1", "#a855f7"),
            ("--highlight-bg", "rgba(37, 99, 235, 0.5)", "rgba(147, 197, 253, 0.5)"),
            ("--highlight-fg", "black", "white"),
            ("--selected-bg", "rgba(214, 214, 214, 0.5)", "rgba(147, 197, 253, 0.5)"),
            ("--selected-fg", "black", "white"),
        ]

        /// C4.2: the same in both appearances.
        static let otherTokens: [(name: String, value: String)] = [
            ("--font-size", "15px"),
            ("--border-radius", "5px"),
            ("--border-radius-medium", "12px"),
            ("--border-radius-large", "15px"),
            ("--transition", "180ms"),
            ("--transition-medium", "500ms"),
            ("--transition-slow", "1000ms"),
            ("--blur", "20px"),
            ("--focus-color", "#6366f1"),
        ]

        /// Computed values of `properties` on the element `selector` finds.
        static func computed(_ page: TestPage, _ selector: String, _ properties: [String]) async throws -> [String: String] {
            try await page.json(
                [String: String].self,
                """
                var style = getComputedStyle(document.querySelector(selector));
                var result = {};
                properties.forEach(function (p) { result[p] = style.getPropertyValue(p).trim(); });
                return JSON.stringify(result);
                """, ["selector": selector, "properties": properties])
        }

        // MARK: BC1

        @Test func bodyHasATwentyPixelMargin() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                let style = try await Self.computed(page, "body", ["margin-top", "margin-right", "margin-bottom", "margin-left"])
                #expect(Set(style.values) == ["20px"])
            }
        }

        // MARK: BC2

        @Test func boxSizingIsContentBoxExceptTheTypeInBox() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(#"<div id="d">x</div><input id="typeans">"#)
                #expect(try await Self.computed(page, "#d", ["box-sizing"]) == ["box-sizing": "content-box"])
                let typeans = try await Self.computed(page, "#typeans", ["box-sizing", "width", "line-height"])
                #expect(typeans["box-sizing"] == "border-box")
                // Full width of #qa: the page is 400 px wide, less the body's
                // 20 px margins.
                #expect(typeans["width"] == "360px")
            }
        }

        // MARK: BC3

        @Test func typeInComparisonColours() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    #"<span class="typeGood">a</span><span class="typeBad">b</span><span class="typeMissed">c</span><code id="typeans">d</code>"#)
                #expect(try await Self.computed(page, ".typeGood", ["background-color", "color"]) == ["background-color": "rgb(170, 255, 170)", "color": "rgb(0, 0, 0)"])
                #expect(try await Self.computed(page, ".typeBad", ["background-color", "color"]) == ["background-color": "rgb(255, 170, 170)", "color": "rgb(0, 0, 0)"])
                #expect(try await Self.computed(page, ".typeMissed", ["background-color", "color"]) == ["background-color": "rgb(204, 204, 204)", "color": "rgb(0, 0, 0)"])
                let code = try await Self.computed(page, "code#typeans", ["white-space", "font-variant-ligatures"])
                #expect(code["font-variant-ligatures"] == "none")
                #expect(code["white-space"] == "pre-wrap")
            }
        }

        // MARK: BC4, BC5

        @Test func flagAndMarkPositions() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.run("_drawFlag(1); _drawMark(true);")
                let flag = try await Self.computed(page, "#_flag", ["position", "right", "top", "font-size", "-webkit-text-stroke-width", "-webkit-text-stroke-color"])
                #expect(flag == [
                    "position": "fixed", "right": "10px", "top": "0px", "font-size": "30px",
                    "-webkit-text-stroke-width": "1px", "-webkit-text-stroke-color": "rgb(0, 0, 0)",
                ])
                let mark = try await Self.computed(page, "#_mark", ["position", "left", "top", "font-size", "color"])
                #expect(mark == ["position": "fixed", "left": "10px", "top": "0px", "font-size": "30px", "color": "rgb(255, 255, 0)"])
            }
        }

        // MARK: BC6

        @Test func flagOffsetNeedsTheDirectionSelectorToMove() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion("<style>#_flag { right: 50px }</style><p>x</p>")
                #expect(try await Self.computed(page, "#_flag", ["right"]) == ["right": "10px"])
                try await page.showPresentedQuestion("<style>[dir] #_flag { right: 50px }</style><p>x</p>")
                #expect(try await Self.computed(page, "#_flag", ["right"]) == ["right": "50px"])
            }
        }

        // MARK: BC7

        @Test(arguments: CardRuntime.Appearance.allCases)
        func tokensHoldTheTableText(appearance: CardRuntime.Appearance) async throws {
            try await WebTestHarness.withPage(appearance: appearance, log: false) { _, page async throws in
                let names = Self.colourTokens.map(\.name) + Self.otherTokens.map(\.name) + ["--flag-0"]
                let values = try await Self.computed(page, ":root", names)
                for token in Self.colourTokens {
                    #expect(values[token.name] == (appearance == .dark ? token.dark : token.light), "\(token.name)")
                }
                for token in Self.otherTokens {
                    #expect(values[token.name] == token.value, "\(token.name)")
                }
                #expect(values["--flag-0"] == "")
                let scheme = try await Self.computed(page, ":root", ["color-scheme"])
                #expect(scheme["color-scheme"] == appearance.rawValue)
            }
        }

        // MARK: BC8

        @Test func focusColourOnTheMacBody() async throws {
            try await WebTestHarness.withPage(platform: .mac, log: false) { _, page async throws in
                #expect(try await Self.computed(page, "body", ["--focus-color"]) == ["--focus-color": "rgba(0 103 244 / 0.247)"])
                #expect(try await Self.computed(page, "html", ["--focus-color"]) == ["--focus-color": "#6366f1"])
            }
            try await WebTestHarness.withPage(platform: .iPhone, log: false) { _, page async throws in
                #expect(try await Self.computed(page, "body", ["--focus-color"]) == ["--focus-color": "#6366f1"])
            }
        }

        // MARK: BC9, D1, D15

        @Test func htmlFontIsSystemStack() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                let html = try await Self.computed(page, "html", ["font-family"])
                #expect(html["font-family"]?.hasPrefix("system-ui") == true, "\(html)")
            }
        }

        @Test func viewportAndTextSizeAdjust() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                // The Mac's WebKit has no text autosizing and drops the
                // declaration; where the engine has it, it applies.
                if try await page.bool("CSS.supports('-webkit-text-size-adjust', '100%')") {
                    #expect(try await Self.computed(page, "html", ["-webkit-text-size-adjust"]) == ["-webkit-text-size-adjust": "100%"])
                }
                let css = String(decoding: try #require(CardRuntime.asset(at: "base.css")).data, as: UTF8.self)
                let htmlRule = try #require(css.firstMatch(of: /\nhtml \{[^}]*\}/))
                #expect(htmlRule.output.contains("-webkit-text-size-adjust: 100%;"))
                #expect(
                    try await page.string(#"document.querySelector('meta[name="viewport"]').content"#)
                        == "width=device-width, initial-scale=1, viewport-fit=cover")
                #expect(try await page.string("document.characterSet") == "UTF-8")
                #expect(try await page.string("document.baseURI") == WebTestHarness.mediaRoot.absoluteString)
            }
        }

        // MARK: BC10, D2

        @Test func rootFontSizeIsEngineDefault() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(#"<div id="r" style="width: 1rem">x</div>"#)
                #expect(try await Self.computed(page, "html", ["font-size"]) == ["font-size": "16px"])
                #expect(try await Self.computed(page, "body", ["font-size"]) == ["font-size": "16px"])
                #expect(try await Self.computed(page, "#r", ["width"]) == ["width": "16px"])
            }
        }

        // MARK: BC11, BC12

        @Test func ruleAndImageLimits() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(#"<hr><img id="i" alt="">"#)
                let hr = try await Self.computed(page, "hr", ["height", "background-color", "border-top-style", "margin-top"])
                #expect(hr == ["height": "1px", "background-color": "rgb(115, 115, 115)", "border-top-style": "none", "margin-top": "16px"])
                // 95vh of the 600 px page.
                #expect(try await Self.computed(page, "#i", ["max-height", "max-width"]) == ["max-height": "570px", "max-width": "100%"])
            }
        }

        // MARK: BC13, D3

        @Test func buttonKeepsPlatformAppearance() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion("<button>b</button>")
                let button = try await Self.computed(page, "button", ["margin-top", "margin-right", "margin-bottom", "margin-left", "appearance"])
                // 1em .5em at the button's font size.
                let size = try await Self.computed(page, "button", ["font-size"])["font-size"] ?? ""
                let pixels = try #require(Double(size.replacingOccurrences(of: "px", with: "")))
                #expect(button["margin-top"] == "\(Self.css(pixels))px" && button["margin-bottom"] == "\(Self.css(pixels))px")
                #expect(button["margin-left"] == "\(Self.css(pixels / 2))px" && button["margin-right"] == "\(Self.css(pixels / 2))px")
                #expect(button["appearance"] != "none")
                // At a 16 px font, 16px 8px.
                try await page.run(#"document.querySelector("button").style.fontSize = "16px";"#)
                #expect(try await Self.computed(page, "button", ["margin-top", "margin-left"]) == ["margin-top": "16px", "margin-left": "8px"])
            }
        }

        static func css(_ value: Double) -> String {
            value == value.rounded() ? String(Int(value)) : String(value)
        }

        // MARK: BC14

        @Test func replayButtonSize() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    ##"<a class="replay-button" href="#"><svg viewBox="0 0 64 64"><circle cx="32" cy="32" r="29"></circle><path d="M56.502,32.301l-37.502,20.101l0.329,-40.804l37.173,20.703Z"></path></svg></a>"##)
                #expect(try await Self.computed(page, ".replay-button svg", ["width", "height"]) == ["width": "40px", "height": "40px"])
                #expect(try await Self.computed(page, ".replay-button", ["display", "vertical-align", "margin-top", "text-decoration-line"]) == [
                    "display": "inline-flex", "vertical-align": "middle", "margin-top": "3px", "text-decoration-line": "none",
                ])
                #expect(try await Self.computed(page, ".replay-button svg circle", ["fill", "stroke"]) == ["fill": "rgb(255, 255, 255)", "stroke": "rgb(65, 65, 65)"])
                #expect(try await Self.computed(page, ".replay-button svg path", ["fill"]) == ["fill": "rgb(65, 65, 65)"])
            }
        }

        // MARK: BC15

        @Test func imageOcclusionLayout() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    #"<div id="image-occlusion-container"><img id="io" alt=""><canvas id="image-occlusion-canvas"></canvas></div>"#)
                #expect(try await Self.computed(page, "#image-occlusion-container", ["position", "margin-left", "max-height"]) == [
                    "position": "relative", "margin-left": "0px", "max-height": "530px",
                ])
                #expect(try await Self.computed(page, "#io", ["position", "top", "left", "max-width", "max-height"]) == [
                    "position": "absolute", "top": "0px", "left": "0px", "max-width": "none", "max-height": "none",
                ])
                #expect(try await page.string(#"document.getElementById("io").style.width === "" && getComputedStyle(document.getElementById("io")).width === getComputedStyle(document.getElementById("image-occlusion-container")).width"#) == "true")
                #expect(try await Self.computed(page, "#image-occlusion-canvas", ["position", "top", "left"]) == [
                    "position": "absolute", "top": "0px", "left": "0px",
                ])
            }
        }

        // MARK: C4.3: links and lists

        @Test func linksAndText() async throws {
            try await WebTestHarness.withPage(log: false) { _, page async throws in
                try await page.showPresentedQuestion(##"<a id="a" href="#">a</a><ul><li id="li">x</li></ul><pre id="pre">p</pre><h1 id="h">h</h1>"##)
                #expect(try await Self.computed(page, "#a", ["color", "text-decoration-line"]) == ["color": "rgb(29, 78, 216)", "text-decoration-line": "none"])
                #expect(try await Self.computed(page, "#li", ["text-align"]) == ["text-align": "start"])
                #expect(try await Self.computed(page, "#pre", ["text-align"]) == ["text-align": "left"])
                // .2em of the heading's 32 px, in layout units of 1/64 px.
                let heading = try await Self.computed(page, "#h", ["margin-bottom"])["margin-bottom"] ?? ""
                let margin = try #require(Double(heading.replacingOccurrences(of: "px", with: "")))
                #expect(abs(margin - 6.4) < 1.0 / 32, "\(heading)")
                let body = try await Self.computed(page, "body", [
                    "color", "background-color", "background-size", "background-repeat", "background-attachment", "overscroll-behavior-y", "overflow-wrap",
                ])
                #expect(body == [
                    "color": "rgb(2, 2, 2)", "background-color": "rgb(245, 245, 245)", "background-size": "cover",
                    "background-repeat": "no-repeat", "background-attachment": "fixed", "overscroll-behavior-y": "none",
                    "overflow-wrap": "break-word",
                ])
            }
        }
    }
}
