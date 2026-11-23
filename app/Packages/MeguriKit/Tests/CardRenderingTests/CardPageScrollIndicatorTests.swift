#if !os(macOS)
import Testing
import WebKit
@testable import CardRendering

extension WebKitSuites {
    /// N16.10: the review card's page shows no scroll indicators on touch (iPhone,
    /// iPad, a foldable folded or opened). The Mac keeps the system's behaviour,
    /// which follows its "Show scroll bars" setting, so it has no case here.
    @Suite struct CardPageScrollIndicatorTests {
        @Test func reviewCardShowsNoScrollIndicatorsOnTouch() {
            let page = CardWebConfiguration(store: .nonPersistent()).makeWarmupPage()
            defer { page.close() }
            #expect(!page.webView.scrollView.showsVerticalScrollIndicator)
            #expect(!page.webView.scrollView.showsHorizontalScrollIndicator)
            #expect(page.webView.scrollView.isScrollEnabled, "the card still scrolls")
        }
    }
}
#endif
