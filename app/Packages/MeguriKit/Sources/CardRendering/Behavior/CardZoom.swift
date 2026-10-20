import Foundation

public nonisolated struct CardZoom: Equatable, Sendable {
    public let percent: Double
    public init(percent: Double = 100) {
        self.percent = percent.isFinite ? min(300, max(50, percent)) : 100
    }
    public var scale: CGFloat { CGFloat(percent / 100) }
}

/// Installed in the actual document's isolated world, never the shared
/// WKUserContentController. Metrics are CSS pixels, including zero extents.
enum CardLayoutContinuity {
    static let source = """
        (() => {
          let binding = 0;
          let user = 0;
          let pending = 0;
          function metrics() {
            const e = document.scrollingElement || document.documentElement;
            return { x: window.scrollX, y: window.scrollY,
              ex: Math.max(0, e.scrollWidth - e.clientWidth),
              ey: Math.max(0, e.scrollHeight - e.clientHeight),
              w: window.innerWidth, h: window.innerHeight };
          }
          let old = metrics();
          const fraction = (offset, extent) => extent > 0 ? Math.min(extent, Math.max(0, offset)) / extent : 0;
          for (const name of ['wheel', 'touchstart', 'pointerdown', 'keydown', 'input'])
            window.addEventListener(name, () => { user++; pending++; old = metrics(); }, true);
          window.addEventListener('scroll', () => {
            const now = metrics();
            if (now.w === old.w && now.h === old.h) { old = now; pending++; }
          }, true);
          function rememberLayout() {
            const now = metrics();
            if (now.w === old.w && now.h === old.h) old = now;
          }
          const layout = new ResizeObserver(rememberLayout);
          layout.observe(document.documentElement);
          if (document.body) layout.observe(document.body);
          new MutationObserver(rememberLayout).observe(document, {subtree:true, childList:true, attributes:true});
          window.addEventListener('resize', () => {
            const saved = old;
            const resized = metrics();
            if (saved.w === resized.w && saved.h === resized.h) return;
            const scrollVersion = ++pending;
            const cardVersion = binding;
            const gestureVersion = user;
            const proportions = [fraction(saved.x, saved.ex), fraction(saved.y, saved.ey)];
            // Defer one frame for coalesced bounds; newer input wins.
            requestAnimationFrame(() => {
              if (scrollVersion !== pending || cardVersion !== binding || gestureVersion !== user) return;
              const limits = metrics();
              window.scrollTo({behavior:'instant', top:limits.ey * proportions[1], left:limits.ex * proportions[0]});
              old = metrics();
            });
          });
          window.meguriCardLayout = {
            bind() { binding++; pending++; old = metrics(); },
            ready() { pending++; old = metrics(); }
          };
        })();
        """
}
