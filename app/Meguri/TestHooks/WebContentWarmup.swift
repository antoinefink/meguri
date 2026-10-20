#if MEGURI_TEST_HOOKS
import CardRendering
import Foundation
import WebKit
#if !os(macOS)
import notify
#endif

extension TestHooks {
    /// The warmed web content process R1866 asks of performance tests (U12).
    /// On the notification `<bundle id>.testhooks.warmWebContent` (Darwin on
    /// iOS, distributed on the Mac) the app loads a page into a web view it
    /// keeps for the rest of its run, waits until the page has loaded and its
    /// web content process has answered a script, then posts
    /// `<bundle id>.testhooks.webContentReady`. Later requests check that the
    /// same process still answers; one that terminated is replaced. Without
    /// a ready process the app posts nothing, and `PerfTestCase` fails before
    /// measuring.
    ///
    /// Uses the production reviewer document, origin and shared configuration.
    /// A real round trip proves this page is warm; WebKit does not guarantee
    /// that a later reviewer page will share its process.
    final class WebContentWarmup: NSObject {
        static let shared = WebContentWarmup()

        static var requestName: String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.warmWebContent" }
        static var readyName: String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.webContentReady" }

        private(set) var page: CardPage?
        private var current: Task<Bool, Never>?
        private var flight = UUID()
        private let configuration: CardWebConfiguration
        init(configuration: CardWebConfiguration = .shared) { self.configuration = configuration; super.init() }

        private var listening = false
        #if os(macOS)
        private var observer: NSObjectProtocol?
        #else
        private var token: Int32 = NOTIFY_TOKEN_INVALID
        #endif

        /// Answers warm-up requests from now on; a second call does nothing.
        func listen() {
            guard !listening else { return }
            listening = true
            #if os(macOS)
            observer = DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name(Self.requestName), object: nil, queue: .main
            ) { [weak self] _ in
                Task { @MainActor [weak self] in self?.requested() }
            }
            #else
            notify_register_dispatch(Self.requestName, &token, .main) { [weak self] _ in
                Task { @MainActor [weak self] in self?.requested() }
            }
            #endif
        }

        private func requested() {
            Task {
                guard await warm() else { return }
                #if os(macOS)
                // Delivered at once: the test runner is usually not active.
                DistributedNotificationCenter.default().postNotificationName(
                    Notification.Name(Self.readyName), object: nil, userInfo: nil, deliverImmediately: true)
                #else
                notify_post(Self.readyName)
                #endif
            }
        }

        /// Whether a web content process is loaded and answering, after
        /// starting one if needed. Concurrent requests share one warm-up.
        func warm() async -> Bool {
            if let current {
                return await current.value
            }
            let flight = UUID()
            self.flight = flight
            let task = Task { await warmOnce(flight) }
            current = task
            let ready = await task.value
            if self.flight == flight { current = nil }
            return ready
        }

        private func warmOnce(_ flight: UUID) async -> Bool {
            guard self.flight == flight, !Task.isCancelled else { return false }
            if let page, await page.answersWarmup(), self.page === page, self.flight == flight, !Task.isCancelled { return true }
            guard self.flight == flight, !Task.isCancelled else { return false }
            discardPage()
            let page = configuration.makeWarmupPage()
            self.page = page
            page.onTermination = { [weak self] page in
                guard let self, self.page === page else { return }
                self.discardPage()
            }
            do { try await page.load() } catch {
                if self.page === page { discardPage() }
                return false
            }
            guard self.flight == flight, !Task.isCancelled, await page.answersWarmup(), self.page === page else {
                if self.page === page { discardPage() }
                return false
            }
            return true
        }

        private func discardPage() {
            let old = page
            page = nil
            old?.close()
        }

        func close() {
            flight = UUID()
            current?.cancel(); current = nil
            discardPage()
        }

    }
}
#endif
