import re
A="app/Packages/MeguriKit/Sources/"
def sub(path, old, new):
    s=open(path).read(); assert old in s,(path,old[:80]); open(path,"w").write(s.replace(old,new,1))
def cut(path, start, end, new=""):
    s=open(path).read(); a=s.index(start); b=s.index(end,a); open(path,"w").write(s[:a]+new+s[b:])

open(A+"DesignSystem/Components/FoldAwareLayout.swift","w").write('''import SwiftUI

/// Where a foldable iPhone's fold falls in a view (R9), from the system's
/// reserved regions. These include a fold that isn't active, so controls keep
/// one place in every position (`native-interface-contract.md` N13).
public struct FoldGeometry: Equatable, Sendable {
    /// A fold running top to bottom: the x range it takes in the view.
    public var acrossWidth: ClosedRange<CGFloat>?
    /// A fold running side to side: the y range it takes in the view.
    public var acrossHeight: ClosedRange<CGFloat>?

    public var exists: Bool { acrossWidth != nil || acrossHeight != nil }

    public init(acrossWidth: ClosedRange<CGFloat>? = nil, acrossHeight: ClosedRange<CGFloat>? = nil) {
        self.acrossWidth = acrossWidth
        self.acrossHeight = acrossHeight
    }

    public init(frames: [CGRect]) {
        for frame in frames {
            if frame.height >= frame.width {
                acrossWidth = frame.minX...frame.maxX
            } else {
                acrossHeight = frame.minY...frame.maxY
            }
        }
    }

    public init(_ proxy: GeometryProxy) {
        #if os(iOS)
        self.init(frames: proxy.reservedRegions(kind: .division).map(\\.frame))
        #else
        self.init()
        #endif
    }
}

/// Two columns that stay off a fold running through them: each side of the
/// fold when there is one, otherwise two equal columns.
public struct FoldAvoidingRow: Layout {
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
        let width = proposal.width ?? subviews.reduce(spacing) { $0 + $1.sizeThatFits(.unspecified).width }
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
''')

# Deck menu: a long press on touch (N13).
M=A+"Features/Home/DeckManagement/DeckMenu.swift"
sub(M,'''        if let collection {
            HStack {
                content
                Menu { DeckMenu(collection: collection, deck: deck) } label: {''','''        if let collection {
            #if os(macOS)
            HStack {
                content
                Menu { DeckMenu(collection: collection, deck: deck) } label: {''')
sub(M,'''            .contextMenu { DeckMenu(collection: collection, deck: deck) }
            .onHover { hovering = $0 }
        } else { content }''','''            .contextMenu { DeckMenu(collection: collection, deck: deck) }
            .onHover { hovering = $0 }
            #else
            content.contextMenu { DeckMenu(collection: collection, deck: deck) }
            #endif
        } else { content }''')
s=open(M).read()
s=s.replace('''                .focused($menuFocused)
                .accessibilityIdentifier("home.deckMenu.open.\\(deck.id)")
                #if os(macOS)
                .menuStyle(.button)
                .buttonStyle(.borderless)
                .opacity(hovering || menuFocused ? 1 : 0)
                .help(Text("Deck Actions", bundle: .module))
                #endif
''','''                .focused($menuFocused)
                .accessibilityIdentifier("home.deckMenu.open.\\(deck.id)")
                .menuStyle(.button)
                .buttonStyle(.borderless)
                .opacity(hovering || menuFocused ? 1 : 0)
                .help(Text("Deck Actions", bundle: .module))
''')
open(M,"w").write(s)

H=A+"Features/Home/Home.swift"
sub(H,'''    @FocusState private var homeFocused: Bool
    #endif
    public init(collection''','''    @FocusState private var homeFocused: Bool
    #else
    @Environment(\\.horizontalSizeClass) private var sizeClass
    @Environment(\\.shellLayout) private var shellLayout
    #endif
    public init(collection''')
cut(H,'''                #if os(macOS)
                if let deck = macDeckPage {''','''            }
        }
        .navigationTitle(deckID == nil''','''                if let deck = deckPageDetail {
                    // Keeps the screen's and the page's identifiers on separate elements.
                    GeometryReader { _ in deckPage(deck) }
                } else {
                    #if os(macOS)
                    GeometryReader { geometry in
                        scrollContent(availableHeight: geometry.size.height)
                    }
                    #else
                    scrollContent()
                    #endif
                }
''')
sub(H,'''            ToolbarItem(placement: .primaryAction) {
                if collection.review == nil, collection.navigation.review == nil {
                    Button { collection.presentGoToDeck() } label: {
                        Text("Go to Deck…", bundle: .module)
                    }''','''            ToolbarItem(placement: .primaryAction) {
                if collection.review == nil, collection.navigation.review == nil, showsCollectionActions {
                    Button { collection.presentGoToDeck() } label: {
                        Label { Text("Go to Deck…", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                    }''')
sub(H,'''            ToolbarItem(placement: .primaryAction) {
                Button { collection.deckActions.present(.create) } label: {
                    Text("Create Deck", bundle: .module)
                }
                .disabled(!collection.deckActions.canPresent(.create))
                .accessibilityIdentifier("home.collection.createDeck")
            }''','''            ToolbarItem(placement: .primaryAction) {
                if showsCollectionActions {
                    Button { collection.deckActions.present(.create) } label: {
                        Label { Text("Create Deck", bundle: .module) } icon: { Image(systemName: "plus") }
                    }
                    .disabled(!collection.deckActions.canPresent(.create))
                    .accessibilityIdentifier("home.collection.createDeck")
                }
            }''')
sub(H,'''    #if os(macOS)
    private var homeKeyboardActive: Bool {''','''    #if !os(macOS)
    /// On an unfolded foldable the deck list beside the page already has them (N13).
    private var showsCollectionActions: Bool {
        !(deckID != nil && shellLayout == .compact && sizeClass == .regular)
    }
    #endif
    #if os(macOS)
    private var homeKeyboardActive: Bool {''')
cut(H,'''            if let deckID, let deck = collection.deckPage(deckID) {
                #if !os(macOS)''','''                EmptyStateView(Text("No Decks", bundle: .module), systemImage: "rectangle.stack",''','''            if deckPageDetail == nil, collection.decks.isEmpty {
''')
s=open(H).read()
s=s.replace('''    #if os(macOS)
    private var macDeckPage: ReviewCollection.DeckPageDetail? {''','''    private var deckPageDetail: ReviewCollection.DeckPageDetail? {''')
s=s.replace('''            .accessibilityIdentifier("home.deck.counts.\\(id)")
    }
    #endif
}''','''            .accessibilityIdentifier("home.deck.counts.\\(id)")
    }
}''')
s=s.replace('''                Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                    Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
                }
                .buttonStyle(.borderedProminent)
                .frame(minHeight: ControlMetrics.minimumTarget)''','''                Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                    #if os(macOS)
                    Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
                    #else
                    Text("Study", bundle: .module)
                    #endif
                }
                .buttonStyle(.borderedProminent)
                .frame(minHeight: ControlMetrics.minimumTarget)''')
s=s.replace('''        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.deck.page.\\(deck.id)")
    }
    private func countText''','''        #if !os(macOS)
        .listStyle(.plain)
        #endif
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.deck.page.\\(deck.id)")
    }
    private func countText''')
# Decks root: no list header on touch either (N12.1, N13).
s=s.replace('''                    #if os(macOS)
                    ForEach(nodes, id: \\.id) { node in
                        HomeDeckBranch(node: node, collection: collection, ancestors: [], depth: 0, select: select,''','''                    ForEach(nodes, id: \\.id) { node in
                        HomeDeckBranch(node: node, collection: collection, ancestors: [], depth: 0, select: select,''',1)
a=s.index('                    #else\n                    Section {\n                        ForEach(nodes, id: \\.id) { node in')
b=s.index('                    #endif\n',a)+len('                    #endif\n')
s=s[:a]+s[b:]
# Row: the name and one quiet number.
a=s.index('                VStack(alignment: .leading) {\n                    Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)\n                        .font(.body)')
b=s.index('                #endif',a)
s=s[:a]+'''                Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                    .accessibilityIdentifier("home.deck.name.\\(node.id)")
                Spacer()
                if node.counts.total > 0 {
                    Text(node.counts.total, format: .number)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                        .accessibilityIdentifier("home.deck.rowCounts.\\(node.id)")
                }
'''+s[b:]
a=s.index('    private var subtitle: AttributedString? {')
b=s.index('public struct DeckCounts: View {')
s=s[:a].rstrip()+'\n}\n\n'+s[b:]
s=s.replace('''            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(verbatim: name)
                DeckCounts(counts: node.counts)
            }
            .foregroundStyle(.token(.textPrimary))''','''            HStack {
                Text(verbatim: name)
                Spacer()
                if node.counts.total > 0 {
                    Text(node.counts.total, format: .number)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            .foregroundStyle(.token(.textPrimary))''')
s=s.replace('public struct DeckCounts: View {','''private extension DeckCardCounts {
    /// Everything left to study today, as one number (N13).
    var total: UInt32 { new + learning + review }
}

public struct DeckCounts: View {''')
open(H,"w").write(s)

R=A+"Features/Review/Review.swift"
cut(R,'''    private var touchBody: some View {
        VStack(spacing: 0) {
            header''','''    #endif
    /// The card under any notice''','''    /// System bars hold the corners, so they move to the side on a foldable (R10).
    /// One container in every position keeps the card's page alive through a fold (R576).
    private var touchBody: some View {
        NavigationStack {
            VStack(spacing: 0) {
                let lowerHalf = DuoPortrait.review == "halves" ? fold.acrossHeight : nil
                VStack(spacing: Spacing.xSmall) {
                    HStack(spacing: Spacing.small) {
                        sessionProgress
                        timer
                    }
                    .padding(.horizontal, Spacing.medium)
                    cardRegion
                }
                .frame(height: lowerHalf?.lowerBound)
                if let lowerHalf {
                    Color.clear.frame(height: lowerHalf.upperBound - lowerHalf.lowerBound)
                }
                ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate)
                    .frame(maxHeight: lowerHalf == nil ? nil : .infinity, alignment: .bottom)
            }
            .onGeometryChange(for: FoldGeometry.self) { FoldGeometry($0) } action: { fold = $0 }
            .background(.token(.cardPage))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(action: close) {
                        Label { Text("Close Review", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .accessibilityIdentifier("review.session.close")
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    Button { if canUndo { undo() } } label: {
                        Label { Text(verbatim: undoDescription) } icon: { Image(systemName: "arrow.uturn.backward") }
                    }
                    .disabled(!canUndo)
                    .accessibilityIdentifier("review.session.back")
                    if canRedo {
                        Button { if canRedo { redo() } } label: {
                            Label { Text("Redo Answer", bundle: .module) } icon: { Image(systemName: "arrow.uturn.forward") }
                        }
                        .accessibilityIdentifier("review.session.redo")
                    }
                }
                if let status {
                    ToolbarItem(placement: .primaryAction) { status }
                }
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
    }
''')
cut(R,'''    @ViewBuilder private var header: some View {''','''    private var sessionProgress: some View {''')
cut(R,'''    private var backButton: some View {''','''    @ViewBuilder let card''' if False else '''}

/// One reserved bottom region''','')
sub(R,'''    @State private var noticeContentHeight: CGFloat?
''','''    @State private var noticeContentHeight: CGFloat?
    @State private var fold = FoldGeometry()
''')
sub(R,'''    public let rate: (ReviewRating) -> Void
    public init(phase''','''    public let rate: (ReviewRating) -> Void
    #if !os(macOS)
    @State private var fold = FoldGeometry()
    #endif
    public init(phase''')
a_marker='''    @ViewBuilder private var touchBar: some View {'''
s=open(R).read()
a=s.index(a_marker)
b=s.index('    public static func interval(_ label: ReviewAnswerLabel) -> String {')
s=s[:a]+'''    #if !os(macOS)
    /// The answers either side of a fold running through the row (R9, N13).
    @ViewBuilder private var touchBar: some View {
        if phase == .question || phase == .answer || phase == .transition {
            ZStack {
                // Kept in layout on both sides, including at large text sizes.
                if phase == .answer {
                    ratings.allowsHitTesting(enabled)
                } else {
                    ratings.hidden()
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                if phase == .question {
                    if dynamicTypeSize.isAccessibilitySize {
                        revealButton
                    } else {
                        FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                            Color.clear.frame(height: 0)
                            revealButton
                        }
                    }
                }
            }
            .onGeometryChange(for: FoldGeometry.self) { FoldGeometry($0) } action: { fold = $0 }
            .padding(Spacing.small)
            .frame(maxWidth: fold.acrossWidth == nil ? ContentWidth.readable : nil)
            .frame(maxWidth: .infinity)
            .fixedSize(horizontal: false, vertical: DuoPortrait.review != "halves")
            .layoutPriority(1)
        }
    }
    private var revealButton: some View {
        Button(action: reveal) {
            Text("Show Answer", bundle: .module)
                .font(.subheadline.weight(.semibold))
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.roundedRectangle)
        .tint(ColorToken.textPrimary.color)
        .disabled(!enabled)
        .accessibilityIdentifier("review.session.reveal")
    }
    @ViewBuilder private var ratings: some View {
        let again = rating(.again, title: "Again", label: states?.again.label, color: .ratingAgain)
        let hard = rating(.hard, title: "Hard", label: states?.hard.label, color: .textPrimary)
        let good = rating(.good, title: "Good", label: states?.good.label, color: .ratingGood)
        let easy = rating(.easy, title: "Easy", label: states?.easy.label, color: .textPrimary)
        if dynamicTypeSize.isAccessibilitySize || DuoPortrait.review == "halves" && fold.acrossHeight != nil {
            VStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        } else if DuoPortrait.review == "grid", fold.acrossHeight != nil {
            Grid(horizontalSpacing: Spacing.xSmall, verticalSpacing: Spacing.xSmall) {
                GridRow { again; hard }
                GridRow { good; easy }
            }
        } else {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                HStack(spacing: Spacing.xSmall) { again; hard }
                HStack(spacing: Spacing.xSmall) { good; easy }
            }
        }
    }
    private func rating(_ rating: ReviewRating, title: String, label: ReviewAnswerLabel?, color: ColorToken) -> some View {
        Button { rate(rating) } label: {
            Group {
                if dynamicTypeSize.isAccessibilitySize {
                    HStack(spacing: Spacing.small) {
                        Text(LocalizedStringKey(title), bundle: .module).font(.headline)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: Spacing.small)
                        Text(verbatim: label.map(Self.interval) ?? "—").font(.caption).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                } else {
                    VStack(spacing: 0) {
                        Text(LocalizedStringKey(title), bundle: .module).font(.subheadline.weight(.semibold))
                            .fixedSize(horizontal: false, vertical: true)
                        Text(verbatim: label.map(Self.interval) ?? "—").font(.caption).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .multilineTextAlignment(.center)
                }
            }
            .foregroundStyle(.token(color))
            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
            .fixedSize(horizontal: false, vertical: true)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.roundedRectangle)
        .disabled(!enabled || phase != .answer)
        .accessibilityLabel("\\(title), \\(label.map(Self.interval) ?? "")")
        .accessibilityIdentifier("review.session.rate.\\(ReviewCoordinator.name(rating))")
    }
    #endif
'''+s[b:]
s=s.replace('''struct ReviewLayout<Card: View>: View {''','''/// Preview only, removed before commit: the portrait inner display options.
enum DuoPortrait {
    static var review: String { UserDefaults.standard.string(forKey: "DuoPortraitReview") ?? "" }
}

struct ReviewLayout<Card: View>: View {''',1)
open(R,"w").write(s)

P="app/Meguri/Shell/RootView.swift"
sub(P,"import CoreBridge\nimport SwiftUI","import CoreBridge\nimport DesignSystem\nimport SwiftUI")
sub(P,'''    @Environment(\\.horizontalSizeClass) private var sizeClass
    #else''','''    @Environment(\\.horizontalSizeClass) private var sizeClass
    /// A foldable keeps the tab layout when unfolded (N13). iPad has no fold.
    @State private var hasFold = false
    #else''')
sub(P,'''        sizeClass == .compact ? .compact : .sidebar
''','''        sizeClass == .compact || hasFold ? .compact : .sidebar
''')
sub(P,'''        #if os(macOS)
        .focusedSceneValue(\\.nativeCommandContext''','''        #if os(iOS)
        .onGeometryChange(for: Bool.self) { FoldGeometry($0).exists } action: { hasFold = $0 }
        #endif
        #if os(macOS)
        .focusedSceneValue(\\.nativeCommandContext''')

C="app/Meguri/Shell/CompactRootView.swift"
sub(C,'''    let configuration: LaunchConfiguration

    var body: some View {''','''    let configuration: LaunchConfiguration
    @Environment(\\.horizontalSizeClass) private var sizeClass
    @State private var isUpright = false
    private var portraitDecks: String { UserDefaults.standard.string(forKey: "DuoPortraitDecks") ?? "" }

    var body: some View {''')
sub(C,'''        .fullScreenCover(item:''','''        .onGeometryChange(for: Bool.self) { $0.size.height > $0.size.width } action: { isUpright = $0 }
        .fullScreenCover(item:''')
sub(C,'''    private func stack(_ tab: CompactTab) -> some View {
        NavigationStack(path:''','''    /// Unfolded, Decks shows the list beside the deck; everything else keeps its stack (N13).
    @ViewBuilder private func stack(_ tab: CompactTab) -> some View {
        if tab == .decks, sizeClass == .regular, !(isUpright && portraitDecks == "stack") {
            if isUpright, portraitDecks == "halves" {
                VStack(spacing: 0) {
                    decksList.frame(maxHeight: .infinity)
                    Divider()
                    deckDetail.frame(maxHeight: .infinity)
                }
            } else {
                NavigationSplitView(columnVisibility: .constant(.doubleColumn)) {
                    decksList
                        .navigationSplitViewColumnWidth(min: 280, ideal: isUpright ? 320 : 475, max: 475)
                } detail: {
                    deckDetail
                }
                .navigationSplitViewStyle(.balanced)
            }
        } else {
            singleStack(tab)
        }
    }

    private var decksList: some View {
        NavigationStack {
            SectionDestinationView(screen: .root(.home), navigation: navigation, features: features)
                .toolbar { ToolbarItem(placement: .primaryAction) { features.syncStatus() } }
        }
    }

    private var deckDetail: some View {
        NavigationStack(path: Binding(get: { navigation.path(.home) }, set: { navigation.setPath($0, for: .home) })) {
            Group {
                if let deck = navigation.homeDeckPage {
                    SectionDestinationView(screen: .deckPage(deck), navigation: navigation, features: features)
                } else {
                    ContentUnavailableView("Select a Deck", systemImage: "rectangle.stack")
                }
            }
            .navigationDestination(for: Destination.self) { destination in
                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
            }
        }
    }

    private func singleStack(_ tab: CompactTab) -> some View {
        NavigationStack(path:''')
