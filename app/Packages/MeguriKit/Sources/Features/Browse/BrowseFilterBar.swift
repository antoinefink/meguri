#if os(macOS)
import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The filter bar (N21.2 of 2026-11-24; `search/filter-tokens` T1, T10): the
/// query's tokens in their order, each a menu button with T10's menu, across
/// the table's width and on as many lines as they need; then Filter, which
/// opens a kind's value list in the search field, and at the trailing edge
/// Save Search… (T13) and Clear. It shows while the query has a token.
///
/// Exception to the native contract, for a functional need: macOS fixes the
/// width of the toolbar's search field, so its own tokens scroll out of sight
/// after the second. The tokens are system menu buttons here; the field, its
/// prompt and its suggestions stay the system's.
struct BrowseFilterBar: View {
    let model: BrowseModel

    /// Filter's kinds, in the suggestions' order (T12.1).
    private static let kinds: [FilterSection] = [.cardState, .flag, .due, .added, .edited, .studied, .decks, .tags, .notetypes,
                                                 .cardTypes, .fields]

    var body: some View {
        if !model.query.tokens.isEmpty {
            VStack(spacing: 0) {
                HStack(alignment: .top, spacing: Spacing.small) {
                    FlowLayout(spacing: Spacing.xSmall) {
                        ForEach(Array(model.query.tokens.enumerated()), id: \.offset) { index, token in
                            BrowseFilterChip(model: model, index: index, token: token)
                        }
                        addMenu
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    Button { model.startSavingSearch() } label: { Text("browse.search.saveSearch", bundle: .module) }
                        .disabled(!model.canSaveSearch)
                        .accessibilityIdentifier("browse.filterBar.save")
                    Button { model.clearField() } label: { Text("browse.filterBar.clear", bundle: .module) }
                        .accessibilityIdentifier("browse.filterBar.clear")
                }
                .buttonStyle(.borderless)
                .controlSize(.small)
                .padding(.horizontal, Spacing.small)
                .padding(.vertical, Spacing.xSmall)
                Divider()
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("browse.filterBar.label", bundle: .module))
            .accessibilityIdentifier("browse.filterBar")
        }
    }

    private var addMenu: some View {
        Menu {
            ForEach(Self.kinds, id: \.self) { section in
                Button { model.startFilter(section) } label: {
                    Label { Text(verbatim: BrowseStrings.filterBy(section)) } icon: { Image(systemName: section.symbol) }
                }
            }
        } label: {
            Label { Text("browse.filterBar.add", bundle: .module) } icon: { Image(systemName: "plus") }
                .labelStyle(.titleAndIcon)
        }
        .menuStyle(.button)
        .menuIndicator(.hidden)
        .fixedSize()
        .accessibilityIdentifier("browse.filterBar.add")
    }
}

/// One token as a menu button: the kind, "is not" after it when negated, then
/// the values; a flag's colour dot; an advanced token's text. Its menu is the
/// token's (T10).
private struct BrowseFilterChip: View {
    let model: BrowseModel
    let index: Int
    let token: FilterToken

    var body: some View {
        Menu {
            BrowseTokenMenu(model: model, index: index, token: token)
        } label: {
            Label {
                title
            } icon: {
                if let flag {
                    Image(systemName: "circle.fill").foregroundStyle(BrowseStrings.flagColor(flag))
                }
            }
            .lineLimit(1)
        }
        .menuStyle(.button)
        .buttonStyle(.bordered)
        .buttonBorderShape(.capsule)
        .help(Text(verbatim: spoken))
        .accessibilityLabel(Text(verbatim: spoken))
        .accessibilityIdentifier("browse.search.token.\(index)")
    }

    private var title: Text {
        switch token {
        case .advanced(let text):
            return Text(verbatim: text).foregroundStyle(.secondary)
        case .value(let values, let negated):
            if let duplicates { return Text(verbatim: BrowseFindText.duplicates(field: duplicates.field, groups: duplicates.groups)) }
            guard let kind = token.kind else { return Text(verbatim: "") }
            var label = AttributedString(BrowseStrings.kindLabel(kind, negated: negated) + " ")
            label.foregroundColor = .secondary
            return Text(label + AttributedString(BrowseStrings.values(values, flagNames: model.flagNames)))
        }
    }

    private var flag: UInt8? {
        guard case .value(let values, _) = token else { return nil }
        return values.lazy.compactMap { value -> UInt8? in if case .flag(let flag) = value { flag } else { nil } }.first
    }

    /// The facts of the Duplicates token Find Duplicates made, when this is it (V10).
    private var duplicates: BrowseModel.Duplicates? {
        guard case .value(let values, _) = token, values.count == 1, case .duplicates(let ids) = values[0],
              let info = model.duplicates, info.noteIDs == ids else { return nil }
        return info
    }

    private var spoken: String {
        if let duplicates { return BrowseFindText.duplicates(field: duplicates.field, groups: duplicates.groups) }
        return BrowseStrings.accessibility(token, flagNames: model.flagNames)
    }
}

/// Lays its subviews out leading to trailing and onto a new line when the
/// width is used up; each line's views share its vertical centre.
private struct FlowLayout: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let placed = arrange(width: bounds.width, subviews)
        for (subview, frame) in zip(subviews, placed.frames) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.midY), anchor: .leading,
                          proposal: ProposedViewSize(frame.size))
        }
    }

    private func arrange(width: CGFloat, _ subviews: Subviews) -> (size: CGSize, frames: [CGRect]) {
        var frames: [CGRect] = [], line: [Int] = []
        var x: CGFloat = 0, y: CGFloat = 0, widest: CGFloat = 0
        func close() {
            let height = line.map { frames[$0].height }.max() ?? 0
            for index in line { frames[index].origin.y = y + (height - frames[index].height) / 2 }
            y += height
            line = []
        }
        for subview in subviews {
            // A view wider than the bar takes the bar's width and truncates.
            let size = subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
            if x > 0, x + size.width > width {
                close()
                x = 0
                y += spacing
            }
            line.append(frames.count)
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            widest = max(widest, x + size.width)
            x += size.width + spacing
        }
        close()
        return (CGSize(width: widest, height: y), frames)
    }
}
#endif
