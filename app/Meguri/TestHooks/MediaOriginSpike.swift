#if MEGURI_TEST_HOOKS
import AVFoundation
import DesignSystem
import Foundation
import SwiftUI
import WebKit

extension TestHooks {
    /// Spike S2's in-app checks (U65): what needs a real app run, a relaunch
    /// or a simulator. `spike/storage/write` and `spike/storage/read` keep a
    /// value in localStorage and IndexedDB on the media origin across a
    /// relaunch (S2(d), R502); `spike/media-seek` seeks a muted video through
    /// Range requests (S2(a)), and `spike/media-seek/audio` an 8 MB silent
    /// MP3, for a device or a Mac's own hardware: in the test VM no
    /// `<audio>` plays, from any source, and an audio element left loading
    /// there stalled the app's main thread for over a minute, so the test
    /// skips the audio route there. The macOS host checks, and the throwaway
    /// handler they share with this one, are CustomSchemeMediaSpikeTests'.
    /// Throwaway: U67's media handler and configuration replace it.
    enum MediaOriginSpike {
        static let scheme = "meguri-media"
        static let origin = URL(string: "meguri-media://media/")!
        /// The spike's persistent store; never the production one (R502).
        static let storeIdentifier = UUID(uuidString: "6D656775-7269-5350-494B-453230303032")!

        static func configuration(_ handler: Handler, persistent: Bool) -> WKWebViewConfiguration {
            let configuration = WKWebViewConfiguration()
            configuration.setURLSchemeHandler(handler, forURLScheme: scheme)
            configuration.websiteDataStore = persistent ? WKWebsiteDataStore(forIdentifier: storeIdentifier) : .nonPersistent()
            configuration.mediaTypesRequiringUserActionForPlayback = []
            #if !os(macOS)
            configuration.allowsInlineMediaPlayback = true
            #endif
            return configuration
        }

        /// A constant-bitrate MP3 of silence at least `minimumBytes` long
        /// (MPEG-1 Layer III, 128 kbit/s, 44.1 kHz, mono, no tag), made at run
        /// time like CustomSchemeMediaSpikeTests' own: each 417-byte frame is
        /// a header and zeros, which decode to silence, so playing it makes
        /// no sound.
        nonisolated static func silentMP3(minimumBytes: Int) -> Data {
            let frameLength = 417
            var frame = [UInt8](repeating: 0, count: frameLength)
            frame.replaceSubrange(0..<4, with: [0xFF, 0xFB, 0x90, 0xC4])
            let frames = (minimumBytes + frameLength - 1) / frameLength
            var data = Data(capacity: frames * frameLength)
            for _ in 0..<frames { data.append(contentsOf: frame) }
            return data
        }

        /// An H.264 MP4 of 20 s at 10 frames a second, fast start, made at
        /// run time (CustomSchemeMediaSpikeTests makes the same one).
        @concurrent nonisolated static func mp4() async throws -> Data {
            let url = FileManager.default.temporaryDirectory.appending(path: "spike-\(UUID().uuidString).mp4")
            defer { try? FileManager.default.removeItem(at: url) }
            let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
            writer.shouldOptimizeForNetworkUse = true
            let input = AVAssetWriterInput(
                mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 320, AVVideoHeightKey: 240])
            let attributes = CVPixelBufferCreationAttributes(
                pixelFormatType: CVPixelFormatType(rawValue: kCVPixelFormatType_32BGRA), size: CVImageSize(width: 320, height: 240))
            let receiver = writer.inputPixelBufferReceiver(for: input, pixelBufferAttributes: attributes)
            try writer.start()
            writer.startSession(atSourceTime: .zero)
            for index in 0..<200 {
                let frame = try CVMutablePixelBuffer(attributes)
                let grey = Int32(truncatingIfNeeded: 40 + (index / 10) * 9)
                frame.withUnsafeBuffer { buffer in
                    CVPixelBufferLockBaseAddress(buffer, [])
                    memset(CVPixelBufferGetBaseAddress(buffer), grey, CVPixelBufferGetDataSize(buffer))
                    CVPixelBufferUnlockBaseAddress(buffer, [])
                }
                // `append` sometimes never resumed on the macOS host: poll.
                let ready = CVReadOnlyPixelBuffer(frame)
                let deadline = ContinuousClock.now + .seconds(10)
                while try !receiver.appendImmediately(ready, with: CMTime(value: CMTimeValue(index), timescale: 10)) {
                    guard ContinuousClock.now < deadline else { throw URLError(.timedOut) }
                    try await Task.sleep(for: .milliseconds(2))
                }
            }
            receiver.finish()
            await writer.finishWriting()
            guard writer.status == .completed else { throw URLError(.cannotCreateFile) }
            return try Data(contentsOf: url)
        }

        /// Serves in-memory files at the origin's root, answers one `Range`
        /// with 206, and logs what it answered.
        final class Handler: NSObject, WKURLSchemeHandler {
            struct Answer {
                let path: String
                /// The request's `Range` header.
                let requested: String?
                let status: Int
                let first: Int?
                let contentRange: String?
                let length: Int
                /// What a ranged request got back (`rangeProblems` compares it
                /// with the file).
                let body: Data?
            }

            var files: [String: (data: Data, type: String)] = [:]
            private(set) var answers: [Answer] = []

            func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
                guard let url = task.request.url else { return task.didFailWithError(URLError(.badURL)) }
                let path = url.path(percentEncoded: false)
                let requested = task.request.value(forHTTPHeaderField: "Range")
                guard let file = files[path] else {
                    return finish(task, url, path, requested, status: 404, first: nil, headers: [:], body: Data())
                }
                var headers = ["Content-Type": file.type, "Accept-Ranges": "bytes", "Cache-Control": "no-store"]
                let total = file.data.count
                if let range = requested, let bounds = Self.bounds(range, total: total) {
                    headers["Content-Range"] = "bytes \(bounds.first)-\(bounds.last)/\(total)"
                    return finish(
                        task, url, path, requested, status: 206, first: bounds.first, headers: headers,
                        body: file.data.subdata(in: bounds.first..<(bounds.last + 1)))
                }
                finish(task, url, path, requested, status: 200, first: nil, headers: headers, body: file.data)
            }

            func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {}

            /// The first and last byte a single-range `Range` header asks for
            /// (`bytes=a-b`, `bytes=a-`, or the suffix `bytes=-n`), or nil.
            static func bounds(_ header: String, total: Int) -> (first: Int, last: Int)? {
                guard let match = header.wholeMatch(of: /bytes=(\d*)-(\d*)/), total > 0 else { return nil }
                if match.1.isEmpty {
                    guard let suffix = Int(match.2), suffix > 0 else { return nil }
                    return (max(0, total - suffix), total - 1)
                }
                guard let first = Int(match.1), first < total else { return nil }
                let last = min(Int(match.2) ?? (total - 1), total - 1)
                return last >= first ? (first, last) : nil
            }

            /// What is wrong with the answers to ranged requests for `path`
            /// (R495): each must be a 206 whose `Content-Range` names the
            /// first byte asked for, the last one asked for (or the file's
            /// last), and the file's length, with exactly those bytes of the
            /// file. The expected range comes from `bounds`, which
            /// MediaOriginSpikeHandlerTests holds to fixed answers.
            func rangeProblems(_ path: String) -> [String] {
                guard let file = files[path]?.data else { return ["\(path): no such file"] }
                let total = file.count
                return answers.filter { $0.path == path && $0.requested != nil }.compactMap { answer in
                    let requested = answer.requested ?? ""
                    guard let bounds = Self.bounds(requested, total: total) else {
                        return "\(requested): not a satisfiable single range"
                    }
                    let expected = "bytes \(bounds.first)-\(bounds.last)/\(total)"
                    guard answer.status == 206, answer.contentRange == expected, answer.length == bounds.last - bounds.first + 1 else {
                        return "\(requested): \(answer.status) \(answer.contentRange ?? "no Content-Range") with \(answer.length) bytes, not 206 \(expected)"
                    }
                    guard answer.body == file.subdata(in: bounds.first..<(bounds.last + 1)) else {
                        return "\(requested): 206 \(expected), but not the file's bytes \(bounds.first)-\(bounds.last)"
                    }
                    return nil
                }
            }

            private func finish(
                _ task: any WKURLSchemeTask, _ url: URL, _ path: String, _ requested: String?, status: Int, first: Int?,
                headers: [String: String], body: Data
            ) {
                var headers = headers
                headers["Content-Length"] = String(body.count)
                answers.append(
                    Answer(
                        path: path, requested: requested, status: status, first: first, contentRange: headers["Content-Range"], length: body.count,
                        body: requested == nil ? nil : body))
                task.didReceive(HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!)
                task.didReceive(body)
                task.didFinish()
            }
        }

        /// A web view that reports when its page finished loading.
        final class Page: NSObject, WKNavigationDelegate {
            let webView: WKWebView
            private var loaded: CheckedContinuation<Bool, Never>?

            init(_ configuration: WKWebViewConfiguration) {
                webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 240), configuration: configuration)
                super.init()
                webView.navigationDelegate = self
            }

            func open(_ path: String) async -> Bool {
                await withCheckedContinuation { continuation in
                    loaded = continuation
                    webView.load(URLRequest(url: MediaOriginSpike.origin.appending(path: path)))
                }
            }

            func run(_ script: String, _ arguments: [String: Any] = [:]) async -> String {
                do {
                    return try await webView.callAsyncJavaScript(script, arguments: arguments, contentWorld: .page) as? String ?? "no result"
                } catch {
                    return "error: \(error.localizedDescription)"
                }
            }

            /// `run`, or "timed out" after `seconds`: a web content process
            /// stuck on media (no audio route in the test VM) never answers.
            func run(_ script: String, _ arguments: [String: Any] = [:], within seconds: Int) async -> String {
                let once = Once()
                return await withCheckedContinuation { continuation in
                    once.continuation = continuation
                    Task { once.finish(await self.run(script, arguments)) }
                    Task {
                        try? await Task.sleep(for: .seconds(seconds))
                        once.finish("{\"error\":\"timed out after \(seconds) s\"}")
                    }
                }
            }

            private func done(_ ok: Bool) {
                loaded?.resume(returning: ok)
                loaded = nil
            }

            /// Resumed by whichever answer comes first.
            private final class Once {
                var continuation: CheckedContinuation<String, Never>?

                func finish(_ value: String) {
                    continuation?.resume(returning: value)
                    continuation = nil
                }
            }

            func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { done(true) }
            func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) { done(false) }
            func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) { done(false) }
        }
    }

    /// Shows a spike page on screen: a page outside any window may not
    /// load media.
    #if os(macOS)
    struct PageHost: NSViewRepresentable {
        let webView: WKWebView
        func makeNSView(context: Context) -> WKWebView { webView }
        func updateNSView(_ view: WKWebView, context: Context) {}
    }
    #else
    struct PageHost: UIViewRepresentable {
        let webView: WKWebView
        func makeUIView(context: Context) -> WKWebView { webView }
        func updateUIView(_ view: WKWebView, context: Context) {}
    }
    #endif

    /// S2(d): writes (or reads back) a value in localStorage and IndexedDB on
    /// the media origin, in the spike's persistent store, and shows
    /// `written <value>` or `read <localStorage value> <IndexedDB value>`.
    struct StorageSpikeView: View {
        let write: Bool
        @State private var result = "running"
        @State private var page: MediaOriginSpike.Page?

        static let script = """
            const open = () => new Promise((resolve, reject) => {
              const request = indexedDB.open("spike", 1);
              request.onupgradeneeded = () => request.result.createObjectStore("values");
              request.onsuccess = () => resolve(request.result);
              request.onerror = () => reject(request.error);
            });
            try {
              const db = await open();
              if (value) {
                localStorage.setItem("spike", value);
                await new Promise((resolve, reject) => {
                  const transaction = db.transaction("values", "readwrite");
                  transaction.objectStore("values").put(value, "spike");
                  transaction.oncomplete = resolve;
                  transaction.onerror = () => reject(transaction.error);
                });
                return "written " + value;
              }
              const indexed = await new Promise((resolve, reject) => {
                const request = db.transaction("values").objectStore("values").get("spike");
                request.onsuccess = () => resolve(request.result);
                request.onerror = () => reject(request.error);
              });
              return "read " + localStorage.getItem("spike") + " " + indexed;
            } catch (error) {
              return "error " + error;
            }
            """

        @State private var handler = MediaOriginSpike.Handler()

        var body: some View {
            VStack(spacing: Spacing.small) {
                Text(result)
                    .accessibilityIdentifier("spike.storage.result")
                if let page {
                    PageHost(webView: page.webView)
                }
            }
            .padding()
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onAppear {
                if page == nil {
                    handler.files["/_anki/storage.html"] = (Data("<!doctype html><title>storage</title>".utf8), "text/html")
                    page = MediaOriginSpike.Page(MediaOriginSpike.configuration(handler, persistent: true))
                }
            }
            .task(id: page == nil) {
                guard let page else { return }
                guard await page.open("_anki/storage.html") else {
                    result = "error: the page did not load"
                    return
                }
                let outcome = await page.run(Self.script, ["value": write ? UUID().uuidString : ""])
                if write {
                    // localStorage reaches disk on WebKit's own schedule:
                    // give it time before the test relaunches the app.
                    try? await Task.sleep(for: .seconds(3))
                }
                result = outcome
            }
        }
    }

    /// S2(a) in the app: a muted H.264 video (or, with `audio`, an 8 MB
    /// silent MP3) on the media origin seeks to 90 % and plays on; the result
    /// names the ranged answers the handler gave and what is wrong with any
    /// of them.
    struct MediaSeekSpikeView: View {
        var audio = false
        @State private var result = "running"
        @State private var page: MediaOriginSpike.Page?

        /// Plays, seeks to 90 % and plays on, logging every media event,
        /// so a stall says where it stopped.
        static let script = """
            const media = document.getElementById(id);
            const log = [];
            const started = performance.now();
            for (const name of ["loadedmetadata", "canplay", "playing", "seeking", "seeked", "waiting", "stalled", "suspend", "error"]) {
              media.addEventListener(name, () => log.push(name + "@" + Math.round(performance.now() - started) + ":" + media.currentTime.toFixed(1)));
            }
            const event = (name, seconds) => new Promise((resolve, reject) => {
              media.addEventListener(name, resolve, { once: true });
              media.addEventListener("error", () => reject(new Error("media error " + (media.error && media.error.code))), { once: true });
              setTimeout(() => reject(new Error("no " + name + " within " + seconds + " s, readyState " + media.readyState + ", networkState " + media.networkState)), seconds * 1000);
            });
            const progress = async (from, seconds) => {
              const until = performance.now() + seconds * 1000;
              while (media.currentTime < from + 0.4 && performance.now() < until) await new Promise((resolve) => setTimeout(resolve, 50));
              return media.currentTime;
            };
            try {
              if (media.readyState < 1) await event("loadedmetadata", 10);
              const played = await Promise.race([
                media.play().then(() => "playing", (error) => "refused: " + error),
                new Promise((resolve) => setTimeout(() => resolve("play() pending after 5 s"), 5000)),
              ]);
              log.push("play: " + played);
              const playedFromStart = await progress(0, 5);
              media.currentTime = media.duration * 0.9;
              await event("seeked", 15);
              const seekedTo = media.currentTime;
              const playedTo = await progress(seekedTo, 5);
              return JSON.stringify({ duration: media.duration, playedFromStart, seekedTo, playedTo, log });
            } catch (error) {
              return JSON.stringify({ error: String(error), log });
            }
            """

        @State private var handler = MediaOriginSpike.Handler()

        /// What the view reports: the script's result and the ranged answers
        /// the handler gave (all of them, and those in the last fifth).
        struct Outcome: Codable {
            var video = ""
            var videoRanged = 0
            var videoLate = 0
            var audio = ""
            var audioRanged = 0
            var audioLate = 0
            /// Ranged answers whose `Content-Range` or length is not the one
            /// asked for.
            var badRanges: [String] = []
        }

        var body: some View {
            VStack(spacing: Spacing.small) {
                Text(result)
                    .accessibilityIdentifier("spike.media.result")
                if let page {
                    PageHost(webView: page.webView)
                }
            }
            .padding()
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onAppear {
                if page == nil {
                    handler.files["/_anki/media.html"] = (
                        Data(#"<!doctype html><base href="meguri-media://media/"><body></body>"#.utf8), "text/html"
                    )
                    page = MediaOriginSpike.Page(MediaOriginSpike.configuration(handler, persistent: false))
                }
            }
            .task(id: page == nil) {
                guard let page else { return }
                result = await run(page)
            }
        }

        private func run(_ page: MediaOriginSpike.Page) async -> String {
            var outcome = Outcome()
            guard await page.open("_anki/media.html") else { return "error: the page did not load" }
            func ranged(_ path: String, _ total: Int) -> (all: Int, late: Int) {
                let answers = handler.answers.filter { $0.path == path && $0.status == 206 }
                return (answers.count, answers.filter { ($0.first ?? 0) >= total * 8 / 10 }.count)
            }
            if audio {
                let mp3 = MediaOriginSpike.silentMP3(minimumBytes: 8 * 1024 * 1024)
                handler.files["/_long.mp3"] = (mp3, "audio/mpeg")
                result = "running: the audio"
                outcome.audio = await page.run(Self.script, ["id": await page.run(Self.audioElement)], within: 50)
                (outcome.audioRanged, outcome.audioLate) = ranged("/_long.mp3", mp3.count)
                outcome.badRanges = handler.rangeProblems("/_long.mp3")
            } else {
                result = "running: encoding the video"
                if let mp4 = try? await MediaOriginSpike.mp4() {
                    handler.files["/_clip.mp4"] = (mp4, "video/mp4")
                    result = "running: the video"
                    outcome.video = await page.run(Self.script, ["id": await page.run(Self.video)], within: 50)
                    (outcome.videoRanged, outcome.videoLate) = ranged("/_clip.mp4", mp4.count)
                    outcome.badRanges = handler.rangeProblems("/_clip.mp4")
                } else {
                    outcome.video = "no MP4 encoder"
                }
            }
            let encoder = JSONEncoder()
            encoder.outputFormatting = .sortedKeys
            return (try? encoder.encode(outcome)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        }

        static let audioElement = """
            const audio = document.createElement("audio");
            audio.id = "audio";
            audio.preload = "auto";
            audio.src = "_long.mp3";
            document.body.appendChild(audio);
            return audio.id;
            """

        static let video = """
            const video = document.createElement("video");
            video.id = "video";
            video.muted = true;
            video.playsInline = true;
            video.preload = "auto";
            video.width = 160;
            video.src = "_clip.mp4";
            document.body.appendChild(video);
            return video.id;
            """
    }
}
#endif
