#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import Instrumentation
import SwiftUI
#if !os(macOS)
import notify
#endif

extension TestHooks {
    /// Routes for the performance harness (U12). `perf/probe` emits the
    /// registered interval `probe.syntheticDelay` around a fixed delay, so
    /// the UI performance tests can check that a registered interval reaches
    /// XCTest's signpost metric and the perf result with the value it had.
    enum PerfRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("perf/probe") { _ in
                .screen {
                    PerfProbe()
                }
            }
        }
    }

    /// Each run of the probe begins `probe.syntheticDelay`, waits
    /// `PerfProbe.delay`, then ends it as the run count changes. A run
    /// starts on a tap of "Run probe", or on the notification
    /// `<bundle id>.testhooks.perfProbe` (Darwin on iOS, distributed on the
    /// Mac), which the performance tests post: they need no synthesized
    /// input, which macOS drops on this toolchain (app/AGENTS.md).
    struct PerfProbe: View {
        /// The synthetic delay the smoke performance test expects to measure.
        static let delay = Duration.milliseconds(30)

        @State private var runs = 0
        @State private var running = false
        @State private var trigger = PerfProbeTrigger()

        var body: some View {
            VStack(spacing: Spacing.medium) {
                Button("Run probe") {
                    run()
                }
                .disabled(running)
                .accessibilityIdentifier("perf.probe.run")
                Text("Runs: \(runs)")
                    .accessibilityIdentifier("perf.probe.runs")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onAppear {
                trigger.start { run() }
            }
            .onDisappear {
                trigger.stop()
            }
        }

        private func run() {
            guard !running else { return }
            running = true
            let token = Signposts.begin(IntervalNames.probeSyntheticDelay)
            Task {
                try? await Task.sleep(for: Self.delay)
                runs += 1
                running = false
                Signposts.end(token)
            }
        }
    }

    /// Listens for `<bundle id>.testhooks.perfProbe`.
    final class PerfProbeTrigger {
        private var action: (() -> Void)?
        #if os(macOS)
        private var observer: NSObjectProtocol?
        #else
        private var token: Int32 = NOTIFY_TOKEN_INVALID
        #endif

        static var name: String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.perfProbe" }

        func start(_ action: @escaping () -> Void) {
            stop()
            self.action = action
            #if os(macOS)
            observer = DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name(Self.name), object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.action?() }
            }
            #else
            notify_register_dispatch(Self.name, &token, .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.action?() }
            }
            #endif
        }

        func stop() {
            #if os(macOS)
            if let observer {
                DistributedNotificationCenter.default().removeObserver(observer)
            }
            observer = nil
            #else
            if token != NOTIFY_TOKEN_INVALID {
                notify_cancel(token)
            }
            token = NOTIFY_TOKEN_INVALID
            #endif
            action = nil
        }
    }
}
#endif
