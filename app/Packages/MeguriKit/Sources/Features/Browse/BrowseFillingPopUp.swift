#if os(macOS)
import AppKit
import SwiftUI

/// The inspector's Deck and Type controls (N17.1's rows): the system's pop-up
/// button, which SwiftUI's `Picker` and `Menu` keep at their natural widths
/// whatever frame they are given (VM, 2026-11-23). This one takes the width
/// the row offers, so both controls end at the pane's trailing inset, as in
/// Apple's inspectors; a title too long for it is shortened in the middle,
/// the whole title in its help.
struct BrowseFillingPopUp: NSViewRepresentable {
    struct Item {
        var title: String
        var enabled = true
        var separatorBefore = false
        var identifier: String?
        var action: () -> Void
    }

    /// A pop-up (one of `items` chosen, `selected`) or, with `pullDownTitle`,
    /// a pull-down whose title stays.
    var items: [Item]
    var selected: Int?
    var pullDownTitle: String?
    var label: String
    var identifier: String

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSPopUpButton {
        let button = NSPopUpButton(frame: .zero, pullsDown: pullDownTitle != nil)
        button.autoenablesItems = false
        (button.cell as? NSPopUpButtonCell)?.lineBreakMode = .byTruncatingMiddle
        button.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return button
    }

    func updateNSView(_ button: NSPopUpButton, context: Context) {
        context.coordinator.items = items
        let menu = NSMenu()
        if let pullDownTitle { menu.addItem(NSMenuItem(title: pullDownTitle, action: nil, keyEquivalent: "")) }
        for (index, item) in items.enumerated() {
            if item.separatorBefore { menu.addItem(.separator()) }
            let entry = NSMenuItem(title: item.title, action: #selector(Coordinator.choose(_:)), keyEquivalent: "")
            entry.target = context.coordinator
            entry.tag = index
            entry.isEnabled = item.enabled
            if let id = item.identifier { entry.setAccessibilityIdentifier(id) }
            menu.addItem(entry)
        }
        button.menu = menu
        if pullDownTitle == nil, let selected, selected < items.count {
            button.selectItem(withTag: selected)
        }
        let title = pullDownTitle ?? selected.flatMap { $0 < items.count ? items[$0].title : nil } ?? ""
        button.toolTip = title
        button.setAccessibilityLabel(label)
        button.setAccessibilityIdentifier(identifier)
    }

    /// The row's width, the button's own height.
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSPopUpButton, context: Context) -> CGSize? {
        let natural = nsView.intrinsicContentSize
        return CGSize(width: proposal.width ?? natural.width, height: natural.height)
    }

    final class Coordinator: NSObject {
        var items: [Item] = []
        @objc func choose(_ sender: NSMenuItem) {
            guard sender.tag < items.count else { return }
            items[sender.tag].action()
        }
    }
}
#endif
