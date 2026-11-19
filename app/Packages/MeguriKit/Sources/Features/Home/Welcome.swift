import AppModel
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The three ways to start without a collection (N18.1), in the order the screen lists them.
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
}

/// How the welcome screen arranges its heading and its choices (N18.1).
nonisolated enum WelcomeArrangement: Equatable, Sendable {
    /// The heading over the list, in one column.
    case column
    /// An iPhone at regular width, wider than tall: the heading and the list side by side.
    case sides

    static func resolve(phone: Bool, regularWidth: Bool, size: CGSize) -> WelcomeArrangement {
        phone && regularWidth && size.width > size.height ? .sides : .column
    }
    /// The rows' extent down the screen, first top to last bottom.
    static func rowsSpan(_ frames: some Collection<CGRect>) -> ClosedRange<CGFloat>? {
        guard let top = frames.map(\.minY).min(), let bottom = frames.map(\.maxY).max(), bottom > top else { return nil }
        return top...bottom
    }
    /// The top margin that puts `rows` in the middle of `visible`, from the margin that put
    /// them where they are. Nil, the system's own margin, when they leave too little room
    /// to centre without scrolling: then the list starts at the top.
    static func centredOffset(current: CGFloat?, rows: ClosedRange<CGFloat>?, visible: ClosedRange<CGFloat>,
                              clearance: CGFloat = Spacing.xxLarge) -> CGFloat? {
        guard let rows else { return current }
        let height = rows.upperBound - rows.lowerBound
        guard visible.upperBound - visible.lowerBound >= height + 2 * clearance else { return nil }
        let wanted = (visible.lowerBound + visible.upperBound - height) / 2
        // Whether or not the system keeps an inset of its own above ours, the next measurement corrects the rest.
        let next = max(0, (current ?? 0) + wanted - rows.lowerBound)
        if let current, abs(next - current) < 0.5 { return current }
        return next
    }
    /// The side margin that keeps a column `readable` wide in the middle of `width`;
    /// nil leaves the system's margin.
    static func columnMargin(width: CGFloat, readable: CGFloat = ContentWidth.readable) -> CGFloat? {
        width > readable ? (width - readable) / 2 : nil
    }
}

/// Home without a collection (N18.1): a welcome, one line, and one grouped list of the ways to start.
struct HomeWelcome: View {
    struct Action {
        var enabled = true
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
    @State private var fold = FoldGeometry()
    /// Where each row sits on screen, and the top margin that centres them on the trailing half.
    @State private var rowFrames: [WelcomeChoice: CGRect] = [:]
    @State private var rowsOffset: CGFloat?
    #endif

    var body: some View {
        GeometryReader { proxy in
            #if os(macOS)
            column(width: proxy.size.width)
            #else
            let arrangement = WelcomeArrangement.resolve(phone: shellLayout == .compact,
                regularWidth: sizeClass == .regular, size: proxy.size)
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
                choices
            } header: {
                heading
                    .textCase(nil)
                    .padding(.bottom, Spacing.xSmall)
            }
            notices
        }
        .formStyle(.grouped)
        .contentMargins(.horizontal, WelcomeArrangement.columnMargin(width: width), for: .scrollContent)
    }

    #if !os(macOS)
    /// Either side of the fold: the heading centred on the leading half, the list on the trailing half.
    private var sides: some View {
        FoldAvoidingRow(fold: fold.acrossWidth, spacing: 0) {
            heading
                .padding(.horizontal, Spacing.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            GeometryReader { half in
                let visible = half.frame(in: .global)
                Form {
                    Section { choices }
                    notices
                }
                .formStyle(.grouped)
                .scrollBounceBehavior(.basedOnSize)
                // A form has no alignment of its own: its top margin puts the rows in the middle.
                .contentMargins(.top, rowsOffset, for: .scrollContent)
                .onChange(of: WelcomeArrangement.rowsSpan(rowFrames.values), initial: true) { _, rows in
                    rowsOffset = WelcomeArrangement.centredOffset(current: rowsOffset, rows: rows,
                        visible: visible.minY...visible.maxY)
                }
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

    @ViewBuilder private var choices: some View {
        if let sync { row(.sync, sync) }
        if let importFile { row(.importFile, importFile) }
        if let startEmpty { row(.startEmpty, startEmpty) }
    }

    private func row(_ choice: WelcomeChoice, _ action: Action) -> some View {
        WelcomeRow(choice: choice, action: action)
            #if !os(macOS)
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { rowFrames[choice] = $0 }
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

    private var text: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            Text(verbatim: choice.title)
                .foregroundStyle(.token(.textPrimary))
            Text(verbatim: choice.explanation)
                .font(.subheadline)
                .foregroundStyle(.token(.textSecondary))
        }
        // Rows grow with the text size rather than cut it.
        .fixedSize(horizontal: false, vertical: true)
    }

    var body: some View {
        Button(action: action.run) {
            HStack(spacing: Spacing.small) {
                Label { text } icon: {
                    // The title names the row; the symbol is decoration.
                    Image(systemName: choice.systemImage)
                        .accessibilityHidden(true)
                }
                Spacer(minLength: 0)
                // Drawn: these rows act in place, so the system adds no disclosure indicator of its own.
                Image(systemName: "chevron.forward")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(chevronStyle)
                    .accessibilityHidden(true)
            }
            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
            .contentShape(Rectangle())
            .opacity(action.enabled ? 1 : 0.5)
        }
        #if os(macOS)
        .buttonStyle(.plain)
        #endif
        .disabled(!action.enabled)
        // The title names the row; the explanation is its hint.
        .accessibilityLabel(Text(verbatim: choice.title))
        .accessibilityHint(Text(verbatim: choice.explanation))
        .accessibilityIdentifier(choice.identifier)
    }
}
