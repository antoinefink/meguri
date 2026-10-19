import SwiftUI

/// Layout changes move only the container; pages and documents stay alive.
public struct CardWebView {
    public let pair: CardPagePair
    public init(pair: CardPagePair) { self.pair = pair }
    private func host(_ host: CardPlatformView) {
        if pair.container.superview !== host {
            pair.container.removeFromSuperview()
            pair.container.frame = host.bounds
            #if os(macOS)
            pair.container.autoresizingMask = [.width, .height]
            #else
            pair.container.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            #endif
            host.addSubview(pair.container)
        }
    }
}
#if os(macOS)
extension CardWebView: NSViewRepresentable {
    public func makeNSView(context: Context) -> CardPlatformView { let view = CardPlatformView(); host(view); return view }
    public func updateNSView(_ view: CardPlatformView, context: Context) { host(view) }
}
#else
extension CardWebView: UIViewRepresentable {
    public func makeUIView(context: Context) -> CardPlatformView { let view = CardPlatformView(); host(view); return view }
    public func updateUIView(_ view: CardPlatformView, context: Context) { host(view) }
}
#endif
