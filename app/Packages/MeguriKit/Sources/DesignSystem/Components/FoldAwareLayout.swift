import SwiftUI

/// Where a foldable iPhone's fold runs top to bottom through a view (R9), from
/// the system's reserved regions. `.includeInactive` keeps a fully open fold in
/// the answer, so controls keep one place in every fold position (N13).
nonisolated public struct FoldGeometry: Equatable, Sendable {
    /// The x range the fold takes in the view. A fold running side to side
    /// doesn't split a row, so it isn't reported.
    public var acrossWidth: ClosedRange<CGFloat>?

    public init(frames: [CGRect] = []) {
        for frame in frames where frame.height >= frame.width {
            acrossWidth = frame.minX...frame.maxX
        }
    }

    public init(_ proxy: GeometryProxy) {
        #if os(iOS)
        if #available(iOS 27.1, *) {
            self.init(frames: proxy.reservedRegions(kind: .division, options: .includeInactive).map(\.frame))
        } else {
            self.init()
        }
        #else
        self.init()
        #endif
    }
}

/// Two columns that stay off a fold running through them: each side of the
/// fold when there is one, otherwise two equal columns.
nonisolated public struct FoldAvoidingRow: Layout {
    /// Narrower than this, a side can't hold two answer buttons.
    static let minimumColumn: CGFloat = 120
    public var fold: ClosedRange<CGFloat>?
    public var spacing: CGFloat

    public init(fold: ClosedRange<CGFloat>?, spacing: CGFloat) {
        self.fold = fold
        self.spacing = spacing
    }

    /// The x ranges of the two columns in a row `width` wide.
    public static func columns(width: CGFloat, fold: ClosedRange<CGFloat>?, spacing: CGFloat)
        -> (leading: ClosedRange<CGFloat>, trailing: ClosedRange<CGFloat>) {
        if let fold, fold.lowerBound >= minimumColumn, width - fold.upperBound >= minimumColumn {
            return (0...fold.lowerBound, fold.upperBound...width)
        }
        let column = max(0, width - spacing) / 2
        return (0...column, (width - column)...max(width - column, width))
    }

    public func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil }
            ?? subviews.reduce(spacing) { $0 + $1.sizeThatFits(.unspecified).width }
        let parts = Self.columns(width: width, fold: fold, spacing: spacing)
        let spans = [parts.leading, parts.trailing]
        let height = zip(subviews, spans).reduce(CGFloat.zero) { tallest, pair in
            max(tallest, pair.0.sizeThatFits(ProposedViewSize(width: pair.1.upperBound - pair.1.lowerBound, height: proposal.height)).height)
        }
        return CGSize(width: width, height: height)
    }

    public func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let parts = Self.columns(width: bounds.width, fold: fold, spacing: spacing)
        for (subview, span) in zip(subviews, [parts.leading, parts.trailing]) {
            subview.place(at: CGPoint(x: bounds.minX + span.lowerBound, y: bounds.minY), anchor: .topLeading,
                proposal: ProposedViewSize(width: span.upperBound - span.lowerBound, height: bounds.height))
        }
    }
}

/// One child as wide as `FoldAvoidingRow`'s trailing column, centred in the
/// row. With a fold through the row it spans the fold: Show Answer on the
/// iPhone Duo held wide sits under the card's centre at its half-row size
/// (N16.3, an exception to R9 decided 2026-11-22).
nonisolated public struct FoldCentredRow: Layout {
    public var fold: ClosedRange<CGFloat>?
    public var spacing: CGFloat

    public init(fold: ClosedRange<CGFloat>?, spacing: CGFloat) {
        self.fold = fold
        self.spacing = spacing
    }

    /// The child's x range in a row `width` wide.
    public static func span(width: CGFloat, fold: ClosedRange<CGFloat>?, spacing: CGFloat) -> ClosedRange<CGFloat> {
        let trailing = FoldAvoidingRow.columns(width: width, fold: fold, spacing: spacing).trailing
        let childWidth = trailing.upperBound - trailing.lowerBound
        let start = max(0, (width - childWidth) / 2)
        return start...(start + childWidth)
    }

    public func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let child = subviews.first else { return .zero }
        guard let width = proposal.width, width.isFinite else { return child.sizeThatFits(proposal) }
        let span = Self.span(width: width, fold: fold, spacing: spacing)
        let size = child.sizeThatFits(ProposedViewSize(width: span.upperBound - span.lowerBound, height: proposal.height))
        return CGSize(width: width, height: size.height)
    }

    public func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let span = Self.span(width: bounds.width, fold: fold, spacing: spacing)
        subviews.first?.place(at: CGPoint(x: bounds.minX + span.lowerBound, y: bounds.minY), anchor: .topLeading,
            proposal: ProposedViewSize(width: span.upperBound - span.lowerBound, height: bounds.height))
    }
}
