import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// The Statistics page on the Mac and the iPad at regular width (plan U296; `stats/statistics-page`
/// S5, S6, S10, S12, S14; contract N25.1 to N25.3, N25.9 to N25.11): the window's title
/// "Statistics" with the subtitle and its status, the toolbar's deck pop-up and period control
/// and the shell's sync button; then one scroll view with the strip and the seven sections, each
/// a heading and its grid of boxes; or one of S12's states in their place.
public struct StatisticsView: View {
    private let page: StatsPage
    private let navigation: AppNavigation?
    @Environment(\.syncStatusItem) private var syncStatusItem
    @Environment(\.locale) private var locale

    /// - Parameter navigation: keeps the page's scroll position across a layout switch (R5).
    public init(page: StatsPage, navigation: AppNavigation? = nil) {
        self.page = page
        self.navigation = navigation
    }

    private var model: StatsPageModel { page.model }

    public var body: some View {
        content
            .navigationTitle(Text("stats.title", bundle: .module))
            .navigationSubtitle(Text(verbatim: model.subtitle ?? ""))
            .toolbar {
                ToolbarItem(placement: .principal) {
                    StatsDeckPopUp(model: model)
                }
                ToolbarItem(placement: .principal) {
                    StatsPeriodControl(model: model)
                }
                // N23.6: the shell's sync button, in its trailing place.
                if let syncStatusItem { ToolbarItem(placement: .primaryAction) { syncStatusItem } }
            }
            .onAppear { page.appear() }
            .onDisappear { page.disappear() }
            .onChange(of: model.selection) { old, new in page.selectionChanged(from: old, to: new) }
            .onChange(of: model.boxes) { page.boxesChanged() }
            .onChange(of: model.state) { page.boxesChanged() }
    }

    @ViewBuilder private var content: some View {
        switch model.state {
        case .noCollection:
            if page.opening {
                // A collection opening: nothing yet, as the shell shows it (S12.1).
                Color.clear
                    .accessibilityIdentifier("stats.page.opening")
            } else {
                ContentUnavailableView {
                    Label { Text("stats.page.noCollection", bundle: .module) } icon: { Image(systemName: "chart.bar.xaxis") }
                } description: {
                    Text("stats.page.noCollection.message", bundle: .module)
                }
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("stats.page.noCollection")
            }
        case .noCards:
            ContentUnavailableView {
                Label { Text("stats.page.noCards", bundle: .module) } icon: { Image(systemName: "rectangle.stack") }
            } description: {
                Text("stats.page.noCards.message", bundle: .module)
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("stats.page.noCards")
        case .deckMissing:
            ContentUnavailableView {
                Label { Text("stats.page.deckMissing", bundle: .module) } icon: { Image(systemName: "questionmark.folder") }
            } actions: {
                Button { model.selectDeck(nil) } label: { Text("stats.page.showAllDecks", bundle: .module) }
                    .accessibilityIdentifier("stats.page.showAllDecks")
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("stats.page.deckMissing")
        case .unavailable:
            // S12.8: the strip and the controls stay; the sections give way.
            VStack(alignment: .leading, spacing: 0) {
                StatsStrip(model: model) { _ in }
                    .padding()
                ContentUnavailableView {
                    Label { Text("stats.page.unavailable", bundle: .module) } icon: { Image(systemName: "exclamationmark.triangle") }
                } description: {
                    Text("stats.page.unavailable.message", bundle: .module)
                }
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("stats.page.unavailable")
            }
        case .content:
            StatsPageScroll(page: page, navigation: navigation)
        }
    }
}

/// The page's scroll view: the strip, the filtered deck's line, then the seven sections.
private struct StatsPageScroll: View {
    let page: StatsPage
    let navigation: AppNavigation?
    private var model: StatsPageModel { page.model }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: Spacing.xLarge) {
                    VStack(alignment: .leading, spacing: Spacing.small) {
                        StatsStrip(model: model) { page.go(to: $0) }
                        if let note = model.filteredDeckNote {
                            Text(verbatim: note)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("stats.page.filteredDeck")
                        }
                    }
                    .id("strip")
                    ForEach(StatsSectionID.allCases, id: \.self) { section in
                        StatsSectionView(section: section, model: model)
                            .id(section.rawValue)
                    }
                }
                .padding()
                .scrollTargetLayout()
            }
            .modifier(StatsScrollPosition(navigation: navigation))
            .onChange(of: page.requestedSection) { _, section in
                guard let section else { return }
                withAnimation { proxy.scrollTo(section.rawValue, anchor: .top) }
                page.requestedSection = nil
            }
        }
        .accessibilityIdentifier("stats.page")
    }
}

/// The section root keeps its scroll position in the navigation (app/AGENTS.md); a sample page
/// with no navigation keeps none.
private struct StatsScrollPosition: ViewModifier {
    let navigation: AppNavigation?

    func body(content: Content) -> some View {
        if let navigation {
            content.navigationScrollPosition(navigation, .section(.stats), idType: String.self)
        } else {
            content
        }
    }
}

// MARK: Sections

/// A section: its heading (the title in bold and its question in secondary text on the same
/// line, under it when narrow, N25.3), then its boxes in the grid.
struct StatsSectionView: View {
    let section: StatsSectionID
    let model: StatsPageModel
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            heading
            StatsBoxGrid(leads: section.boxes.map { $0 == section.lead }, spacing: Spacing.medium) {
                ForEach(section.boxes, id: \.self) { box in
                    boxView(box)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.section.\(section.rawValue)")
    }

    private var heading: some View {
        let title = Text(verbatim: StatsPageText.title(section, locale: locale)).font(.title3.weight(.semibold))
        let question = Text(verbatim: StatsPageText.question(section, locale: locale)).foregroundStyle(.secondary)
        return ViewThatFits(in: .horizontal) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.xSmall) {
                title
                question
            }
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                title
                question
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
        .accessibilityIdentifier("stats.section.\(section.rawValue).heading")
    }

    @ViewBuilder private func boxView(_ box: StatsBoxID) -> some View {
        let slot = StatsBoxSlot(id: box, model: model, size: box == section.lead ? .lead : .regular, open: {})
        switch section {
        case .knowledge: StatsKnowledgeSection.box(slot)
        case .reviews: StatsReviewsSection.box(slot)
        case .cards: StatsCardsSection.box(slot)
        case .comingUp: StatsComingUpSection.box(slot)
        case .answering: StatsAnsweringSection.box(slot)
        case .memory: StatsMemorySection.box(slot)
        case .decks: StatsDecksSection.box(slot)
        }
    }
}

/// N25.3's grid: one column under about 560 points of content width, two up to about 1,250,
/// three beyond. A lead box spans its row; the others follow side by side and wrap; boxes in a
/// row share the row's height.
struct StatsBoxGrid: Layout {
    /// Per subview: whether it is a lead box.
    let leads: [Bool]
    let spacing: CGFloat

    static func columns(for width: CGFloat) -> Int {
        width < 560 ? 1 : width < 1_250 ? 2 : 3
    }

    /// Rows of subview indices, and whether each row is a lead's.
    private func rows(count: Int, columns: Int) -> [(indices: [Int], lead: Bool)] {
        var rows: [(indices: [Int], lead: Bool)] = []
        var row: [Int] = []
        for index in 0..<count {
            let lead = index < leads.count && leads[index]
            if lead || row.count == columns {
                if !row.isEmpty { rows.append((row, false)) }
                row = []
            }
            if lead { rows.append(([index], true)) } else { row.append(index) }
        }
        if !row.isEmpty { rows.append((row, false)) }
        return rows
    }

    private func layout(width: CGFloat, subviews: Subviews) -> (frames: [CGRect], height: CGFloat) {
        let columns = Self.columns(for: width)
        let column = max(0, (width - spacing * CGFloat(columns - 1)) / CGFloat(columns))
        var frames = Array(repeating: CGRect.zero, count: subviews.count)
        var y: CGFloat = 0
        for (offset, row) in rows(count: subviews.count, columns: columns).enumerated() {
            if offset > 0 { y += spacing }
            let cell = row.lead ? width : column
            let height = row.indices.map { subviews[$0].sizeThatFits(ProposedViewSize(width: cell, height: nil)).height }.max() ?? 0
            for (position, index) in row.indices.enumerated() {
                frames[index] = CGRect(x: CGFloat(position) * (column + spacing), y: y, width: cell, height: height)
            }
            y += height
        }
        return (frames, y)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 800
        return CGSize(width: width, height: layout(width: width, subviews: subviews).height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let frames = layout(width: bounds.width, subviews: subviews).frames
        for (index, frame) in frames.enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), anchor: .topLeading,
                                  proposal: ProposedViewSize(width: frame.width, height: frame.height))
        }
    }
}

// MARK: Strip

/// S6's strip: five figures with no box, each its value with the unit in small text and its
/// label under; a click scrolls to its section. A figure whose part has not returned shows the
/// small progress indicator in its value's place (S10.1).
struct StatsStrip: View {
    let model: StatsPageModel
    let go: (StatsSectionID) -> Void
    @Environment(\.locale) private var locale
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        let columns = dynamicTypeSize.isAccessibilitySize ? 2 : 5
        Grid(alignment: .topLeading, horizontalSpacing: Spacing.large, verticalSpacing: Spacing.small) {
            ForEach(Array(stride(from: 0, to: StatsStripID.allCases.count, by: columns)), id: \.self) { start in
                GridRow(alignment: .top) {
                    ForEach(StatsStripID.allCases[start..<min(start + columns, StatsStripID.allCases.count)], id: \.self) { id in
                        figure(id)
                    }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("stats.strip")
    }

    private func figure(_ id: StatsStripID) -> some View {
        let figure = model.strip(id)
        let label = StatsPageText.label(id, locale: locale)
        return Button { go(id.section) } label: {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.xxSmall) {
                    // The value's line keeps its height while the indicator shows.
                    Text(verbatim: figure.value ?? "0")
                        .font(.title2.weight(.semibold))
                        .monospacedDigit()
                        .lineLimit(1)
                        .opacity(figure.loading ? 0 : 1)
                        .overlay(alignment: .leading) {
                            if figure.loading { ProgressView().controlSize(.small) }
                        }
                    if !figure.loading, let unit = figure.unit {
                        Text(verbatim: unit)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    if !figure.loading, let detail = figure.detail {
                        Text(verbatim: detail)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                Text(verbatim: label)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityValue(Text(verbatim: figure.loading ? "" : figure.spoken ?? figure.value ?? ""))
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("stats.strip.\(id.rawValue)")
    }
}

// MARK: Toolbar

/// S3.1's deck pop-up: All Decks, then the deck tree indented in the deck list's order, filtered
/// decks last. Choosing writes nothing (S17). The deck's name truncates in the middle, with the
/// full name as help (N25.2).
struct StatsDeckPopUp: View {
    let model: StatsPageModel
    @Environment(\.locale) private var locale

    private var shown: (name: String, full: String) {
        guard let id = model.selection.deck else {
            let all = String(localized: "stats.deck.allDecks", bundle: .module, locale: locale)
            return (all, all)
        }
        guard let deck = model.decks.deck(id) else { return ("", "") }
        return (deck.name, deck.fullName)
    }

    var body: some View {
        let items = model.deckMenu
        let normal = items.filter { !$0.filtered }
        let filtered = items.filter(\.filtered)
        Menu {
            Picker(selection: Binding(get: { model.selection.deck }, set: { model.selectDeck($0) })) {
                Text("stats.deck.allDecks", bundle: .module)
                    .tag(Int64?.none)
                    .accessibilityIdentifier("stats.deck.all")
                Section {
                    ForEach(normal) { item in
                        // The tree's depth as an indent (S3.1).
                        Text(verbatim: String(repeating: "\u{2003}", count: item.depth) + item.name)
                            .tag(Optional(item.id))
                            .accessibilityLabel(Text(verbatim: item.name))
                            .accessibilityIdentifier("stats.deck.\(item.id)")
                    }
                }
                if !filtered.isEmpty {
                    Section {
                        ForEach(filtered) { item in
                            Text(verbatim: item.name)
                                .tag(Optional(item.id))
                                .accessibilityIdentifier("stats.deck.\(item.id)")
                        }
                    }
                }
            } label: {
                Text("stats.deck.label", bundle: .module)
            }
            .pickerStyle(.inline)
        } label: {
            Text(verbatim: shown.name)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .help(Text(verbatim: shown.full))
        .disabled(!model.controlsEnabled)
        .accessibilityLabel(Text("stats.deck.label", bundle: .module))
        .accessibilityValue(Text(verbatim: shown.full))
        .accessibilityIdentifier("stats.deck")
    }
}

/// S3.2's period control: 1M, 3M, 6M, 1Y, All, read as "1 Month" to "All".
struct StatsPeriodControl: View {
    let model: StatsPageModel
    @Environment(\.locale) private var locale

    var body: some View {
        Picker(selection: Binding(get: { model.selection.period }, set: { model.selectPeriod($0) })) {
            ForEach(StatsPagePeriod.allCases, id: \.self) { period in
                Text(verbatim: StatsPageText.name(period, locale: locale))
                    .accessibilityLabel(Text(verbatim: StatsPageText.spoken(period, locale: locale)))
                    .tag(period)
                    .accessibilityIdentifier("stats.period.\(StatsPageText.identifier(period))")
            }
        } label: {
            Text("stats.period.label", bundle: .module)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .disabled(!model.controlsEnabled)
        .accessibilityIdentifier("stats.period")
    }
}
