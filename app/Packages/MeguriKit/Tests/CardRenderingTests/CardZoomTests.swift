@testable import CardRendering
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite(.timeLimit(.minutes(2))) struct CardZoomTests {
        @Test func savedZoomClampsAndChangesRealWebKitDPRWithoutBodyScaling() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            #expect(CardZoom(percent: 0).percent == 50 && CardZoom(percent: 900).percent == 300)
            #expect(CardZoom(percent: .nan).percent == 100 && CardZoom().percent == 100)
            try await fixture.present(.init(cardID: 1, question: """
                <div id=px style="width:100px;height:20px"></div><div style="width:10rem"></div><div style="width:50vw"></div>
                <script>window.beforeDPR=devicePixelRatio;window.beforeWidth=innerWidth;window.beforeFont=getComputedStyle(document.documentElement).fontSize;
                window.observations=0;window.observer=new ResizeObserver(()=>window.observations++);observer.observe(document.documentElement);</script>
                """, answer: "a"))
            let epoch = fixture.pair.current.hostReports.viewRevision
            fixture.pair.cardZoom = CardZoom(percent: 150)
            #expect(fixture.pair.pages.allSatisfy { $0.webView.pageZoom == 1.5 })
            try await CardBehaviorFixture.until("native zoom did not reflow/DPR redraw") {
                try await fixture.bool("return devicePixelRatio>beforeDPR && innerWidth<beforeWidth && observations>1;") == true
            }
            #expect(try await fixture.bool("return getComputedStyle(document.body).zoom==='1' && getComputedStyle(document.documentElement).fontSize===beforeFont && document.getElementById('px').getBoundingClientRect().width===100;") == true)
            #expect(fixture.pair.current.hostReports.viewRevision > epoch)
        }

        @Test func liveResizeKeepsDOMTextMediaAndProportionalCSSScroll() async throws {
            let fixture = try await CardBehaviorFixture(video: true)
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: """
                <style>body{margin:0}#space{height:500vh;width:200vw}#bottom{position:fixed;bottom:0}#viewport{height:100vh}</style>
                <input id=typeans><video id=media src=clip.mp4 muted playsinline loop></video>
                <div id=viewport></div><div id=space></div><div id=bottom>bottom</div>
                <script>window.runs=(window.runs||0)+1;window.resizes=0;window.addEventListener('resize',()=>window.resizes++);</script>
                """, answer: "a"))
            let page = fixture.pair.current, web = page.webView, update = page.update
            try await fixture.script("""
                window.node=document.getElementById('typeans');window.video=document.getElementById('media');
                node.value='ka';node.dispatchEvent(new Event('input',{bubbles:true}));await video.play();
                const e=document.scrollingElement;scrollTo((e.scrollWidth-e.clientWidth)*.3,(e.scrollHeight-e.clientHeight)*.4);
                await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));window.timeBefore=video.currentTime;
                """)
            let epoch = page.hostReports.viewRevision
            fixture.pair.container.frame.size = CGSize(width: 520, height: 440)
            try await CardBehaviorFixture.until("resize continuity did not settle") {
                try await fixture.bool("return resizes>0 && Math.abs(scrollY/(document.scrollingElement.scrollHeight-document.scrollingElement.clientHeight)-.4)<.04;") == true
            }
            #expect(fixture.pair.current === page && page.webView === web && page.update == update)
            try await CardBehaviorFixture.until("live media stopped advancing during resize") {
                try await fixture.bool("return !video.paused && video.currentTime>timeBefore;") == true
            }
            #expect(try await fixture.bool("return node===document.getElementById('typeans') && node.value==='ka' && video===document.getElementById('media') && !video.paused && video.currentTime>=timeBefore && runs===1;") == true)
            #expect(try await fixture.bool("return Math.abs(document.getElementById('viewport').getBoundingClientRect().height-innerHeight)<1 && Math.abs(document.getElementById('bottom').getBoundingClientRect().bottom-document.documentElement.clientHeight)<1;") == true)
            let nativeHeight = web.bounds.height
            let matchesNativeHeight = try await web.callAsyncJavaScript("return Math.abs(innerHeight-nativeHeight)<1;",
                arguments: ["nativeHeight": nativeHeight], in: nil, contentWorld: .page) as? Bool
            #expect(matchesNativeHeight == true && web.pageZoom == 1)
            #expect(page.hostReports.viewRevision > epoch)
            #expect(try await fixture.bool("return Math.abs(scrollX/(document.scrollingElement.scrollWidth-document.scrollingElement.clientWidth)-.3)<.04;") == true)
            #if !os(macOS)
            #expect(web.scrollView.contentInsetAdjustmentBehavior == .never)
            #endif
            // Continuity was asserted at its preserved scroll fractions above.
            // Capture the real input/video/footer rather than the blank spacer.
            try await fixture.script("scrollTo(0,0);await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));")
            try await fixture.snapshot("layout-light")
            try await fixture.pair.setAppearance(.dark)
            try await fixture.snapshot("layout-dark")
        }

        @Test func zeroExtentAndFreshInputNeverRestoreAnOldScroll() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            try await fixture.present(.init(cardID: 1, question: "<style>body{margin:0}</style><input id=typeans>", answer: "a"))
            fixture.pair.container.frame.size = CGSize(width: 300, height: 500)
            try await fixture.script("await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))); ")
            #expect(try await fixture.bool("return scrollX===0 && scrollY===0;") == true)
            try await fixture.script("document.getElementById('qa').style.height='3000px';scrollTo(0,600);await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))); ")
            fixture.pair.container.frame.size = CGSize(width: 350, height: 400)
            try await fixture.script("document.dispatchEvent(new Event('input',{bubbles:true}));scrollTo(0,25);await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))); ")
            #expect(try await fixture.bool("return Math.abs(scrollY-25)<1;") == true)
        }

        #if !os(macOS)
        @Test func temporaryPinchSurvivesRevealAndResetsOnNextVisit() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let payload = CardPayload(cardID: 1, question: "<p>pinch question</p>", answer: "<p id=answer>answer</p>")
            fixture.pair.cardZoom = CardZoom(percent: 150)
            try await fixture.present(payload)
            let web = fixture.pair.current.webView
            web.scrollView.setZoomScale(2, animated: false)
            #expect(web.scrollView.zoomScale > 1)
            let pinch = web.scrollView.zoomScale
            _ = try await fixture.pair.showAnswer()
            #expect(abs(web.scrollView.zoomScale - pinch) < 0.01 && web.pageZoom == 1.5)
            // The same card ID on a new visit also resets temporary zoom.
            try await fixture.present(payload)
            #expect(fixture.pair.current.webView.scrollView.zoomScale == fixture.pair.current.webView.scrollView.minimumZoomScale)
            #expect(fixture.pair.cardZoom.percent == 150)
        }
        #endif
    }
}
