#if MEGURI_TEST_HOOKS
import AVFoundation
import DesignSystem
import CardRendering
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
    /// skips the audio route there. This measurement proxy uses U67's
    /// production media handler, fixed origin and website data store.
    enum MediaOriginSpike {
        static let scheme = "meguri-media"
        static let origin = URL(string: "meguri-media://media/")!
        // Fresh per test, passed unchanged across the two app launches.
        static var storeIdentifier: UUID? { UserDefaults.standard.string(forKey: "CardRenderingTestStore").flatMap(UUID.init(uuidString:)) }

        static func configuration(_ handler: Handler, persistent: Bool) -> WKWebViewConfiguration {
            let engine = CardWebConfiguration(store: persistent ? storeIdentifier.map { WKWebsiteDataStore(forIdentifier: $0) } ?? .nonPersistent() : .nonPersistent())
            handler.engine = engine
            // Measurement proxy only: all responses come from production.
            let configuration = engine.configuration.copy() as! WKWebViewConfiguration
            // The copy retains production's registration; replace it only here.
            configuration.setURLSchemeHandler(nil, forURLScheme: scheme)
            configuration.setURLSchemeHandler(handler, forURLScheme: scheme)
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

        /// Proxies the production handler with temporary fixture files and
        /// a supplied root document, logging its actual responses.
        final class Handler: NSObject, WKURLSchemeHandler {
            struct Answer {
                let path: String
                let requested: String?
                let status: Int
                let first: Int?
                let contentRange: String?
                let length: Int
                let body: Data?
            }
            var engine = CardWebConfiguration(store: .nonPersistent())
            let root = FileManager.default.temporaryDirectory.appending(path: "meguri-s2-\(UUID())", directoryHint: .isDirectory)
            private var resolver: MediaPathResolver?
            var files: [String: (data: Data, type: String)] = [:] {
                didSet {
                    for (path, file) in files where !path.hasPrefix("/_anki/") {
                        precondition(!path.dropFirst().contains("/"))
                        try! file.data.write(to: root.appending(path: String(path.dropFirst())))
                    }
                }
            }
            private(set) var answers: [Answer] = []
            private var tasks: [ObjectIdentifier: Proxy] = [:]
            override init() {
                super.init()
                try! FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            }
            isolated deinit { engine.media.close(); try? FileManager.default.removeItem(at: root) }
            func bind(_ page: WKWebView, document: String) async throws {
                try Task.checkCancellation()
                if resolver == nil {
                    let opened = try await MediaPathResolver.open(root: root, trackedNames: [])
                    try Task.checkCancellation()
                    resolver = opened
                }
                try await engine.ready()
                try Task.checkCancellation()
                engine.media.bind(page, to: .init(resolver: resolver!, document: document, diagnostics: CardDiagnostics()))
            }
            func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
                let proxy = Proxy(task, owner: self)
                tasks[ObjectIdentifier(task)] = proxy
                engine.media.webView(webView, start: proxy)
            }
            func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) {
                if let proxy = tasks.removeValue(forKey: ObjectIdentifier(task)) { engine.media.webView(webView, stop: proxy) }
            }
            // Expectation helper, not a server. Fixed independent byte cases
            // in MediaOriginSpikeHandlerTests hold it to the contract.
            static func bounds(_ header: String, total: Int) -> (first: Int, last: Int)? {
                let result = RangeResponder(total: UInt64(total), header: header)
                guard result.status == 206 else { return nil }
                return (Int(result.offset), Int(result.offset + result.count - 1))
            }
            func rangeProblems(_ path: String) -> [String] {
                guard let file = files[path]?.data else { return ["\(path): no such file"] }
                return answers.filter { $0.path == path && $0.requested != nil }.compactMap { answer in
                    guard let bounds = Self.bounds(answer.requested ?? "", total: file.count) else { return "not a single satisfiable range" }
                    let expected = "bytes \(bounds.first)-\(bounds.last)/\(file.count)"
                    guard answer.status == 206, answer.contentRange == expected, answer.length == bounds.last - bounds.first + 1,
                          answer.body == file.subdata(in: bounds.first..<(bounds.last + 1)) else { return "incorrect ranged body/headers: \(answer.requested ?? "")" }
                    return nil
                }
            }
            private final class Proxy: NSObject, WKURLSchemeTask {
                let target: any WKURLSchemeTask
                weak var owner: Handler?
                var request: URLRequest { target.request }
                var response: HTTPURLResponse?
                var bytes = Data()
                init(_ target: any WKURLSchemeTask, owner: Handler) { self.target = target; self.owner = owner }
                func didReceive(_ response: URLResponse) { self.response = response as? HTTPURLResponse; target.didReceive(response) }
                func didReceive(_ data: Data) { bytes.append(data); target.didReceive(data) }
                func didFinish() {
                    let range = request.value(forHTTPHeaderField: "Range")
                    let contentRange = response?.value(forHTTPHeaderField: "Content-Range")
                    let first = contentRange.flatMap { Int($0.wholeMatch(of: /bytes (\d+)-.*/)?.1 ?? "") }
                    owner?.answers.append(Answer(path: request.url?.path(percentEncoded: false) ?? "", requested: range,
                                                  status: response?.statusCode ?? 0, first: first, contentRange: contentRange,
                                                  length: bytes.count, body: range == nil ? nil : bytes))
                    owner?.tasks[ObjectIdentifier(target)] = nil
                    target.didFinish()
                }
                func didFailWithError(_ error: any Error) { owner?.tasks[ObjectIdentifier(target)] = nil; target.didFailWithError(error) }
            }
        }

        /// Owns one bounded bind/navigation attempt at a time.
        final class Page: NSObject, WKNavigationDelegate {
            let webView: WKWebView
            let handler: Handler
            enum OpenStage: String, Sendable { case binding, navigation }
            enum OpenError: LocalizedError {
                case alreadyOpening, navigationDidNotStart, timedOut(OpenStage), contentProcessTerminated

                nonisolated var errorDescription: String? {
                    switch self {
                    case .alreadyOpening: "a page open is already in progress"
                    case .navigationDidNotStart: "navigation did not start"
                    case .timedOut(let stage): "page \(stage.rawValue) timed out"
                    case .contentProcessTerminated: "the web content process terminated"
                    }
                }
            }
            private final class OpenAttempt {
                let id: UUID
                let expires: ContinuousClock.Instant
                var stage = OpenStage.binding
                var navigation: WKNavigation?
                var continuation: CheckedContinuation<Void, any Error>?
                var binding: Task<Void, Never>?
                var deadline: Task<Void, Never>?

                init(id: UUID, expires: ContinuousClock.Instant, continuation: CheckedContinuation<Void, any Error>) {
                    self.id = id
                    self.expires = expires
                    self.continuation = continuation
                }
            }
            private var opening: OpenAttempt?

            init(_ configuration: WKWebViewConfiguration, handler: Handler) {
                self.handler = handler
                webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 240), configuration: configuration)
                super.init()
                webView.navigationDelegate = self
            }

            func open(
                _ path: String, within limit: Duration = .seconds(10),
                stageChanged: @escaping (OpenStage) -> Void = { _ in }
            ) async throws {
                let id = UUID()
                try await withTaskCancellationHandler {
                    try Task.checkCancellation()
                    guard opening == nil else { throw OpenError.alreadyOpening }
                    let expires = ContinuousClock.now + limit
                    let text = handler.files["/" + path].map { String(decoding: $0.data, as: UTF8.self) } ?? "<!doctype html>"
                    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                        let attempt = OpenAttempt(id: id, expires: expires, continuation: continuation)
                        opening = attempt
                        stageChanged(.binding)
                        attempt.deadline = Task {
                            do { try await Task.sleep(until: expires, clock: .continuous) } catch { return }
                            self.finish(attempt, with: .failure(OpenError.timedOut(attempt.stage)))
                        }
                        attempt.binding = Task { [handler, webView] in
                            do {
                                try self.check(attempt)
                                try await handler.bind(webView, document: text)
                                try self.check(attempt)
                                attempt.stage = .navigation
                                attempt.navigation = webView.load(URLRequest(url: MediaOriginSpike.origin))
                                guard attempt.navigation != nil else {
                                    self.finish(attempt, with: .failure(OpenError.navigationDidNotStart))
                                    return
                                }
                                stageChanged(.navigation)
                            } catch {
                                self.finish(attempt, with: .failure(error))
                            }
                        }
                    }
                    try Task.checkCancellation()
                } onCancel: {
                    Task { @MainActor in
                        guard let attempt = self.opening, attempt.id == id else { return }
                        self.finish(attempt, with: .failure(CancellationError()))
                    }
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
                return await withTaskCancellationHandler {
                    guard !Task.isCancelled else { return "error: script cancelled" }
                    return await withCheckedContinuation { continuation in
                        once.continuation = continuation
                        once.script = Task { once.finish(await self.run(script, arguments)) }
                        once.deadline = Task {
                            do { try await Task.sleep(for: .seconds(seconds)) } catch { return }
                            once.finish("{\"error\":\"timed out after \(seconds) s\"}")
                        }
                    }
                } onCancel: {
                    Task { @MainActor in once.finish("error: script cancelled") }
                }
            }

            private func check(_ attempt: OpenAttempt) throws {
                try Task.checkCancellation()
                guard opening === attempt else { throw CancellationError() }
                guard ContinuousClock.now < attempt.expires else { throw OpenError.timedOut(attempt.stage) }
            }

            private func finish(_ attempt: OpenAttempt, with result: Result<Void, any Error>) {
                guard opening === attempt else { return }
                // Clear ownership before stopLoading can deliver an old callback.
                opening = nil
                attempt.binding?.cancel()
                attempt.deadline?.cancel()
                attempt.binding = nil
                attempt.deadline = nil
                if case .failure = result, attempt.navigation != nil { webView.stopLoading() }
                let continuation = attempt.continuation
                attempt.continuation = nil
                continuation?.resume(with: result)
            }

            private func done(_ navigation: WKNavigation?, with result: Result<Void, any Error>) {
                guard let attempt = opening, let navigation, attempt.navigation === navigation else { return }
                if ContinuousClock.now >= attempt.expires {
                    finish(attempt, with: .failure(OpenError.timedOut(attempt.stage)))
                } else {
                    finish(attempt, with: result)
                }
            }

            /// Resumed by whichever answer comes first.
            private final class Once {
                var continuation: CheckedContinuation<String, Never>?
                var script: Task<Void, Never>?
                var deadline: Task<Void, Never>?

                func finish(_ value: String) {
                    guard let continuation else { return }
                    self.continuation = nil
                    script?.cancel()
                    deadline?.cancel()
                    script = nil
                    deadline = nil
                    continuation.resume(returning: value)
                }
            }

            func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { done(navigation, with: .success(())) }
            func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) { done(navigation, with: .failure(error)) }
            func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) { done(navigation, with: .failure(error)) }
            func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
                guard let attempt = opening else { return }
                finish(attempt, with: .failure(OpenError.contentProcessTerminated))
            }
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
        @State private var attempted = false

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
            .task {
                guard !attempted, !Task.isCancelled else { return }
                attempted = true
                if UserDefaults.standard.bool(forKey: "CardRenderingTestCleanup") {
                    guard let identifier = MediaOriginSpike.storeIdentifier else {
                        result = "error: no store to clean up"
                        return
                    }
                    do { try await WKWebsiteDataStore.remove(forIdentifier: identifier); result = "removed" }
                    catch { result = "error: store cleanup" }
                    return
                }
                handler.files["/_anki/storage.html"] = (Data("<!doctype html><title>storage</title>".utf8), "text/html")
                let page = MediaOriginSpike.Page(MediaOriginSpike.configuration(handler, persistent: true), handler: handler)
                self.page = page
                do {
                    try await page.open("_anki/storage.html")
                } catch {
                    result = "error: \(error.localizedDescription)"
                    return
                }
                guard !Task.isCancelled else { return }
                // Binding/navigation (10 s), script (10 s) and the flush
                // delay (3 s) fit inside the existing 30 s UI observation.
                let outcome = await page.run(Self.script, ["value": write ? UUID().uuidString : ""], within: 10)
                if write {
                    // localStorage reaches disk on WebKit's own schedule:
                    // give it time before the test relaunches the app.
                    do { try await Task.sleep(for: .seconds(3)) } catch { return }
                }
                guard !Task.isCancelled else { return }
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
        @State private var attempted = false

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
            .task {
                guard !attempted, !Task.isCancelled else { return }
                attempted = true
                handler.files["/_anki/media.html"] = (
                    Data(#"<!doctype html><base href="meguri-media://media/"><body></body>"#.utf8), "text/html"
                )
                let page = MediaOriginSpike.Page(MediaOriginSpike.configuration(handler, persistent: false), handler: handler)
                self.page = page
                result = await run(page)
            }
        }

        private func run(_ page: MediaOriginSpike.Page) async -> String {
            var outcome = Outcome()
            do {
                try await page.open("_anki/media.html") { result = "running: \($0.rawValue)" }
            } catch {
                return "error: \(error.localizedDescription)"
            }
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
