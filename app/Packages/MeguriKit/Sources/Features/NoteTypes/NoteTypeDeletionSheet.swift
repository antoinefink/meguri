import AppModel
import SwiftUI

/// The same action on Mac and on compact/wide touch lists.
struct NoteTypeDeletionMenu: View {
    let model: NoteTypesModel
    let id: Int64

    var body: some View {
        Button(role: .destructive) { model.delete(id) } label: {
            Label { Text("noteTypes.delete.menu", bundle: .module) } icon: { Image(systemName: "trash") }
        }
        .disabled(!model.canDelete(id))
        .accessibilityIdentifier("noteTypes.list.delete")
    }
}

/// W12's native alert belongs to the host of the navigation route, rather
/// than the structural Save sheet. Both platforms use the same message.
public struct NoteTypeDeletionConfirmation: View {
    let model: NoteTypesModel
    let isPresented: Bool
    @Environment(\.locale) private var locale

    public init(model: NoteTypesModel, isPresented: Bool) {
        self.model = model
        self.isPresented = isPresented
    }

    public var body: some View {
        DeletionAlertHost(model: model, question: isPresented ? model.deletionQuestion : nil,
                          confirming: model.confirming, canConfirm: model.canConfirmDeletion,
                          title: model.deletionQuestion.map { NoteTypesStrings.deleteTitle($0.name, locale: locale) } ?? "",
                          message: message,
                          cancelTitle: NoteTypesStrings.text("noteTypes.save.cancel", locale: locale),
                          confirmTitle: NoteTypesStrings.text(
                            model.deletionQuestion?.service == .meguriSync && !model.meguriUnreachable
                                ? "noteTypes.delete.upload" : "noteTypes.delete.confirm", locale: locale))
    }

    private var message: String {
        guard let question = model.deletionQuestion else { return "" }
        let device = NoteTypesStrings.device(locale: locale)
        var paragraphs = [NoteTypesStrings.deleteLosses(question.counts, locale: locale)]
        if question.service == .meguriSync, model.meguriSyncState == .ready {
            paragraphs.append(model.meguriUnreachable
                ? NoteTypesStrings.text("noteTypes.save.unreachable.title", locale: locale) + " "
                    + NoteTypesStrings.unreachableBody(device: device, locale: locale)
                : NoteTypesStrings.text("noteTypes.save.sync.title", locale: locale) + " "
                    + NoteTypesStrings.syncBody(device: device, locale: locale))
            paragraphs.append(NoteTypesStrings.text("noteTypes.save.sync.others", locale: locale))
        }
        paragraphs.append(NoteTypesStrings.backup(device: device, locale: locale))
        if question.service == .meguriSync {
            switch model.meguriSyncState {
            case .ready: break
            case .notSetUp: paragraphs.append(NoteTypesStrings.text("noteTypes.save.notSetUp", locale: locale))
            case .busy: paragraphs.append(NoteTypesStrings.text("noteTypes.save.syncBusy", locale: locale))
            case .uploadPending: paragraphs.append(NoteTypesStrings.text("noteTypes.save.uploadPending", locale: locale))
            }
        }
        return paragraphs.joined(separator: "\n\n")
    }
}

#if os(macOS)
import AppKit

private struct DeletionAlertHost: NSViewRepresentable {
    let model: NoteTypesModel
    let question: NoteTypesModel.DeletionQuestion?
    let confirming: Bool
    let canConfirm: Bool
    let title: String
    let message: String
    let cancelTitle: String
    let confirmTitle: String

    func makeNSView(context: Context) -> PresenterView { PresenterView() }

    func updateNSView(_ view: PresenterView, context: Context) {
        view.presentation = self
        view.reconcile()
    }

    static func dismantleNSView(_ view: PresenterView, coordinator: ()) {
        view.presentation = nil
        view.dismiss()
    }

    final class PresenterView: NSView {
        var presentation: DeletionAlertHost?
        private var alert: NSAlert?
        private var shown: NoteTypesModel.DeletionQuestion?
        private var observers: [NSObjectProtocol] = []
        private var escapeMonitor: Any?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            for observer in observers { NotificationCenter.default.removeObserver(observer) }
            observers = []
            // Never retain an old window across a shell/host replacement.
            dismiss()
            guard let window else { return }
            for name in [NSWindow.didBecomeKeyNotification, NSWindow.didChangeOcclusionStateNotification,
                         NSWindow.didDeminiaturizeNotification, NSWindow.didEndSheetNotification] {
                observe(name, object: window)
            }
            observe(NSApplication.didHideNotification, object: NSApp)
            observe(NSApplication.didUnhideNotification, object: NSApp)
            reconcile()
        }

        private func observe(_ name: Notification.Name, object: AnyObject) {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: object, queue: .main) {
                [weak self] _ in
                MainActor.assumeIsolated { self?.reconcile() }
            })
        }

        func reconcile() {
            guard let presentation, let question = presentation.question,
                  !presentation.confirming, !presentation.model.confirming,
                  presentation.model.deletionQuestion == question,
                  let window, window.isVisible, !window.isMiniaturized, !NSApp.isHidden else {
                dismiss()
                return
            }
            if let alert {
                guard shown == question,
                      alert.window.sheetParent == nil || alert.window.sheetParent === window else { dismiss(); return }
                if alert.messageText != presentation.title || alert.informativeText != presentation.message {
                    alert.messageText = presentation.title
                    alert.informativeText = presentation.message
                    alert.layout()
                }
                alert.buttons[0].title = presentation.cancelTitle
                alert.buttons[1].title = presentation.confirmTitle
                alert.buttons[1].isEnabled = presentation.canConfirm
                return
            }
            // Another sheet keeps its ownership; didEndSheet resumes this ask.
            guard window.attachedSheet == nil else { return }
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = presentation.title
            alert.informativeText = presentation.message
            alert.window.identifier = NSUserInterfaceItemIdentifier("noteTypes.delete.sheet")
            let cancel = alert.addButton(withTitle: presentation.cancelTitle)
            let confirm = alert.addButton(withTitle: presentation.confirmTitle)
            confirm.hasDestructiveAction = true
            confirm.keyEquivalent = ""
            confirm.isEnabled = presentation.canConfirm
            // Keep the first Cancel button as AppKit's Return default.
            cancel.keyEquivalent = "\r"
            self.alert = alert
            shown = question
            escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self, weak alert] event in
                let handled = MainActor.assumeIsolated {
                    guard let self, let alert, self.alert === alert,
                          event.window === alert.window, event.keyCode == 53,
                          event.modifierFlags.intersection(.deviceIndependentFlagsMask).isEmpty,
                          let cancel = alert.buttons.first, cancel.isEnabled else { return false }
                    cancel.performClick(nil)
                    return true
                }
                return handled ? nil : event
            }
            alert.beginSheetModal(for: window) { [weak self] response in
                guard let self else { return }
                // Programmatic dismissal has no model side effects. In
                // particular it must never cancel an asynchronously renewed ask.
                if self.alert === alert {
                    self.removeEscapeMonitor()
                    self.alert = nil
                    self.shown = nil
                    if let current = self.presentation, current.question == question,
                       current.model.deletionQuestion == question, !current.model.confirming {
                        if response == .alertSecondButtonReturn { current.model.confirmDeletion() }
                        else { current.model.cancelQuestion() }
                    }
                }
                DispatchQueue.main.async { [weak self] in self?.reconcile() }
            }
        }

        private func removeEscapeMonitor() {
            if let escapeMonitor { NSEvent.removeMonitor(escapeMonitor) }
            escapeMonitor = nil
        }

        func dismiss() {
            removeEscapeMonitor()
            guard let alert else { return }
            self.alert = nil
            shown = nil
            alert.window.sheetParent?.endSheet(alert.window, returnCode: .abort)
        }

        isolated deinit {
            removeEscapeMonitor()
            for observer in observers { NotificationCenter.default.removeObserver(observer) }
        }
    }
}
#endif

#if os(iOS)
import UIKit

/// SwiftUI's alert binding has no dismissal completion: the model retains its
/// question while confirming, and can renew it before the old alert finishes
/// leaving. This bridge serializes those transitions and updates availability
/// while shown. UIKit owns the entire alert, including sizing and Dynamic Type.
private struct DeletionAlertHost: UIViewControllerRepresentable {
    let model: NoteTypesModel
    let question: NoteTypesModel.DeletionQuestion?
    let confirming: Bool
    let canConfirm: Bool
    let title: String
    let message: String
    let cancelTitle: String
    let confirmTitle: String

    func makeUIViewController(context: Context) -> Presenter { Presenter() }

    func updateUIViewController(_ presenter: Presenter, context: Context) {
        presenter.presentation = self
        // Do not start UIKit transitions during SwiftUI's update pass.
        presenter.scheduleReconciliation()
    }

    static func dismantleUIViewController(_ presenter: Presenter, coordinator: ()) {
        presenter.presentation = nil
        presenter.reconcile()
    }

    final class Presenter: UIViewController {
        var presentation: DeletionAlertHost?
        private var alert: UIAlertController?
        private var shown: DeletionAlertHost?
        private var transitioning = false
        private var scheduled = false

        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            scheduleReconciliation()
        }

        func scheduleReconciliation() {
            guard !scheduled else { return }
            scheduled = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                scheduled = false
                reconcile()
            }
        }

        func reconcile() {
            guard !transitioning else { return }
            guard let current = presentation, let question = current.question,
                  !current.confirming, !current.model.confirming,
                  current.model.deletionQuestion == question,
                  viewIfLoaded?.window != nil else {
                dismissAlert()
                return
            }
            if let alert, let shown {
                guard shown.model === current.model, shown.question == question,
                      shown.confirmTitle == current.confirmTitle, shown.cancelTitle == current.cancelTitle else {
                    dismissAlert()
                    return
                }
                alert.title = current.title
                alert.message = current.message
                alert.actions.first(where: { $0.style == .destructive })?.isEnabled = current.canConfirm
                return
            }
            // Respect another presentation on this route. A transition completion
            // or viewDidAppear resumes the question once its presenter is free.
            var host: UIViewController = self
            while let parent = host.parent { host = parent }
            if let other = host.presentedViewController {
                other.transitionCoordinator?.animate(alongsideTransition: nil) { [weak self] _ in
                    self?.scheduleReconciliation()
                }
                return
            }
            let alert = UIAlertController(title: current.title, message: current.message, preferredStyle: .alert)
            alert.addAction(UIAlertAction(title: current.cancelTitle, style: .cancel) { [weak self, weak alert] _ in
                self?.respond(to: alert, confirm: false)
            })
            let confirm = UIAlertAction(title: current.confirmTitle, style: .destructive) { [weak self, weak alert] _ in
                self?.respond(to: alert, confirm: true)
            }
            confirm.isEnabled = current.canConfirm
            alert.addAction(confirm)
            // Do not make the destructive action the keyboard default.
            self.alert = alert
            shown = current
            transitioning = true
            host.present(alert, animated: true) { [self] in
                transitioning = false
                reconcile()
            }
        }

        private func respond(to alert: UIAlertController?, confirm: Bool) {
            guard let alert, self.alert === alert, let shown else { return }
            // Retire before calling the model. Neither UIKit's automatic
            // dismissal nor a replaced route ever calls Cancel on the model.
            let current = presentation
            dismissAlert {
                guard let current, current.model === shown.model,
                      current.question == shown.question, current.question != nil,
                      current.model.deletionQuestion == shown.question,
                      !current.model.confirming else { return }
                if confirm {
                    if current.model.canConfirmDeletion { current.model.confirmDeletion() }
                } else {
                    current.model.cancelQuestion()
                }
            }
        }

        private func dismissAlert(beforeDismiss: () -> Void = {}) {
            guard let alert else { return }
            self.alert = nil
            shown = nil
            transitioning = true
            // The action changes the model before even a synchronous dismissal
            // completion can consider presenting its retained question again.
            beforeDismiss()
            // Keep this presenter through completion even when its SwiftUI host
            // is replaced. A renewed question is read only after dismissal.
            alert.dismiss(animated: true) { [self] in
                transitioning = false
                reconcile()
            }
        }
    }
}
#endif
