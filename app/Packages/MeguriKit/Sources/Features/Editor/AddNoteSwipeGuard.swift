#if os(iOS)
import SwiftUI
import UIKit

/// SwiftUI prevents interactive dismissal; UIKit reports the prevented attempt
/// so it can ask the held-flush guard. Preserve the host's other delegate work.
struct AddNoteSwipeGuard: UIViewControllerRepresentable {
    let attempt: () -> Void
    func makeUIViewController(context: Context) -> Observer { Observer(attempt: attempt) }
    func updateUIViewController(_ controller: Observer, context: Context) {
        controller.attempt = attempt
        controller.install()
    }
    static func dismantleUIViewController(_ controller: Observer, coordinator: ()) { controller.restore() }

    final class Observer: UIViewController, UIAdaptivePresentationControllerDelegate {
        var attempt: () -> Void
        private weak var observed: UIPresentationController?
        // NSObject's nonisolated selector introspection only reads this weak
        // reference. Installation and presentation callbacks stay on MainActor.
        nonisolated(unsafe) private weak var previous: (any UIAdaptivePresentationControllerDelegate)?
        init(attempt: @escaping () -> Void) {
            self.attempt = attempt
            super.init(nibName: nil, bundle: nil)
        }
        required init?(coder: NSCoder) { return nil }
        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            install()
        }
        override func didMove(toParent parent: UIViewController?) {
            super.didMove(toParent: parent)
            install()
        }
        func install() {
            var ancestor = parent
            while let controller = ancestor {
                if controller.presentingViewController != nil, let presentation = controller.presentationController {
                    if presentation.delegate !== self {
                        restore()
                        observed = presentation; previous = presentation.delegate
                        presentation.delegate = self
                    }
                    return
                }
                ancestor = controller.parent
            }
        }
        func restore() {
            if observed?.delegate === self { observed?.delegate = previous }
            observed = nil; previous = nil
        }
        func presentationControllerShouldDismiss(_ presentationController: UIPresentationController) -> Bool { false }
        func presentationControllerDidAttemptToDismiss(_ presentationController: UIPresentationController) { attempt() }
        nonisolated override func responds(to selector: Selector!) -> Bool {
            super.responds(to: selector) || previous?.responds(to: selector) == true
        }
        nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
            if previous?.responds(to: selector) == true { return previous }
            return super.forwardingTarget(for: selector)
        }
    }
}
#endif
