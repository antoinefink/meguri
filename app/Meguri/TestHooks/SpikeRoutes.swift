#if MEGURI_TEST_HOOKS
import AppModel
import DesignSystem
import SwiftUI

extension TestHooks {
    /// Routes for the rendering spikes S1 and S2 (U65), which
    /// `MeguriUITests/CardSwapTimingSpikeTests` drives. Throwaway: the review
    /// screen's own routes replace them once U67 and U176 exist.
    ///
    /// - `spike/card-swap`: the page pair and the baseline, swapped by
    ///   inputs (S1(a), (b), (f)); `/capture` also checks the screen at each
    ///   swap's frame (S1(c), iOS); `/typein` focuses `#typeans` after each
    ///   swap (S1(e)).
    /// - `spike/storage/write`, `spike/storage/read`: storage on the media
    ///   origin across a relaunch (S2(d)).
    /// - `spike/media-seek`: a muted video seeks through Range requests
    ///   (S2(a)); `/audio`: an 8 MB silent MP3 does, on a device or a Mac's
    ///   own hardware (no `<audio>` plays in the test VM).
    enum SpikeRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.register("spike/card-swap") { _ in .screen { CardSwapSpikeView(mode: .timing) } }
            registry.register("spike/card-swap/capture") { _ in .screen { CardSwapSpikeView(mode: .capture) } }
            registry.register("spike/card-swap/typein") { _ in .screen { CardSwapSpikeView(mode: .typein) } }
            registry.register("spike/storage/write") { _ in .screen { StorageSpikeView(write: true) } }
            registry.register("spike/storage/read") { _ in .screen { StorageSpikeView(write: false) } }
            registry.register("spike/media-seek") { _ in .screen { MediaSeekSpikeView() } }
            registry.register("spike/media-seek/audio") { _ in .screen { MediaSeekSpikeView(audio: true) } }
        }
    }

    /// The S1 harness on screen: the swap count (the chrome that changes in
    /// the swap's commit), the JSON report, and the stage.
    struct CardSwapSpikeView: View {
        @State private var spike: CardSwapSpike
        @State private var control = ""

        init(mode: CardSwapSpike.Mode) {
            _spike = State(initialValue: CardSwapSpike(mode: mode))
        }

        var body: some View {
            VStack(spacing: Spacing.xxSmall) {
                Text("Swaps: \(spike.swaps)")
                    .accessibilityIdentifier("spike.cardSwap.count")
                Text(spike.reportJSON)
                    .font(.caption2)
                    .lineLimit(1)
                    .accessibilityIdentifier("spike.cardSwap.report")
                #if !os(macOS)
                if spike.mode == .typein {
                    // A native field: shows whether this simulator raises a
                    // software keyboard at all.
                    TextField("Control", text: $control)
                        .accessibilityIdentifier("spike.cardSwap.control")
                }
                #endif
                StageHost(stage: spike.stage)
            }
            .onAppear { spike.start() }
            .onDisappear { spike.stop() }
        }
    }

    #if os(macOS)
    struct StageHost: NSViewRepresentable {
        let stage: Stage
        func makeNSView(context: Context) -> Stage { stage }
        func updateNSView(_ view: Stage, context: Context) {}
    }
    #else
    struct StageHost: UIViewRepresentable {
        let stage: Stage
        func makeUIView(context: Context) -> Stage { stage }
        func updateUIView(_ view: Stage, context: Context) {}
    }
    #endif
}
#endif
