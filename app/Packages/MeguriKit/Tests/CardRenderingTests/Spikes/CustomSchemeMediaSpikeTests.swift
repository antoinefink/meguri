import Foundation
import Testing
import WebKit

extension RenderingSpikes {
    /// Spike S2 (U65): a custom-scheme media origin supports Range, fetch,
    /// modules, fonts and persistent storage, and keeps scripts in media
    /// documents from running (R493–R503, R560; KTD11). Kept as regression
    /// tests: U67 re-runs them against its production handler. Results and
    /// the chosen configuration: docs/design/decisions/s2-custom-scheme-media.md.
    ///
    /// These run on the macOS host in an off-screen window. Relaunch
    /// persistence (S2(d)) and the iOS simulator's media checks are in
    /// `app/MeguriUITests/CardSwapTimingSpikeTests.swift`.
    @Suite struct CustomSchemeMediaSpikeTests {
        /// A page that runs card scripts, at `/_anki/` with the media root as
        /// its base, as the runtime page would be.
        static func page(body: String = "", head: String = "") -> String {
            """
            <!doctype html><html><head><meta charset="utf-8"><base href="\(Spike.origin.absoluteString)">\(head)</head>
            <body>\(body)</body></html>
            """
        }

        /// A handler, a page and its window, torn down after `body`.
        static func withPage<T>(
            _ handler: SpikeMediaHandler, html: String = page(), store: WKWebsiteDataStore = .nonPersistent(),
            origin: URL = Spike.origin, _ body: (SpikeWebView) async throws -> T
        ) async throws -> T {
            handler.add("/_anki/card.html", text: html, "text/html")
            let configuration = Spike.configuration(handler: handler, store: store)
            let page = SpikeWebView(frame: CGRect(x: 0, y: 0, width: 400, height: 600), configuration: configuration)
            let window = SpikeWindow(page)
            defer { window.close() }
            try await page.open(origin.appending(path: "_anki/card.html"))
            return try await body(page)
        }

        // MARK: S2(a): Range/206 seek

        struct SeekResult: Decodable {
            let duration: Double
            let seekedTo: Double
            let playedTo: Double
            let paused: Bool
            let error: String?
        }

        /// Seeks a media element to 90 %, plays, and reports how far it got.
        static let seekScript = """
            const media = document.getElementById("media");
            const event = (name) => new Promise((resolve, reject) => {
              media.addEventListener(name, resolve, { once: true });
              media.addEventListener("error", () => reject(new Error("media error " + (media.error && media.error.code))), { once: true });
            });
            try {
              if (media.readyState < 1) await event("loadedmetadata");
              const duration = media.duration;
              media.currentTime = duration * 0.9;
              await event("seeked");
              const seekedTo = media.currentTime;
              await media.play();
              const until = performance.now() + 3000;
              while (media.currentTime < seekedTo + 0.4 && performance.now() < until) {
                await new Promise((resolve) => setTimeout(resolve, 50));
              }
              return JSON.stringify({ duration, seekedTo, playedTo: media.currentTime, paused: media.paused, error: null });
            } catch (error) {
              return JSON.stringify({ duration: media.duration || 0, seekedTo: 0, playedTo: 0, paused: true, error: String(error) });
            }
            """

        /// The ranged answers to requests for `path`: 206s whose Content-Range
        /// names the bytes sent and the file's length.
        static func rangedAnswers(_ handler: SpikeMediaHandler, path: String, total: Int) -> [SpikeMediaHandler.Request] {
            handler.requests(for: path).filter { request in
                guard request.status == 206, let header = request.contentRange,
                    let match = header.wholeMatch(of: /bytes (\d+)-(\d+)\/(\d+)/),
                    let first = Int(match.1), let last = Int(match.2), let length = Int(match.3)
                else { return false }
                return length == total && last - first + 1 == request.bodyLength
            }
        }

        @Test func anEightMegabyteMP3SeeksToNinetyPercentAndPlaysThroughRangeRequests() async throws {
            let handler = SpikeMediaHandler()
            let mp3 = SpikeAssets.silentMP3(minimumBytes: 8 * 1024 * 1024)
            handler.add("/_long.mp3", mp3.data, "audio/mpeg")
            let html = Self.page(body: #"<audio id="media" src="_long.mp3" preload="auto"></audio>"#)
            let result = try await Self.withPage(handler, html: html) { page in
                try await page.json(SeekResult.self, Self.seekScript)
            }
            #expect(result.error == nil, "\(result.error ?? "")")
            #expect(abs(result.duration - mp3.duration) < 2, "duration \(result.duration), file \(mp3.duration)")
            #expect(result.seekedTo >= mp3.duration * 0.89, "seeked to \(result.seekedTo) of \(mp3.duration)")
            #expect(result.playedTo > result.seekedTo + 0.3, "played from \(result.seekedTo) to \(result.playedTo)")
            let ranged = Self.rangedAnswers(handler, path: "/_long.mp3", total: mp3.data.count)
            // At least one ranged request starts in the last fifth of the file:
            // the seek fetched from there instead of reading everything.
            let late = ranged.filter { request in
                let first = request.contentRange.flatMap { Int($0.wholeMatch(of: /bytes (\d+)-.*/)?.1 ?? "") } ?? 0
                return first >= mp3.data.count * 8 / 10
            }
            #expect(!late.isEmpty, "no ranged request near 90 %: \(handler.requests(for: "/_long.mp3"))")
            print("S2(a) mp3: duration \(result.duration) s, seeked to \(result.seekedTo), played to \(result.playedTo); requests: \(handler.requests(for: "/_long.mp3"))")
        }

        @Test func aVideoSeeksToNinetyPercentAndPlaysThroughRangeRequests() async throws {
            let handler = SpikeMediaHandler()
            let mp4 = try await Spike.deadline("making the MP4", timeout: .seconds(30)) { try await SpikeAssets.mp4() }
            handler.add("/_clip.mp4", mp4, "video/mp4")
            let html = Self.page(body: #"<video id="media" src="_clip.mp4" preload="auto" muted playsinline width="320" height="240"></video>"#)
            let result = try await Self.withPage(handler, html: html) { page in
                try await page.json(SeekResult.self, Self.seekScript)
            }
            #expect(result.error == nil, "\(result.error ?? "")")
            #expect(result.seekedTo >= result.duration * 0.89, "seeked to \(result.seekedTo) of \(result.duration)")
            #expect(result.playedTo > result.seekedTo + 0.3, "played from \(result.seekedTo) to \(result.playedTo)")
            #expect(!Self.rangedAnswers(handler, path: "/_clip.mp4", total: mp4.count).isEmpty, "\(handler.requests(for: "/_clip.mp4"))")
            print("S2(a) mp4: \(mp4.count) bytes, duration \(result.duration) s; requests: \(handler.requests(for: "/_clip.mp4"))")
        }

        // MARK: S2(b): fetch and dynamic import

        struct FetchImportResult: Decodable {
            let imported: String?
            let fetched: String?
            let error: String?
        }

        static let fetchImportScript = """
            try {
              const module = await import("./_lib.js");
              const data = await (await fetch("_data.json")).json();
              return JSON.stringify({ imported: module.value, fetched: data.value, error: null });
            } catch (error) {
              return JSON.stringify({ imported: null, fetched: null, error: String(error) });
            }
            """

        static func addModuleAndData(_ handler: SpikeMediaHandler) {
            handler.add("/_lib.js", text: #"export const value = "from the module";"#, "text/javascript")
            handler.add("/_data.json", text: #"{"value": "from the data"}"#, "application/json")
        }

        @Test func aCardPageImportsAModuleAndFetchesDataFromTheMediaOrigin() async throws {
            let handler = SpikeMediaHandler()
            Self.addModuleAndData(handler)
            let result = try await Self.withPage(handler) { page in
                try await page.json(FetchImportResult.self, Self.fetchImportScript)
            }
            #expect(result.error == nil, "\(result.error ?? "")")
            #expect(result.imported == "from the module")
            #expect(result.fetched == "from the data")
            #expect(handler.requests(for: "/_lib.js").map(\.status) == [200])
            #expect(handler.requests(for: "/_data.json").map(\.status) == [200])
        }

        // MARK: S2(c): root-relative @font-face and <base href>

        struct FontsResult: Decodable {
            let rootRelative: Bool
            let relative: Bool
            let images: [Bool]
        }

        @Test func rootRelativeFontFacesAndRelativeMediaResolveAgainstTheMediaRoot() async throws {
            let handler = SpikeMediaHandler()
            let fonts = try SpikeAssets.fonts(2)
            handler.add("/_root.ttf", fonts[0], "font/ttf")
            handler.add("/_relative.ttf", fonts[1], "font/ttf")
            let png = try SpikeAssets.solidPNG()
            for name in ["/plain.png", "/dotted.png", "/rooted.png"] { handler.add(name, png, "image/png") }
            // The page lives under /_anki/; its base makes relative URLs, in the
            // document and in its inline style, resolve at the media root.
            let html = Self.page(
                body: """
                    <p style="font-family: RootFace">root</p><p style="font-family: RelativeFace">relative</p>
                    <img src="plain.png"><img src="./dotted.png"><img src="/rooted.png">
                    """,
                head: """
                    <style>
                    @font-face { font-family: RootFace; src: url("/_root.ttf"); font-display: block; }
                    @font-face { font-family: RelativeFace; src: url("_relative.ttf"); font-display: block; }
                    </style>
                    """)
            let result = try await Self.withPage(handler, html: html) { page in
                try await page.json(
                    FontsResult.self,
                    """
                    await document.fonts.ready;
                    await Promise.all([...document.images].map((image) => image.decode().catch(() => {})));
                    return JSON.stringify({
                      rootRelative: document.fonts.check('16px RootFace'),
                      relative: document.fonts.check('16px RelativeFace'),
                      images: [...document.images].map((image) => image.complete && image.naturalWidth > 0),
                    });
                    """)
            }
            #expect(result.rootRelative)
            #expect(result.relative)
            #expect(result.images == [true, true, true])
            for path in ["/_root.ttf", "/_relative.ttf", "/plain.png", "/dotted.png", "/rooted.png"] {
                #expect(handler.requests(for: path).map(\.status) == [200], "\(path): \(handler.requests)")
            }
        }

        // MARK: S2(e): scripts in media documents

        @Test func anSVGOpenedAsADocumentRunsNoScriptWhileTheCardsOwnScriptRuns() async throws {
            let handler = SpikeMediaHandler()
            // Each script tells the handler it ran by fetching a marker path.
            handler.add("/_x.js", text: #"fetch("/_marker/card-script");"#, "text/javascript")
            handler.add(
                "/_drawing.svg",
                text: """
                    <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
                    <script>fetch("/_marker/svg-script");</script><rect width="10" height="10"/></svg>
                    """, "image/svg+xml")
            handler.add(
                "/_note.html", text: #"<!doctype html><p>media</p><script>fetch("/_marker/html-script");</script>"#, "text/html")
            let html = Self.page(
                body: #"<script src="_x.js"></script><iframe src="_drawing.svg"></iframe><iframe src="_note.html"></iframe>"#)
            try await Self.withPage(handler, html: html) { page in
                // Each frame has loaded (or been refused) before the check.
                try await page.run(
                    """
                    await Promise.all([...document.querySelectorAll("iframe")].map((frame) =>
                      frame.contentDocument && frame.contentDocument.readyState === "complete"
                        ? null : new Promise((resolve) => { frame.onload = resolve; setTimeout(resolve, 2000); })));
                    await new Promise((resolve) => setTimeout(resolve, 300));
                    """)
                // The SVG opened as the top-level document.
                try await page.open(Spike.origin.appending(path: "_drawing.svg"))
                try await Task.sleep(for: .milliseconds(300))
            }
            #expect(!handler.requests(for: "/_marker/card-script").isEmpty, "the card's script did not run: \(handler.requests)")
            #expect(handler.requests(for: "/_marker/svg-script").isEmpty, "the SVG's script ran")
            #expect(handler.requests(for: "/_marker/html-script").isEmpty, "the HTML media's script ran")
            #expect(handler.requests(for: "/_drawing.svg").count == 2, "\(handler.requests)")
        }

        // MARK: S2(d) and S2(f): storage and the origin form

        struct OriginResult: Decodable {
            let origin: String
            let fetched: Bool
            let imported: Bool
            let localStorage: String?
            let indexedDB: String?
            let errors: [String]
        }

        /// What a page at `origin` can do: fetch, import, and keep a value in
        /// localStorage and IndexedDB (written when `write` is set, read back
        /// either way).
        static let originScript = """
            const errors = [];
            let fetched = false, imported = false, stored = null, indexed = null;
            try { fetched = (await (await fetch("_data.json")).json()).value === "from the data"; } catch (e) { errors.push("fetch: " + e); }
            try { imported = (await import("./_lib.js")).value === "from the module"; } catch (e) { errors.push("import: " + e); }
            try {
              if (write) localStorage.setItem("spike", write);
              stored = localStorage.getItem("spike");
            } catch (e) { errors.push("localStorage: " + e); }
            try {
              const db = await new Promise((resolve, reject) => {
                const open = indexedDB.open("spike", 1);
                open.onupgradeneeded = () => open.result.createObjectStore("values");
                open.onsuccess = () => resolve(open.result);
                open.onerror = () => reject(open.error);
              });
              if (write) await new Promise((resolve, reject) => {
                const transaction = db.transaction("values", "readwrite");
                transaction.objectStore("values").put(write, "spike");
                transaction.oncomplete = resolve;
                transaction.onerror = () => reject(transaction.error);
              });
              indexed = await new Promise((resolve, reject) => {
                const request = db.transaction("values").objectStore("values").get("spike");
                request.onsuccess = () => resolve(request.result === undefined ? null : request.result);
                request.onerror = () => reject(request.error);
              });
              db.close();
            } catch (e) { errors.push("indexedDB: " + e); }
            return JSON.stringify({ origin: location.origin, fetched, imported, localStorage: stored, indexedDB: indexed, errors });
            """

        /// S2(f): the origin forms tried. The scheme with a fixed host is a
        /// tuple origin where everything works; the decision record lists what
        /// the others did.
        @Test(arguments: ["meguri-media://media/", "meguri-media:///", "meguri-media://Media.Local/"])
        func originForm(_ form: String) async throws {
            let handler = SpikeMediaHandler()
            Self.addModuleAndData(handler)
            let origin = try #require(URL(string: form))
            let value = UUID().uuidString
            let result = try await Self.withPage(handler, html: Self.page().replacingOccurrences(of: Spike.origin.absoluteString, with: form), origin: origin) { page in
                try await page.json(OriginResult.self, Self.originScript, arguments: ["write": value])
            }
            print("S2(f) \(form): origin \(result.origin), fetch \(result.fetched), import \(result.imported), localStorage \(result.localStorage == value), IndexedDB \(result.indexedDB == value), errors \(result.errors)")
            if form == Spike.origin.absoluteString {
                #expect(result.origin == "meguri-media://media")
                #expect(result.errors.isEmpty, "\(result.errors)")
                #expect(result.fetched && result.imported)
                #expect(result.localStorage == value && result.indexedDB == value)
            }
        }

        /// S2(d) on the host: values written on the fixed origin into a
        /// persistent store with a fixed identifier are read by a page created
        /// afterwards from a new store object with the same identifier; a store
        /// with another identifier does not see them. Persistence across an app
        /// relaunch is CardSwapTimingSpikeTests' (it needs a new process).
        @Test func aPersistentStoreWithAFixedIdentifierKeepsStorageForLaterPages() async throws {
            let identifier = UUID()
            let other = UUID()
            var failure: (any Error)?
            do {
                let value = UUID().uuidString
                let written = try await Self.withPage(storeHandler(), store: WKWebsiteDataStore(forIdentifier: identifier)) { page in
                    try await page.json(OriginResult.self, Self.originScript, arguments: ["write": value])
                }
                #expect(written.errors.isEmpty, "\(written.errors)")
                let read = try await Self.withPage(storeHandler(), store: WKWebsiteDataStore(forIdentifier: identifier)) { page in
                    try await page.json(OriginResult.self, Self.originScript, arguments: ["write": ""])
                }
                #expect(read.localStorage == value, "localStorage read \(read.localStorage ?? "nil")")
                #expect(read.indexedDB == value, "IndexedDB read \(read.indexedDB ?? "nil")")
                let elsewhere = try await Self.withPage(storeHandler(), store: WKWebsiteDataStore(forIdentifier: other)) { page in
                    try await page.json(OriginResult.self, Self.originScript, arguments: ["write": ""])
                }
                #expect(elsewhere.localStorage == nil && elsewhere.indexedDB == nil)
            } catch {
                failure = error
            }
            // The stores live under ~/Library/WebKit: never leave them behind.
            try? await WKWebsiteDataStore.remove(forIdentifier: identifier)
            try? await WKWebsiteDataStore.remove(forIdentifier: other)
            if let failure { throw failure }
        }

        private func storeHandler() -> SpikeMediaHandler {
            let handler = SpikeMediaHandler()
            Self.addModuleAndData(handler)
            return handler
        }

        // MARK: The handler itself

        @Test func rangeHeadersMapToByteRanges() {
            #expect(SpikeMediaHandler.bounds(of: "bytes=100-199", total: 1000) == .some(100..<200))
            #expect(SpikeMediaHandler.bounds(of: "bytes=900-", total: 1000) == .some(900..<1000))
            #expect(SpikeMediaHandler.bounds(of: "bytes=-100", total: 1000) == .some(900..<1000))
            #expect(SpikeMediaHandler.bounds(of: "bytes=0-1", total: 1000) == .some(0..<2))
            #expect(SpikeMediaHandler.bounds(of: "bytes=990-2000", total: 1000) == .some(990..<1000))
            #expect(SpikeMediaHandler.bounds(of: "bytes=1000-", total: 1000) == .some(nil))
            #expect(SpikeMediaHandler.bounds(of: "items=0-1", total: 1000) == nil)
        }
    }
}
