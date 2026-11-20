import AppModel
import CoreBridge
import DesignSystem
import HeatmapKit
import SwiftUI

/// The activity heatmap's group (contract N14.11): a header, the grid and one line.
/// It draws only what the store already holds; the page that hosts it loads the scope
/// (`activityLoading`), so the page never waits and a hidden group still refreshes.
struct ActivityHeatmapSection: View {
    let store: ActivityHeatmapStore
    let scope: ActivityScope
    /// The row's content width, measured once laid out; the grid's height follows from it.
    @State private var width: CGFloat = 0
    @Environment(\.calendar) private var calendar

    /// The narrowest column (a square and its gap) the grid packs before it drops a week.
    #if os(macOS)
    static let minimumPitch: CGFloat = 13
    #else
    static let minimumPitch: CGFloat = 20
    #endif

    var body: some View {
        let geometry = ActivityGrid.Geometry(width: width, minimumPitch: Self.minimumPitch)
        let days = store.days(scope)
        let grid = days.map {
            ActivityGrid(counts: $0.counts, weeks: geometry.weeks, todayWeekday: $0.today.weekday,
                         firstWeekday: calendar.firstWeekday)
        }
        // Hidden once a result says the days shown hold no review (AH8); until the first
        // result the group keeps its height, so nothing moves when the squares arrive.
        if grid?.total != 0 || width == 0 {
            Section {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    Group {
                        if let grid, let days {
                            ActivityGridView(grid: grid, geometry: geometry, today: days.today)
                        } else {
                            Color.clear
                        }
                    }
                    .frame(width: max(0, width), height: geometry.height, alignment: .leading)
                    summary(grid, weeks: geometry.weeks)
                        .font(.footnote)
                        .quietText()
                        .fixedSize(horizontal: false, vertical: true)
                        .opacity(grid == nil ? 0 : 1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
                // One element: the grid's squares say nothing one by one (N14.11).
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("Review activity", bundle: .module))
                .accessibilityValue(summary(grid, weeks: geometry.weeks))
                .accessibilityIdentifier("home.activity")
                .accessibilityHidden(grid == nil)
                .listRowSeparator(.hidden)
            } header: {
                Text("Activity", bundle: .module)
                    .accessibilityIdentifier("home.activity.header")
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
    #if os(macOS)
    @Environment(\.locale) private var locale
    #endif

    var body: some View {
        HStack(alignment: .top, spacing: geometry.gap) {
            ForEach(0..<grid.weeks, id: \.self) { column in
                VStack(spacing: geometry.gap) {
                    ForEach(0..<7, id: \.self) { row in
                        if let daysAgo = grid.daysAgo(column: column, row: row) {
                            square(daysAgo)
                        } else {
                            // After today: not drawn, the place kept.
                            Color.clear.frame(width: geometry.side, height: geometry.side)
                        }
                    }
                }
            }
        }
    }

    private func square(_ daysAgo: Int) -> some View {
        let shape = RoundedRectangle(cornerRadius: geometry.side * ActivityGrid.gapRatio)
        return shape
            .fill(Self.style(level: grid.level(daysAgo: daysAgo)))
            .overlay {
                if daysAgo == 0 { shape.strokeBorder(.primary, lineWidth: 1) }
            }
            .frame(width: geometry.side, height: geometry.side)
            #if os(macOS)
            .help(tooltip(daysAgo))
            #endif
    }

    /// System colours only: the secondary fill for no reviews, the accent's four
    /// hierarchical steps for the levels (N14.11).
    static func style(level: Int) -> AnyShapeStyle {
        switch level {
        case 4: AnyShapeStyle(.tint)
        case 3: AnyShapeStyle(.tint.secondary)
        case 2: AnyShapeStyle(.tint.tertiary)
        case 1: AnyShapeStyle(.tint.quaternary)
        default: AnyShapeStyle(.fill.secondary)
        }
    }

    #if os(macOS)
    /// The date and its count. The core gives today's date; a day's date is that
    /// date minus its days, in plain calendar days (AH6).
    private func tooltip(_ daysAgo: Int) -> Text {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        let start = calendar.date(from: DateComponents(year: today.year, month: today.month, day: today.day))
        let date = start.flatMap { calendar.date(byAdding: .day, value: -daysAgo, to: $0) }
        // The date the core named is a calendar date: read it back in the same zone.
        let format = Date.FormatStyle(date: .long, time: .omitted, locale: locale, timeZone: .gmt)
        let label = date.map { $0.formatted(format) } ?? ""
        return Text("\(label): \(Int(grid.count(daysAgo: daysAgo))) reviews", bundle: .module)
    }
    #endif
}

extension View {
    /// Loads the page's heatmap scope off the main actor whenever the store says a
    /// refresh is due, and not while a review covers the page. Leaving the page cancels
    /// the load; a page without a heatmap (`store` nil) loads nothing.
    func activityLoading(_ store: ActivityHeatmapStore?, scope: ActivityScope, active: Bool) -> some View {
        task(id: active ? store?.request(scope) : nil) {
            guard active, let store else { return }
            await store.load(store.request(scope))
        }
    }
}

extension ReviewCollection {
    /// A review covering Home holds the heatmap's refreshes until it ends: its
    /// answers would otherwise ask again after every card.
    var heatmapRefreshes: Bool { review == nil && navigation.review == nil }
}
