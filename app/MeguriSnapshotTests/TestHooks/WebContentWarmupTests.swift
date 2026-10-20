#if MEGURI_TEST_HOOKS
import CardRendering
import Foundation
import Testing
import WebKit
@testable import Meguri

@MainActor
@Suite(.serialized) struct WebContentWarmupTests {
    @Test func productionReviewerRoundTripCoalescesWithoutCollectionOrWindow() async throws {
        let warmup = TestHooks.WebContentWarmup()
        defer { warmup.close() }
        async let first = warmup.warm()
        async let second = warmup.warm()
        let results = await (first, second)
        #expect(results.0 && results.1)
        let page = try #require(warmup.page)
        #expect(page.webView.url == MediaOrigin.url)
        #expect(page.webView.configuration.websiteDataStore.identifier == CardWebConfiguration.storeIdentifier)
        #expect(page.webView.configuration.userContentController === CardWebConfiguration.shared.configuration.userContentController)
        #expect(page.webView.window == nil && page.webView.superview == nil)
        #expect(await page.answersWarmup())
        #expect(await warmup.warm())
        #expect(warmup.page === page)
        let mediaStatus = try await page.webView.callAsyncJavaScript("return (await fetch('missing-collection-file.png')).status;", arguments: [:], in: nil, contentWorld: .page)
        #expect(mediaStatus as? Int == 404)
    }

    @Test func failedRoundTripIsNotReadyAndTerminationDiscardsOldPage() async throws {
        let warmup = TestHooks.WebContentWarmup(configuration: CardWebConfiguration(store: .nonPersistent()))
        defer { warmup.close() }
        #expect(await warmup.warm())
        let old = try #require(warmup.page)
        // Break the actual answering document, never a fake ready callback.
        _ = try await old.webView.callAsyncJavaScript("document.getElementById('qa').remove();", arguments: [:], in: nil, contentWorld: .page)
        #expect(await old.answersWarmup() == false)
        #expect(await warmup.warm())
        let replacement = try #require(warmup.page)
        #expect(replacement !== old && old.closed)
        replacement.webViewWebContentProcessDidTerminate(replacement.webView)
        #expect(warmup.page == nil && replacement.closed)
        #expect(await replacement.answersWarmup() == false)
        #expect(await warmup.warm())
        #expect(warmup.page !== replacement)
        warmup.close()
        #expect(warmup.page == nil)
    }
}
#endif
