import re,json
A="app/Packages/MeguriKit/Sources/"
def edit(path, pairs):
    s=open(path).read()
    for old,new in pairs:
        assert old in s,(path,old[:80])
        s=s.replace(old,new,1)
    open(path,"w").write(s)

F=A+"DesignSystem/Components/FoldAwareLayout.swift"
s=open(F).read()
a=s.index("/// Where a foldable iPhone's fold falls"); b=s.index("/// Two columns that stay off a fold")
s=s[:a]+'''/// Where a foldable iPhone's fold runs top to bottom through a view (R9), from
/// the system's reserved regions. `.includeInactive` keeps a fully open fold in
/// the answer, so controls keep one place in every position (N13).
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
            self.init(frames: proxy.reservedRegions(kind: .division, options: .includeInactive).map(\\.frame))
        } else {
            self.init()
        }
        #else
        self.init()
        #endif
    }
}

'''+s[b:]
s=s.replace('''        let width = proposal.width ?? subviews.reduce(spacing) { $0 + $1.sizeThatFits(.unspecified).width }
''','''        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil }
            ?? subviews.reduce(spacing) { $0 + $1.sizeThatFits(.unspecified).width }
''')
open(F,"w").write(s)
T="app/Packages/MeguriKit/Tests/DesignSystemTests/FoldAwareLayoutTests.swift"
s=open(T).read()
a=s.index("    @Test func aFoldRunningTopToBottomSplitsTheWidth()"); b=s.index("    @Test func columnsStopAtTheFold()")
s=s[:a]+'''    @Test func aFoldRunningTopToBottomSplitsTheWidth() {
        let fold = FoldGeometry(frames: [CGRect(x: 455, y: 0, width: 40, height: 669)])
        #expect(fold.acrossWidth == 455...495)
    }

    @Test func aFoldRunningSideToSideLeavesARowWhole() {
        let fold = FoldGeometry(frames: [CGRect(x: 0, y: 455, width: 669, height: 40)])
        #expect(fold.acrossWidth == nil)
    }

    @Test func noRegionsMeansNoFold() {
        #expect(FoldGeometry(frames: []).acrossWidth == nil)
    }

'''+s[b:]
open(T,"w").write(s)

P="app/Meguri/Shell/RootView.swift"
edit(P,[("import CoreBridge\nimport DesignSystem\nimport SwiftUI","import CoreBridge\nimport SwiftUI"),
('''    /// A foldable keeps the tab layout when unfolded (N13). iPad has no fold.
    @State private var hasFold = false
''',''),
('''        sizeClass == .compact || hasFold ? .compact : .sidebar
''','''        // An iPhone keeps the tab layout at any width, unfolded included (R3, N13).
        UIDevice.current.userInterfaceIdiom == .phone || sizeClass == .compact ? .compact : .sidebar
'''),
('''        #if os(iOS)
        .onGeometryChange(for: Bool.self) { FoldGeometry($0).exists } action: { hasFold = $0 }
        #endif
''',''),
('''CompactRootView(navigation: navigation, features: features, configuration: configuration)''','''CompactRootView(navigation: navigation, features: features, configuration: configuration,
                        hasDecks: collection?.decks.isEmpty == false)''')])

C="app/Meguri/Shell/CompactRootView.swift"
edit(C,[('''    let configuration: LaunchConfiguration
    @Environment''','''    let configuration: LaunchConfiguration
    let hasDecks: Bool
    @Environment'''),
('''                decksList
                    .navigationSplitViewColumnWidth(min: 280, ideal: 475, max: 475)
''','''                decksList
                    .navigationSplitViewColumnWidth(min: 280, ideal: 475, max: 475)
                    .toolbar(removing: .sidebarToggle)
'''),
('''                } else {
                    ContentUnavailableView("Select a Deck", systemImage: "rectangle.stack")
                }
            }
            .navigationDestination''','''                } else if hasDecks {
                    ContentUnavailableView("Select a Deck", systemImage: "rectangle.stack")
                }
            }
            .navigationDestination'''),
('''                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
            }
        }
    }

    private func singleStack''','''                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
            }
        }
        // A new deck starts from its own top, as a new sidebar selection does.
        .id(navigation.homeDeckPage)
    }

    private func singleStack''')])

R=A+"Features/Review/Review.swift"
edit(R,[('''    @Environment(\\.appClock) var clock
    @Environment(\\.dynamicTypeSize) private var dynamicTypeSize
    @State private var noticeContentHeight''','''    @Environment(\\.appClock) var clock
    @State private var noticeContentHeight'''),
('''                        Label { Text("Close Review", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .accessibilityIdentifier("review.session.close")''','''                        Label { Text("Close Review", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .help(Text("Close Review", bundle: .module))
                    .accessibilityIdentifier("review.session.close")'''),
('''                    .disabled(!canUndo)
                    .accessibilityIdentifier("review.session.back")''','''                    .disabled(!canUndo)
                    .help(undoDescription)
                    .accessibilityIdentifier("review.session.back")'''),
('''                        .accessibilityIdentifier("review.session.redo")''','''                        .help(Text("Redo Answer", bundle: .module))
                        .accessibilityIdentifier("review.session.redo")'''),
('''public struct ReviewAnswerBar: View {
    @Environment(\\.dynamicTypeSize) private var dynamicTypeSize
''','''public struct ReviewAnswerBar: View {
    #if !os(macOS)
    @Environment(\\.dynamicTypeSize) private var dynamicTypeSize
    #endif
'''),
('''                if phase == .question {
                    if dynamicTypeSize.isAccessibilitySize {
                        revealButton
                    } else {''','''                if phase == .question {
                    if dynamicTypeSize.isAccessibilitySize, fold.acrossWidth == nil {
                        revealButton
                    } else {'''),
('''        if dynamicTypeSize.isAccessibilitySize {
            VStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        } else {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                HStack(spacing: Spacing.xSmall) { again; hard }
                HStack(spacing: Spacing.xSmall) { good; easy }
            }
        }''','''        if dynamicTypeSize.isAccessibilitySize, fold.acrossWidth == nil {
            VStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        } else if dynamicTypeSize.isAccessibilitySize {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                VStack(spacing: Spacing.xSmall) { again; hard }
                VStack(spacing: Spacing.xSmall) { good; easy }
            }
        } else {
            FoldAvoidingRow(fold: fold.acrossWidth, spacing: Spacing.xSmall) {
                HStack(spacing: Spacing.xSmall) { again; hard }
                HStack(spacing: Spacing.xSmall) { good; easy }
            }
        }''')])

M=A+"Features/Home/DeckManagement/DeckMenu.swift"
edit(M,[('''    let deck: CountedDeckNode
    @State private var hovering = false
    @FocusState private var menuFocused: Bool
''','''    let deck: CountedDeckNode
    #if os(macOS)
    @State private var hovering = false
    @FocusState private var menuFocused: Bool
    #endif
''')])

H=A+"Features/Home/Home.swift"
s=open(H).read()
def rep(old,new):
    global s
    assert old in s,old[:80]; s=s.replace(old,new,1)
rep('''                            select: { collection.openDeckPage(deckID: $0) },
                            isExpanded: { collection.studyExpanded(deckID: $0) },
                            isExpansionPending: { collection.isStudyExpansionPending(deckID: $0) },
                            setExpanded: { id, expanded, descendants in
                                collection.setStudyExpanded(deckID: id, expanded: expanded, includingDescendants: descen''' if False else '''                            select: { collection.openDeckPage(deckID: $0) },''','''                            select: openSubdeck,''')
rep('''    private func countText(_ text: LocalizedStringKey''','''    /// A stack pushes the subdeck, so Back returns to its parent; the sidebar selects it.
    private func openSubdeck(_ id: Int64) {
        #if !os(macOS)
        if shellLayout == .compact {
            let navigation = collection.navigation
            navigation.setPath(navigation.path(.home) + [.deckPage(DeckID(id))], for: .home)
            return
        }
        #endif
        collection.openDeckPage(deckID: id)
    }
    private func countText(_ text: LocalizedStringKey''')
rep('''            .padding(.vertical)
            .listRowSeparator(.hidden)
            if !subdecks.isEmpty {''','''            .padding(.vertical)
            .listRowSeparator(.hidden)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.deck.summary")
            if !subdecks.isEmpty {''')
rep('''                } header: { Text("Subdecks", bundle: .module) }''','''                } header: {
                    Text("Subdecks", bundle: .module)
                        .accessibilityIdentifier("home.deck.subdecksHeader")
                }''')
rep('''                } header: { Text("Previous Session", bundle: .module) }''','''                } header: {
                    Text("Previous Session", bundle: .module)
                        .accessibilityIdentifier("home.deck.previousSessionHeader")
                }''')
# Sidebar row: drop the white block; keep all three counts for VoiceOver.
rep('''            .listRowBackground(ColorToken.surface.color)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(Text(verbatim: path))
''','''            .accessibilityElement(children: .combine)
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(Text("\\(Text(node.counts.new, format: .number)) new · \\(Text(node.counts.learning, format: .number)) learning · \\(Text(node.counts.review, format: .number)) due", bundle: .module))
''')
# DeckCounts has no callers left.
a=s.index('public struct DeckCounts: View {'); b=s.index('public struct HomeSidebarDecks')
s=s[:a]+s[b:]
# Mac only: the row's plain counts.
rep('''    /// N11.5: non-zero counts as trailing secondary text, without colours.
    private var plainCounts''','''    #if os(macOS)
    /// N11.5: non-zero counts as trailing secondary text, without colours.
    private var plainCounts''')
open(H,"w").write(s)
m=re.search(r'private var plainCounts: String\? \{.*?\n    \}\n',s,re.S)
s=s[:m.end()]+'    #endif\n'+s[m.end():]
open(H,"w").write(s)

U="app/MeguriUITests/LocalReviewJourneyTests.swift"
edit(U,[('''    private func waitForDeckCounts(_ app: XCUIApplication, due: Int) -> Bool {
        #if os(macOS)
        var ready = true''','''    private func waitForDeckCounts(_ app: XCUIApplication, due: Int) -> Bool {
        var ready = true'''),
('''        return ready
        #else
        return waitForText(element(app, "home.deck.counts"), "0 new · 0 learning · \\(due) due")
        #endif
    }''','''        return ready
    }''')])
E="app/MeguriUITests/CollectionEntryJourneyTests.swift"
edit(E,[('''        let counts = deckPage.descendants(matching: .any)["home.deck.counts"].firstMatch
        let before = text(counts)
''','''        let counts = ["new", "learning", "due"].map { deckPage.descendants(matching: .any)["home.deck.counts.\\($0)"].firstMatch }
        let before = counts.map(text)
'''),
('''        let after = text(counts)
''','''        let after = counts.map(text)
'''),
('''        XCTAssertTrue(waitForText(counts, after))
''','''        for (count, value) in zip(counts, after) { XCTAssertTrue(waitForText(count, value)) }
'''),
('''        XCTAssertFalse(before.isEmpty)
''','''        XCTAssertFalse(before.contains(""))
''')])
HT="app/MeguriUITests/HomeTests.swift"
edit(HT,[('''        XCTAssertTrue(app.descendants(matching: .any)["home.collection.screen"].waitForExistence(timeout: 10))
''','''        let page = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "home.deck.page."))
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "count == 0"), object: page)
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 10), .completed)
''')])
D="tools/mg.d/doctor.sh"
edit(D,[('Until then foldable layouts are approximated."','The app needs its 27.1 SDK to build."')])
C2="docs/design/native-interface-contract.md"
edit(C2,[('''- The shell tells a foldable from an iPad by a fold in `reservedRegions(kind: .division, options: .includeInactive)`, not by device model. `.includeInactive` matters: without it a fully open display reports no fold.
''','''- The shell chooses by interface idiom: an iPhone keeps the tab layout at any width (R3). A size-class-and-fold test would pass through the sidebar layout for a frame while unfolding, dismissing the review's cover.
- The answer row reads the fold from `reservedRegions(kind: .division, options: .includeInactive)`. `.includeInactive` matters: without it a fully open display reports no fold.
'''),
('''- Deck actions are a long press on touch; the row has no … button. Apple reserves the ellipsis for overflow.
''','''- Deck actions are a long press on touch; the row has no … button. Apple reserves the ellipsis for overflow.
- On a stack, a subdeck on the deck page pushes, so Back returns to its parent.
'''),
('''At accessibility text sizes the answers stack and Show Answer takes the full width.''','''At accessibility text sizes the answers stack; with a fold in the row, they stack in two columns either side of it.''')])
