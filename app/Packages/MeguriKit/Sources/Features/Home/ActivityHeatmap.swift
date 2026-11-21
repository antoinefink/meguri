import AppModel
import CoreBridge
import DesignSystem
import HeatmapKit
import SwiftUI
#if os(macOS)
import AppKit
import Combine
#endif

/// The heatmap rows' content widths, kept above the views (`AppNavigation.featureState`):
/// a page drawn again (a pushed deck page, a layout switch, a row the list made again)
/// knows its width, so its height and week count are right from its first frame.
@Observable
@MainActor
final class ActivityRowWidths {
    /// Each list has its own row insets, so each keeps its own width.
    enum Page: Hashable { case today, deckList, deck }
    private var rows: [Page: CGFloat] = [:]
    @ObservationIgnored private var lists: [Page: CGFloat] = [:]
    /// Rows of the page on screen now. Two can overlap while one page replaces another
    /// (a pushed deck page, a fold), and their appear and disappear calls come in any order.
    @ObservationIgnored private var mounted: [Page: Int] = [:]

    func row(_ page: Page) -> CGFloat { rows[page] ?? 0 }

    func measuredRow(_ page: Page, _ width: CGFloat) {
        if rows[page] != width { rows[page] = width }
    }

    func rowAppeared(_ page: Page) { mounted[page, default: 0] += 1 }
    func rowLeft(_ page: Page) { mounted[page] = max(0, mounted[page, default: 0] - 1) }

    /// The hosting list's width. When it changes while no row of the page is on screen
    /// (hidden, or the page left), the row's last width no longer holds and the row
    /// measures again when it next shows.
    func measuredList(_ page: Page, _ width: CGFloat) {
        defer { lists[page] = width }
        guard let previous = lists[page], previous != width, mounted[page, default: 0] == 0 else { return }
        rows[page] = nil
    }
}

extension AppNavigation {
    var activityRowWidths: ActivityRowWidths { featureState(ActivityRowWidths.self) { ActivityRowWidths() } }
}

/// The activity heatmap's group (contract N14.11): a header, the grid and one line.
/// It draws only what the store already holds; the page that hosts it loads the scope
/// (`activityLoading`), so the page never waits and a hidden group still refreshes.
struct ActivityHeatmapSection: View {
    let store: ActivityHeatmapStore
    let scope: ActivityScope
    let widths: ActivityRowWidths
    let page: ActivityRowWidths.Page
    @Environment(\.calendar) private var calendar
    /// This row's own measurement. The shared width only seeds a new row's first frame:
    /// while two rows of one page overlap (a fold, a layout switch), the one leaving must
    /// not resize the one that stays.
    @State private var measured: CGFloat?

    /// The narrowest column (a square and its gap) the grid packs before it drops a week.
    #if os(macOS)
    static let minimumPitch: CGFloat = 13
    #else
    static let minimumPitch: CGFloat = 20
    #endif

    var body: some View {
        // A shared width cleared by a resize while no row showed (hidden or away) clears
        // this row's too: it shows again to measure, rather than judge by an old width.
        let shared = widths.row(page)
        let width = shared == 0 ? 0 : measured ?? shared
        let geometry = ActivityGrid.Geometry(width: width, minimumPitch: Self.minimumPitch)
        let days = store.days(scope)
        let grid = days.map {
            ActivityGrid(counts: $0.counts, weeks: geometry.weeks, todayWeekday: $0.today.weekday,
                         firstWeekday: calendar.firstWeekday)
        }
        let shown = grid != nil && geometry.weeks > 0
        // Hidden once a result says the days shown hold no review (AH8); until the first
        // result the group keeps its height, so nothing moves when the squares arrive.
        // Before its width is known the row shows only to measure it, and only when the
        // scope has a review in any of the 53 weeks: a scope known to be empty never
        // flashes its header.
        let visible = if width == 0 {
            days.map { $0.counts.contains { $0 > 0 } } ?? true
        } else {
            grid?.total != 0
        }
        if visible {
            Section {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    // AH9: the months' row. A caption line's height, kept from the first frame
                    // whether or not a label is drawn yet, so the squares never move down.
                    Text(verbatim: "Nov")
                        .font(.caption)
                        .hidden()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .overlay {
                            if let grid, let days, geometry.weeks > 0 {
                                MonthLabelsView(grid: grid, geometry: geometry, today: days.today)
                            }
                        }
                    // Only this clear view sizes the row, and it reports the width offered.
                    // The squares sit in its overlay, so an old width (taken before a fold
                    // or in another column) can never feed itself.
                    Color.clear
                        .frame(maxWidth: .infinity)
                        .frame(height: geometry.height)
                        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: {
                            measured = $0
                            widths.measuredRow(page, $0)
                        }
                        .overlay(alignment: .leading) {
                            if let grid, let days {
                                ActivityGridView(grid: grid, geometry: geometry, today: days.today)
                            }
                        }
                    summary(grid, weeks: geometry.weeks)
                        .font(.footnote)
                        .quietText()
                        .fixedSize(horizontal: false, vertical: true)
                        .opacity(shown ? 1 : 0)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .onAppear { widths.rowAppeared(page) }
                .onDisappear { widths.rowLeft(page) }
                // One element: the grid's squares say nothing one by one (N14.11).
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("Review activity", bundle: .module))
                .accessibilityValue(summary(grid, weeks: geometry.weeks))
                .accessibilityIdentifier("home.activity.grid")
                .accessibilityHidden(!shown)
                .listRowSeparator(.hidden)
            } header: {
                // The secondary token on touch: the system's header grey nearly misses 4.5:1 on the
                // grouped background (accessibility audit).
                Text("Activity", bundle: .module)
                    .quietText()
                    .accessibilityIdentifier("home.activity.header")
                    // No empty header for VoiceOver while the grid is not drawn yet.
                    .accessibilityHidden(!shown)
            }
        }
    }

    private func summary(_ grid: ActivityGrid?, weeks: Int) -> Text {
        Text("\(grid?.total ?? 0) reviews in the last \(weeks) weeks", bundle: .module)
    }
}

/// Weeks as columns, the newest trailing; days as rows from the locale's first weekday.
private struct ActivityGridView: View {
    let grid: ActivityGrid
    let geometry: ActivityGrid.Geometry
    let today: ActivityDays.Day
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.colorScheme) private var colorScheme
    /// The Mac's accent, read again when the system's colours change (System Settings).
    @State private var graphite = Self.currentGraphite
    #if os(macOS)
    @Environment(\.locale) private var locale
    #endif

    var body: some View {
        #if os(macOS)
        let labels = DayLabels(today: today, locale: locale)
        #endif
        HStack(alignment: .top, spacing: geometry.gap) {
            ForEach(0..<grid.weeks, id: \.self) { column in
                VStack(spacing: geometry.gap) {
                    ForEach(0..<7, id: \.self) { row in
                        if let daysAgo = grid.daysAgo(column: column, row: row) {
                            #if os(macOS)
                            square(daysAgo).help(labels.tooltip(daysAgo, count: grid.count(daysAgo: daysAgo)))
                            #else
                            square(daysAgo)
                            #endif
                        } else {
                            // After today: not drawn, the place kept.
                            Color.clear.frame(width: geometry.side, height: geometry.side)
                        }
                    }
                }
            }
        }
        #if os(macOS)
        .onReceive(NotificationCenter.default.publisher(for: NSColor.systemColorsDidChangeNotification)) { _ in
            graphite = Self.currentGraphite
        }
        #endif
    }

    private func square(_ daysAgo: Int) -> some View {
        let shape = RoundedRectangle(cornerRadius: geometry.side * ActivityGrid.gapRatio)
        let level = grid.level(daysAgo: daysAgo)
        let increased = contrast == .increased
        return shape
            .fill(Self.style(level: level, increasedContrast: increased, dark: colorScheme == .dark, graphite: graphite))
            // N24.6: with Increase Contrast the steps lie over the empty day's own fill.
            .background { if increased && level > 0 { shape.fill(.fill.secondary) } }
            .overlay {
                if daysAgo == 0 { shape.strokeBorder(.primary, lineWidth: 1) }
            }
            .frame(width: geometry.side, height: geometry.side)
    }

    /// The secondary fill for no reviews; the accent at N24.6's fixed opacities for the
    /// levels (`ActivityGrid.opacity`). The Mac's Graphite accent would make grey steps
    /// that do not tell a light day from an empty one, so they use the system blue.
    static func style(level: Int, increasedContrast: Bool, dark: Bool, graphite: Bool) -> AnyShapeStyle {
        guard let opacity = ActivityGrid.opacity(level: level, increasedContrast: increasedContrast, dark: dark) else {
            return AnyShapeStyle(.fill.secondary)
        }
        return graphite ? AnyShapeStyle(Color.blue.opacity(opacity)) : AnyShapeStyle(TintShapeStyle().opacity(opacity))
    }

    private static var currentGraphite: Bool {
        #if os(macOS)
        NSColor.currentControlTint == .graphiteControlTint
        #else
        false
        #endif
    }
}

/// AH9: the months above the grid, in caption secondary text. Drawn, not laid out: which
/// labels fit depends on their measured widths, and the row's height is kept by its
/// host from the first frame whether or not any label is drawn.
private struct MonthLabelsView: View {
    let grid: ActivityGrid
    let geometry: ActivityGrid.Geometry
    let today: ActivityDays.Day
    @Environment(\.locale) private var locale
    @Environment(\.layoutDirection) private var layoutDirection

    var body: some View {
        Canvas { context, size in
            var calendar = Calendar(identifier: .gregorian)
            calendar.locale = locale
            let names = calendar.shortStandaloneMonthSymbols
            guard names.count == 12 else { return }
            #if os(macOS)
            let resolved = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(.secondary)) }
            #else
            let resolved = names.map { context.resolve(Text(verbatim: $0).font(.caption).foregroundStyle(.token(.textSecondary))) }
            #endif
            let labels = grid.monthLabels(year: today.year, month: today.month, day: today.day,
                                          pitch: geometry.pitch, width: size.width,
                                          rightToLeft: layoutDirection == .rightToLeft) { resolved[$0 - 1].measure(in: size).width }
            // The canvas does not mirror itself; the label positions already are.
            for label in labels {
                context.draw(resolved[label.month - 1], at: CGPoint(x: label.x, y: 0), anchor: .topLeading)
            }
        }
    }
}

#if os(macOS)
/// The Mac's hover text for a day: its date and count. The core gives today's date; a
/// day's date is that date minus its days, in plain calendar days (AH6), read back in
/// the same zone as it was built in.
private struct DayLabels {
    private let calendar: Calendar
    private let start: Date?
    private let format: Date.FormatStyle

    init(today: ActivityDays.Day, locale: Locale) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        self.calendar = calendar
        start = calendar.date(from: DateComponents(year: today.year, month: today.month, day: today.day))
        format = Date.FormatStyle(date: .long, time: .omitted, locale: locale, timeZone: .gmt)
    }

    func tooltip(_ daysAgo: Int, count: UInt32) -> Text {
        let date = start.flatMap { calendar.date(byAdding: .day, value: -daysAgo, to: $0) }
        let label = date.map { $0.formatted(format) } ?? ""
        return Text("\(label): \(Int(count)) reviews", bundle: .module)
    }
}
#endif

extension View {
    /// Loads the page's heatmap scope off the main actor whenever the store says a
    /// refresh is due, and not while a review covers the page. Leaving the page cancels
    /// the load; a page without a heatmap (`store` nil) loads nothing.
    /// It also tells `widths` the list's width, so a group that is not on screen learns of
    /// a rotation, a resize or a fold.
    func activityLoading(_ store: ActivityHeatmapStore?, scope: ActivityScope, active: Bool,
                         widths: ActivityRowWidths?, page: ActivityRowWidths.Page) -> some View {
        task(id: active ? store?.request(scope) : nil) {
            guard active, let store else { return }
            await store.load(store.request(scope))
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width in
            widths?.measuredList(page, width)
        }
    }
}

extension ReviewCollection {
    /// A review covering Home holds the heatmap's refreshes until it ends: its
    /// answers would otherwise ask again after every card.
    var heatmapRefreshes: Bool { review == nil && navigation.review == nil }
}
