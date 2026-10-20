@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct LinkPolicyTests {
        @Test func browserDistributionAndSchemeStorage() throws {
            let url = try #require(URL(string: "https://example.com/"))
            for platform in CardRuntime.Platform.allCases {
                #expect(LinkPolicy.route(url, root: MediaOrigin.url, platform: platform, preferExternal: false) == .browser(platform == .mac ? .external : .safari))
                #expect(LinkPolicy.route(url, root: MediaOrigin.url, platform: platform, preferExternal: true) == .browser(.external))
            }
            let suite = "card-behavior.\(UUID())"
            let defaults = try #require(UserDefaults(suiteName: suite))
            defer { defaults.removePersistentDomain(forName: suite) }
            let approvals = CardSchemeApprovals(defaults: defaults)
            approvals.approve("MKDICTIONARIES"); approvals.approve("file"); approvals.approve("bad://path")
            #expect(CardSchemeApprovals(defaults: defaults).contains("mkdictionaries"))
            #expect(!approvals.contains("file") && !approvals.contains("bad://path"))
            #expect(defaults.stringArray(forKey: "card.approvedURLSchemes") == ["mkdictionaries"])
        }

        @Test func actualNavigationFragmentsJavascriptWindowsAndFramesKeepRoot() async throws {
            let fixture = try await CardBehaviorFixture(platform: .iPhone)
            defer { fixture.close() }
            var actions: [CardBridgeAction] = []
            fixture.pair.onAction = { actions.append($0) }
            try await fixture.present(.init(cardID: 7, question: """
                <a id=fragment href="#lower">lower</a><a id=js href="javascript:window.jsRan=1;void(0)">js</a>
                <a id=blank href="https://example.com/blank" target=_blank>blank</a>
                <div style="height:2000px"></div><p id=lower>lower target</p>
                """, answer: "a"))
            let page = fixture.pair.current
            try await fixture.script("document.getElementById('fragment').click();")
            try await CardBehaviorFixture.until("fragment did not scroll") { try await fixture.bool("return scrollY>100;") == true }
            #expect(page.webView.url == MediaOrigin.url)
            try await fixture.script("document.getElementById('js').click();")
            try await CardBehaviorFixture.until("javascript link did not run") { try await fixture.bool("return window.jsRan===1;") == true }
            try await fixture.script("document.getElementById('blank').click();")
            try await CardBehaviorFixture.until("blank did not route") { fixture.presenter.opens.count == 1 }
            try await fixture.script("window.open('https://example.com/window');")
            try await CardBehaviorFixture.until("window.open did not route") { fixture.presenter.opens.count == 2 }
            #expect(fixture.presenter.opens.map(\.browser) == [.safari, .safari])
            try await fixture.script("location.href='https://example.com/script';")
            try await CardBehaviorFixture.until("direct script did not route") { fixture.presenter.opens.count == 3 }
            page.behavior.preferExternalBrowser = true
            try await fixture.script("window.open('https://example.com/preferred');")
            try await CardBehaviorFixture.until("external preference was not applied") { fixture.presenter.opens.count == 4 }
            #expect(fixture.presenter.opens.last?.browser == .external)
            try await fixture.script("location.href='file:///blocked';")
            try await fixture.script("const f=document.createElement('iframe');f.src='https://example.com/frame';document.body.append(f);")
            try await fixture.script("await new Promise(r=>setTimeout(r,50));")
            #expect(fixture.presenter.opens.count == 4 && fixture.presenter.dialogs.isEmpty)
            #expect(page.webView.url == MediaOrigin.url && fixture.pair.pages.count == 2)
            #expect(actions.isEmpty && fixture.pair.current.update?.side == .question)
            try await fixture.script("document.getElementById('fragment').href='#';document.getElementById('fragment').click();")
            try await CardBehaviorFixture.until("empty fragment did not scroll to top") { try await fixture.bool("return scrollY===0;") == true }
            #expect(page.webView.url == MediaOrigin.url)
            try await fixture.script("document.getElementById('fragment').href='#missing';document.getElementById('fragment').click();")
            try await fixture.script("await new Promise(r=>requestAnimationFrame(r));")
            #expect(try await fixture.bool("return scrollY===0;") == true)
        }

        @Test func unknownSchemeRequiresOneApprovalAndRejectsInactiveOrStaleAttempts() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let suite = "card-unknown.\(UUID())"
            let defaults = try #require(UserDefaults(suiteName: suite))
            defer { defaults.removePersistentDomain(forName: suite) }
            let approvals = CardSchemeApprovals(defaults: defaults)
            fixture.pair.configureLinks(preferExternalBrowser: true, approvals: approvals)
            fixture.presenter.schemeAvailability["synthetic-device-app"] = .unknown
            let url = try #require(URL(string: "synthetic-device-app://entry"))
            fixture.pair.idle.behavior.route(url)
            #expect(fixture.presenter.dialogs.isEmpty)
            try await fixture.present(.init(cardID: 1, question: "q", answer: "a"))
            let behavior = fixture.pair.current.behavior
            behavior.route(url)
            #expect(fixture.presenter.dialogs == [.scheme("synthetic-device-app")])
            fixture.presenter.replies[0](.confirm(true))
            #expect(approvals.contains("synthetic-device-app") && fixture.presenter.opens.count == 1)
            behavior.route(url)
            #expect(fixture.presenter.dialogs.count == 1 && fixture.presenter.opens.count == 2)
            await fixture.pair.suspend()
            behavior.route(url)
            #expect(fixture.presenter.opens.count == 2)
            await fixture.pair.resume()
            fixture.presenter.schemeAvailability["synthetic-second-app"] = .unknown
            behavior.route(try #require(URL(string: "synthetic-second-app://entry")))
            try await fixture.present(.init(cardID: 2, question: "new", answer: "a"))
            fixture.presenter.replies[1](.confirm(true))
            #expect(!approvals.contains("synthetic-second-app") && fixture.presenter.opens.count == 2)
            behavior.route(url)
            fixture.pair.close()
            fixture.pair.current.behavior.route(url)
            #expect(fixture.presenter.opens.count == 2)
        }

        @Test func schemeApprovalCancelHandlersAndStaleConfirmation() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let suite = "card-behavior.\(UUID())"
            let defaults = try #require(UserDefaults(suiteName: suite))
            defer { defaults.removePersistentDomain(forName: suite) }
            let approvals = CardSchemeApprovals(defaults: defaults)
            try await fixture.present(.init(cardID: 1, question: "links", answer: "a"))
            let behavior = fixture.pair.current.behavior
            behavior.approvals = approvals
            func route(_ value: String) throws { behavior.route(try #require(URL(string: value))) }
            try route("missing-handler://x"); try route("file:///blocked")
            #expect(fixture.presenter.dialogs.isEmpty && fixture.presenter.opens.isEmpty)
            try route("MKDICTIONARIES://first")
            fixture.presenter.replies[0](.confirm(false))
            #expect(!approvals.contains("mkdictionaries") && fixture.presenter.opens.isEmpty)
            try route("mkdictionaries://second")
            fixture.presenter.replies[1](.confirm(true))
            #expect(approvals.contains("mkdictionaries") && fixture.presenter.opens.count == 1)
            try route("mkdictionaries://third")
            #expect(fixture.presenter.dialogs.count == 2 && fixture.presenter.opens.count == 2)
            fixture.presenter.handledSchemes.insert("another-app")
            try route("another-app://x")
            await fixture.pair.suspend()
            fixture.presenter.replies[2](.confirm(true))
            #expect(!approvals.contains("another-app") && fixture.presenter.opens.count == 2)
            #expect(fixture.presenter.dismissals == 1)
        }
    }
}
