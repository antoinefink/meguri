#if MEGURI_TEST_HOOKS
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

extension TestHooks {
    /// The launch overrides that live in the view tree: Reduce Motion and
    /// Reduce Transparency mapped onto SwiftUI's underscored environment
    /// values (no simulator switch exists for them), animations off, and the
    /// Mac window size.
    struct AppearanceOverrides: ViewModifier {
        let options: LaunchOptions

        /// Overrides that are process-wide rather than per view; called once
        /// before the first scene.
        static func applyProcessWide(_ options: LaunchOptions) {
            #if !os(macOS)
            if let enabled = options.animationsEnabled {
                UIView.setAnimationsEnabled(enabled)
            }
            #endif
        }

        func body(content: Content) -> some View {
            content
                .transformEnvironment(\._accessibilityReduceMotion) { value in
                    if let reduceMotion = options.reduceMotion { value = reduceMotion }
                }
                .transformEnvironment(\._accessibilityReduceTransparency) { value in
                    if let reduceTransparency = options.reduceTransparency { value = reduceTransparency }
                }
                .transaction { transaction in
                    if options.animationsEnabled == false {
                        transaction.disablesAnimations = true
                        transaction.animation = nil
                    }
                }
                #if os(macOS)
                .background(WindowConfigurator(
                    size: options.windowSize ?? (options == LaunchOptions() ? nil : .standard),
                    animationsEnabled: options.animationsEnabled
                ))
                #endif
        }
    }

    #if os(macOS)
    /// Sizes the window once it exists, after SwiftUI has applied its default.
    struct WindowConfigurator: NSViewRepresentable {
        let size: LaunchOptions.WindowSize?
        let animationsEnabled: Bool?

        func makeNSView(context: Context) -> WindowHookView {
            WindowHookView(size: size, animationsEnabled: animationsEnabled)
        }

        func updateNSView(_ view: WindowHookView, context: Context) {}
    }

    /// Also resizes the running window on request (U8, R1962): `AgentDriver`'s
    /// resize step posts the distributed notification `resizeRequest` with a
    /// `-MeguriWindowSize` value as its object. The window takes that size as
    /// a launch with the option would, without a relaunch, so the app keeps
    /// its state; then the app posts `resized`.
    final class WindowHookView: NSView {
        static let resizeRequest = Notification.Name("\(Bundle.main.bundleIdentifier ?? "").testhooks.resizeWindow")
        static let resized = Notification.Name("\(Bundle.main.bundleIdentifier ?? "").testhooks.windowResized")

        private let size: LaunchOptions.WindowSize?
        private let animationsEnabled: Bool?
        private var applied = false

        init(size: LaunchOptions.WindowSize?, animationsEnabled: Bool?) {
            self.size = size
            self.animationsEnabled = animationsEnabled
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("init(coder:) is not used")
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            let center = DistributedNotificationCenter.default()
            center.removeObserver(self, name: Self.resizeRequest, object: nil)
            guard let window else { return }
            // Delivered while the app is inactive too: the driver's runner
            // is usually the active app.
            center.addObserver(
                self, selector: #selector(resizeRequested(_:)), name: Self.resizeRequest, object: nil,
                suspensionBehavior: .deliverImmediately
            )
            guard !applied else { return }
            applied = true
            if animationsEnabled == false {
                window.animationBehavior = .none
            }
            guard let size else { return }
            Task { @MainActor in
                Self.resize(window, to: size)
            }
        }

        @objc private func resizeRequested(_ notification: Notification) {
            guard let window, let value = notification.object as? String,
                  let size = try? LaunchOptions.windowSize(value)
            else { return }
            Self.resize(window, to: size)
            window.contentView?.layoutSubtreeIfNeeded()
            DistributedNotificationCenter.default().postNotificationName(
                Self.resized, object: nil, userInfo: nil, deliverImmediately: true
            )
        }

        private static func resize(_ window: NSWindow, to size: LaunchOptions.WindowSize) {
            switch size {
            case .minimum:
                window.setContentSize(WindowMetrics.minimum)
            case .standard:
                window.setContentSize(WindowMetrics.standard)
            case .large:
                if let frame = window.screen?.visibleFrame {
                    window.setFrame(frame, display: true)
                }
            case let .explicit(width, height):
                window.setContentSize(CGSize(width: width, height: height))
            }
        }
    }
    #endif
}
#endif
