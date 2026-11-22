import AppModel
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The three ways to start without a collection (N18.3), in the order the screen lists them.
nonisolated enum WelcomeChoice: CaseIterable, Sendable {
    case sync, importFile, startEmpty

    /// The surviving actions keep the identifiers they had as buttons.
    var identifier: String {
        switch self {
        case .sync: "home.collection.accountSync"
        case .importFile: "home.collection.import"
        case .startEmpty: "home.collection.startEmpty"
        }
    }
    var systemImage: String {
        switch self {
        case .sync: "cloud"
        case .importFile: "folder"
        case .startEmpty: "plus"
        }
    }
    var title: String {
        switch self {
        case .sync: String(localized: "Set Up Sync", bundle: .module)
        case .importFile: String(localized: "Import a Collection File", bundle: .module)
        case .startEmpty: String(localized: "Start Empty", bundle: .module)
        }
    }
    var explanation: String {
        switch self {
        case .sync: String(localized: "Get your collection from Meguri Sync or iCloud.", bundle: .module)
        case .importFile: String(localized: "Open a .colpkg file exported from Meguri or Anki.", bundle: .module)
        case .startEmpty: String(localized: "Begin with no decks and add your own notes.", bundle: .module)
        }
    }
    /// In place of Start Empty's explanation while it waits for sync work, which
    /// nothing else on the page shows; Set Up Sync opens the section that says what it is.
    static var waitsForSyncReason: String {
        String(localized: "Unavailable while sync is busy. Set Up Sync shows why.", bundle: .module)
    }
}

/// How the welcome screen arranges its heading and its choices (N18.3).
nonisolated enum WelcomeArrangement: Equatable, Sendable {
    /// The heading over the list, in one column.
    case column
    /// An iPhone at regular width, wider than tall: the heading and the list side by side.
    case sides

    /// Shorter than this, a half cannot hold the heading: a large iPhone in landscape keeps the column.
    static let sidesMinimumHeight: CGFloat = 480

    /// The accessibility text sizes keep the column too, where the heading scrolls with the rows.
    static func resolve(phone: Bool, regularWidth: Bool, size: CGSize, accessibilitySize: Bool = false) -> WelcomeArrangement {
        phone && regularWidth && !accessibilitySize && size.width > size.height && size.height >= sidesMinimumHeight
            ? .sides : .column
    }
    /// The top margin that puts rows `rowsHeight` tall in the middle of a half `height` tall.
    /// `inset` is what the system keeps above the first row beside our margin: the row's
    /// top in the list's own content, less the margin in force when it was measured.
    /// Nil, the system's own margin from the top, when there is nothing measured, when
    /// other sections follow the rows, or when the rows leave too little room: then
    /// the list scrolls, and its margin must not follow it.
    static func centredMargin(rowsHeight: CGFloat?, inset: CGFloat, height: CGFloat, followed: Bool,
                              clearance: CGFloat = Spacing.xxLarge) -> CGFloat? {
        guard let rowsHeight, rowsHeight > 0, !followed, height >= rowsHeight + 2 * clearance else { return nil }
        return max(0, (height - rowsHeight) / 2 - inset)
    }
    /// The side margin that keeps a column `readable` wide in the middle of `width`;
    /// nil leaves the system's margin.
    static func columnMargin(width: CGFloat, readable: CGFloat = ContentWidth.readable) -> CGFloat? {
        width > readable ? (width - readable) / 2 : nil
    }
}

/// Home without a collection (N18.3): a welcome, one line, and one grouped list of the ways to start.
struct HomeWelcome: View {
    struct Action {
        var enabled = true
        /// Why a disabled row is unavailable, shown in place of its explanation;
        /// nil where the reason is already on screen (a sheet or flow owns the window).
        var unavailableReason: String? = nil
        var run: () -> Void = {}
    }
    var sync: Action? = nil
    var importFile: Action? = nil
    var startEmpty: Action? = nil
    var error: String? = nil
    var previousAnswerCount: Int? = nil
    #if !os(macOS)
    @Environment(\.shellLayout) private var shellLayout
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var fold = FoldGeometry()
    /// Each row's height, measured only with the rows beside the heading, and what the
    /// system keeps above the first row there.
    @State private var rowHeights: [WelcomeChoice: CGFloat] = [:]
    @State private var systemInset: CGFloat = 0
    #endif

    var body: some View {
        GeometryReader { proxy in
            #if os(macOS)
            column(width: proxy.size.width)
            #else
            let arrangement = WelcomeArrangement.resolve(phone: shellLayout == .compact,
                regularWidth: sizeClass == .regular, size: proxy.size,
                accessibilitySize: dynamicTypeSize.isAccessibilitySize)
            switch arrangement {
            case .column: column(width: proxy.size.width)
            case .sides: sides
            }
            #endif
        }
        // Holds the screen identifier in one place, so the texts keep their own.
        .accessibilityElement(children: .contain)
        #if !os(macOS)
        .onGeometryChange(for: FoldGeometry.self) { FoldGeometry($0) } action: { fold = $0 }
        #endif
    }

    /// Top-aligned; past the readable width the same list keeps a centred column.
    private func column(width: CGFloat) -> some View {
        Form {
            Section {
                choices(measured: false, margin: nil, known: true)
            } header: {
                heading
                    .textCase(nil)
                    .padding(.bottom, Spacing.xSmall)
            }
            notices
        }
        .formStyle(.grouped)
        .contentMargins(.horizontal, WelcomeArrangement.columnMargin(width: width), for: .scrollContent)
        .welcomeBarEdge()
    }

    #if !os(macOS)
    /// Either side of the fold: the heading centred on the leading half, the list on the trailing half.
    private var sides: some View {
        FoldAvoidingRow(fold: fold.acrossWidth, spacing: 0) {
            heading
                .padding(.horizontal, Spacing.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            GeometryReader { half in
                let rowsHeight = rowHeights.isEmpty ? nil : rowHeights.values.reduce(0, +)
                let margin = WelcomeArrangement.centredMargin(rowsHeight: rowsHeight, inset: systemInset,
                    height: half.size.height, followed: hasNotices)
                Form {
                    Section { choices(measured: true, margin: margin, known: rowsHeight != nil) }
                    notices
                }
                .formStyle(.grouped)
                .coordinateSpace(.named(Self.rowsSpace))
                .scrollBounceBehavior(.basedOnSize)
                // A form has no alignment of its own: its top margin puts the rows in the middle.
                .contentMargins(.top, margin, for: .scrollContent)
                .welcomeBarEdge()
            }
        }
        .background(Color(uiColor: .systemGroupedBackground))
    }
    #endif

    private var heading: some View {
        VStack(alignment: .leading, spacing: Spacing.xSmall) {
            Text("Welcome to Meguri", bundle: .module)
                .font(.largeTitle.bold())
                .foregroundStyle(.token(.textPrimary))
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("home.welcome.title")
            Text("Choose how to start. You can change this later in Settings.", bundle: .module)
                .font(.body)
                .foregroundStyle(.token(.textSecondary))
                .accessibilityIdentifier("home.welcome.message")
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private nonisolated static let rowsSpace = "home.welcome.rows"
    private var hasNotices: Bool { error != nil || (previousAnswerCount ?? 0) > 0 }

    private var firstChoice: WelcomeChoice {
        sync != nil ? .sync : importFile != nil ? .importFile : .startEmpty
    }

    @ViewBuilder private func choices(measured: Bool, margin: CGFloat?, known: Bool) -> some View {
        if let sync { row(.sync, sync, measured: measured, margin: margin, known: known) }
        if let importFile { row(.importFile, importFile, measured: measured, margin: margin, known: known) }
        if let startEmpty { row(.startEmpty, startEmpty, measured: measured, margin: margin, known: known) }
    }

    @ViewBuilder private func row(_ choice: WelcomeChoice, _ action: Action, measured: Bool,
                                  margin: CGFloat?, known: Bool) -> some View {
        #if os(macOS)
        WelcomeRow(choice: choice, action: action)
        #else
        if measured {
            WelcomeRow(choice: choice, action: action)
                // A height only: a position changes with every scroll step.
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { rowHeights[choice] = $0 }
                .onGeometryChange(for: CGFloat.self) { $0.frame(in: .named(Self.rowsSpace)).minY } action: { top in
                    // The first row's top, less our margin, is what the system keeps above it.
                    // Read only before the rows are measured and while they are centred: a
                    // centred list fits, so it cannot scroll and move the row under the reading.
                    guard choice == firstChoice, margin != nil || !known else { return }
                    systemInset = max(0, top - (margin ?? 0))
                }
        } else {
            WelcomeRow(choice: choice, action: action)
        }
        #endif
    }

    /// What the page showed above its content before: an error from the last attempt, the last session.
    @ViewBuilder private var notices: some View {
        if let error {
            Section {
                // A symbol with the colour, so colour is not the only cue.
                Label {
                    Text(verbatim: error)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.token(.statusError))
                        .accessibilityHidden(true)
                }
                .accessibilityIdentifier("home.collection.error")
            }
        }
        if let previousAnswerCount, previousAnswerCount > 0 {
            Section {
                Text("\(previousAnswerCount) answers recorded", bundle: .module)
                    .accessibilityIdentifier("home.session.recorded")
            } header: {
                Text("Previous Session", bundle: .module)
            }
        }
    }
}

/// One way to start: a symbol, what it is, what it does, and the mark of a row that leads on.
private struct WelcomeRow: View {
    let choice: WelcomeChoice
    let action: HomeWelcome.Action
    /// The system indicator's grey: inside a button a hierarchical style would follow the tint.
    private var chevronStyle: Color {
        #if os(macOS)
        Color(nsColor: .tertiaryLabelColor)
        #else
        Color(uiColor: .tertiaryLabel)
        #endif
    }

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    private var decorated: Bool { !dynamicTypeSize.isAccessibilitySize }

    /// A disabled row's symbol, title and chevron are dimmed; its reason line is not,
    /// so the reason stays readable.
    private var dimming: Double { action.enabled ? 1 : 0.5 }
    /// The reason replaces the explanation only while the row is disabled.
    private var secondaryLine: String {
        action.enabled ? choice.explanation : action.unavailableReason ?? choice.explanation
    }
    private var showsReason: Bool { !action.enabled && action.unavailableReason != nil }

    private var text: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: choice.title)
                .foregroundStyle(.token(.textPrimary))
                .opacity(dimming)
            Text(verbatim: secondaryLine)
                .font(.subheadline)
                .foregroundStyle(.token(.textSecondary))
                .opacity(showsReason ? 1 : dimming)
        }
        // Rows grow with the text size rather than cut it.
        .fixedSize(horizontal: false, vertical: true)
    }

    var body: some View {
        Group {
            if action.enabled {
                Button(action: action.run) { content }
                #if os(macOS)
                .buttonStyle(.plain)
                #endif
            } else {
                // Not a button while unavailable: the Mac's disabled button style dims all
                // of its content, the reason line too. It is still read as a dimmed button.
                content
                    .accessibilityElement(children: .ignore)
                    .accessibilityAddTraits(.isButton)
            }
        }
        .disabled(!action.enabled)
        // The title names the row; the explanation, or the reason, is its hint.
        .accessibilityLabel(Text(verbatim: choice.title))
        .accessibilityHint(Text(verbatim: secondaryLine))
        .accessibilityIdentifier(choice.identifier)
    }

    private var content: some View {
        HStack(spacing: Spacing.small) {
            // At the accessibility text sizes the text takes the row: a label there wraps
            // each text around its symbol, and the state views leave their symbols out too.
            if decorated {
                Label { text } icon: {
                    // The title names the row; the symbol is decoration.
                    Image(systemName: choice.systemImage)
                        .opacity(dimming)
                        .accessibilityHidden(true)
                }
            } else {
                text
            }
            Spacer(minLength: 0)
            if decorated {
                // Drawn: these rows act in place, so the system adds no disclosure indicator of its own.
                Image(systemName: "chevron.forward")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(chevronStyle)
                    .opacity(dimming)
                    .accessibilityHidden(true)
            }
        }
        .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
        .contentShape(Rectangle())
    }
}

private extension View {
    /// An opaque navigation bar in the page's own colour: nothing shows at rest, and rows
    /// that scroll under it are covered. Under the system's translucent bar the contrast
    /// audit failed a row title seen through the bar, at the largest text size in dark.
    @ViewBuilder func welcomeBarEdge() -> some View {
        #if os(macOS)
        self
        #else
        toolbarBackground(Color(uiColor: .systemGroupedBackground), for: .navigationBar)
            .toolbarBackgroundVisibility(.visible, for: .navigationBar)
        #endif
    }
}
