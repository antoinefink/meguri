import CardRendering
import Foundation
import Testing

extension WebKitSuites.CardRuntimeSuites {
    /// Night mode and the page's classes (specs/render/card-runtime.md C2.2,
    /// C3.1, C5, C16.5; S13; R475, R476, R488–R492).
    @Suite struct NightModeTests {
        /// S13: the card's own colours, `.card { background: white; color: black }`.
        static func nightCard() throws -> String { try RuntimeFixture.text("s13-night-card.html") }

        static func styles(_ page: TestPage, _ selector: String) async throws -> [String: String] {
            try await BaseCSSTests.computed(page, selector, ["background-color", "color", "filter"])
        }

        // MARK: NM7, D19, D20

        @Test func htmlClassesExact() async throws {
            let expected: [CardRuntime.Platform: (light: String, dark: String)] = [
                .mac: ("mac safari", "night-mode mac safari"),
                .iPhone: ("iphone ios mobile safari", "night-mode iphone ios mobile safari"),
                .iPad: ("ipad ios mobile safari", "night-mode ipad ios mobile safari"),
            ]
            for (platform, classes) in expected {
                #expect(CardRuntime.htmlClasses(platform: platform, appearance: .light) == classes.light)
                #expect(CardRuntime.htmlClasses(platform: platform, appearance: .dark) == classes.dark)
                for appearance in CardRuntime.Appearance.allCases {
                    try await WebTestHarness.withPage(platform: platform, appearance: appearance, log: false) { _, page async throws in
                        #expect(try await page.string("document.documentElement.className") == (appearance == .dark ? classes.dark : classes.light))
                        #expect(try await page.string(#"document.documentElement.getAttribute("data-bs-theme")"#) == appearance.rawValue)
                        #expect(try await page.string("document.documentElement.dir") == "ltr")
                    }
                }
            }
        }

        @Test func bodyClassesExact() async throws {
            let initial: [CardRuntime.Platform: (light: String, dark: String)] = [
                .mac: ("isMac", "isMac nightMode night_mode macos-dark-mode"),
                .iPhone: ("", "nightMode night_mode"),
                .iPad: ("", "nightMode night_mode"),
            ]
            let card: [CardRuntime.Platform: (light: String, dark: String)] = [
                .mac: ("card card1 isMac", "card card1 isMac nightMode night_mode macos-dark-mode"),
                .iPhone: ("card card3", "card card3 nightMode night_mode"),
                .iPad: ("card card3", "card card3 nightMode night_mode"),
            ]
            for platform in CardRuntime.Platform.allCases {
                let ordinal = platform == .mac ? 0 : 2
                for appearance in CardRuntime.Appearance.allCases {
                    let dark = appearance == .dark
                    let start = dark ? initial[platform]!.dark : initial[platform]!.light
                    let forCard = dark ? card[platform]!.dark : card[platform]!.light
                    #expect(CardRuntime.bodyClasses(platform: platform, appearance: appearance) == start)
                    #expect(CardRuntime.bodyClasses(platform: platform, appearance: appearance, cardOrdinal: ordinal) == forCard)
                    try await WebTestHarness.withPage(platform: platform, appearance: appearance, log: false) { _, page async throws in
                        #expect(try await page.string("document.body.className") == start)
                        try await page.showPresentedQuestion("<p>x</p>", bodyClass: forCard)
                        #expect(try await page.string("document.body.className") == forCard)
                        for absent in ["fancy", "reduce-motion", "no-blur", "isWin", "isLin", "ankidroid_dark_mode"] {
                            #expect(!forCard.split(separator: " ").contains(Substring(absent)))
                        }
                    }
                }
            }
        }

        // MARK: NM1

        @Test func darkAppearanceSetsEveryNightSignal() async throws {
            try await WebTestHarness.withPage(platform: .mac, appearance: .dark, log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    try Self.nightCard(), bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: .dark, cardOrdinal: 0))
                #expect(try await page.string("document.documentElement.classList[0]") == "night-mode")
                #expect(try await page.string(#"document.documentElement.getAttribute("data-bs-theme")"#) == "dark")
                #expect(try await page.string("document.body.className") == "card card1 isMac nightMode night_mode macos-dark-mode")
                let body = try await Self.styles(page, "body")
                #expect(body["background-color"] == "rgb(44, 44, 44)" && body["color"] == "rgb(252, 252, 252)")
                #expect(try await page.bool(#"matchMedia("(prefers-color-scheme: dark)").matches"#))
            }
        }

        // MARK: NM2, R488

        @Test func cardNightModeRuleWins() async throws {
            try await WebTestHarness.withPage(appearance: .dark, log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    try Self.nightCard() + "<style>.card.nightMode { background: navy }</style>",
                    bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: .dark, cardOrdinal: 0))
                #expect(try await Self.styles(page, "body")["background-color"] == "rgb(0, 0, 128)")
            }
        }

        // MARK: NM3

        @Test func lightAppearanceLeavesTheCardsColours() async throws {
            try await WebTestHarness.withPage(appearance: .light, log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    try Self.nightCard(), bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: .light, cardOrdinal: 0))
                #expect(try await page.string("document.documentElement.className.indexOf('night')") == "-1")
                #expect(try await page.string("document.body.className.indexOf('ight')") == "-1")
                let body = try await Self.styles(page, "body")
                #expect(body["background-color"] == "rgb(255, 255, 255)" && body["color"] == "rgb(0, 0, 0)")
                #expect(try await page.bool(#"matchMedia("(prefers-color-scheme: light)").matches"#))
            }
        }

        // MARK: NM4, NM5, R492, R565

        @Test(arguments: CardRuntime.Appearance.allCases)
        func onlyLatexAndDrawingsAreInverted(appearance: CardRuntime.Appearance) async throws {
            try await WebTestHarness.withPage(appearance: appearance, log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    #"<img class="latex" id="l" alt=""><img class="drawing" id="d" alt=""><img id="p" alt=""><span class="latex" id="s">x</span>"#,
                    bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: appearance, cardOrdinal: 0))
                let dark = appearance == .dark
                #expect(try await Self.styles(page, "#l")["filter"] == (dark ? "invert(1)" : "none"))
                #expect(try await Self.styles(page, "#d")["filter"] == (dark ? "invert(1) hue-rotate(180deg)" : "none"))
                #expect(try await Self.styles(page, "#p")["filter"] == "none")
                #expect(try await Self.styles(page, "#s")["filter"] == (dark ? "invert(1)" : "none"))
                #expect(try await Self.styles(page, "html")["filter"] == "none")
                #expect(try await Self.styles(page, "body")["filter"] == "none")
                #expect(try await BaseCSSTests.computed(page, "#d", ["zoom"]) == ["zoom": "0.5"])
            }
        }

        // MARK: NM6, R492

        @Test func imageOcclusionColoursAreTheCards() async throws {
            var values: [String] = []
            for appearance in CardRuntime.Appearance.allCases {
                try await WebTestHarness.withPage(appearance: appearance, log: false) { _, page async throws in
                    try await page.showPresentedQuestion(
                        try RuntimeFixture.text("s13-night-io.html"),
                        bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: appearance, cardOrdinal: 0))
                    let canvas = try await BaseCSSTests.computed(
                        page, "#image-occlusion-canvas", ["--inactive-shape-color", "--active-shape-color", "filter"])
                    values.append("\(canvas["--inactive-shape-color"]!) \(canvas["--active-shape-color"]!) \(canvas["filter"]!)")
                }
            }
            #expect(values == ["#ffeba2 #ff8e8e none", "#ffeba2 #ff8e8e none"])
        }

        // MARK: C5: a live switch (R490)

        @Test func liveSwitchKeepsClassOrderAndRunsNothing() async throws {
            try await WebTestHarness.withPage(platform: .mac, appearance: .light) { _, page async throws in
                try await page.showPresentedQuestion(
                    "<script>window.__runs = (window.__runs || 0) + 1;</script>",
                    bodyClass: CardRuntime.bodyClasses(platform: .mac, appearance: .light, cardOrdinal: 0))
                try await page.runIsolated("meguriCard.setAppearance(true, true);")
                #expect(try await page.string("document.body.className") == "card card1 isMac night_mode nightMode macos-dark-mode")
                #expect(try await page.string("document.documentElement.className") == "mac safari night-mode")
                #expect(try await page.string(#"document.documentElement.getAttribute("data-bs-theme")"#) == "dark")
                try await page.runIsolated("meguriCard.setAppearance(false, true);")
                #expect(try await page.string("document.body.className") == "card card1 isMac")
                #expect(try await page.string("document.documentElement.className") == "mac safari")
                #expect(try await page.string(#"document.documentElement.getAttribute("data-bs-theme")"#) == "light")
                #expect(try await page.string("window.__runs") == "1")
            }
            try await WebTestHarness.withPage(platform: .iPhone, appearance: .dark, log: false) { _, page async throws in
                try await page.showPresentedQuestion(
                    "<p>x</p>", bodyClass: CardRuntime.bodyClasses(platform: .iPhone, appearance: .dark, cardOrdinal: 1))
                try await page.runIsolated("meguriCard.setAppearance(false, false);")
                #expect(try await page.string("document.body.className") == "card card2")
                #expect(try await page.string("document.documentElement.className") == "iphone ios mobile safari")
                try await page.runIsolated("meguriCard.setAppearance(true, false);")
                #expect(try await page.string("document.body.className") == "card card2 night_mode nightMode")
            }
        }
    }
}
