import AppModel
import DesignSystem
import SwiftUI

#if os(iOS)
/// W12, D4: the touch form sheet. No destructive keyboard default.
struct NoteTypeDeletionSheet: View {
    let model: NoteTypesModel
    let question: NoteTypesModel.DeletionQuestion
    @Environment(\.locale) private var locale

    var body: some View {
        NavigationStack {
            Form { details }
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) { cancel }
                    ToolbarItem(placement: .destructiveAction) { confirm }
                }
        }
        .presentationDetents([.large])
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("noteTypes.delete.sheet")
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: Spacing.medium) {
            Text(verbatim: NoteTypesStrings.deleteTitle(question.name, locale: locale))
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("noteTypes.delete.title")
            Text(verbatim: NoteTypesStrings.deleteLosses(question.counts, locale: locale))
                .accessibilityIdentifier("noteTypes.delete.counts")
            if question.service == .meguriSync, model.meguriSyncState == .ready {
                VStack(alignment: .leading, spacing: Spacing.xSmall) {
                    if model.meguriUnreachable {
                        Text("\(Text("noteTypes.save.unreachable.title", bundle: .module).bold()) \(NoteTypesStrings.unreachableBody(device: device, locale: locale))")
                    } else {
                        Text("\(Text("noteTypes.save.sync.title", bundle: .module).bold()) \(NoteTypesStrings.syncBody(device: device, locale: locale))")
                    }
                    Text("noteTypes.save.sync.others", bundle: .module)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("noteTypes.delete.sync")
            }
            Text(verbatim: NoteTypesStrings.backup(device: device, locale: locale))
                .foregroundStyle(.secondary)
            if let unavailable {
                Text(verbatim: unavailable)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("noteTypes.delete.unavailable")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private var device: String { NoteTypesStrings.device(locale: locale) }

    private var unavailable: String? {
        guard question.service == .meguriSync else { return nil }
        switch model.meguriSyncState {
        case .ready: return nil
        case .notSetUp: return NoteTypesStrings.text("noteTypes.save.notSetUp", locale: locale)
        case .busy: return NoteTypesStrings.text("noteTypes.save.syncBusy", locale: locale)
        case .uploadPending: return NoteTypesStrings.text("noteTypes.save.uploadPending", locale: locale)
        }
    }

    private var cancel: some View {
        Button(role: .cancel) { model.cancelQuestion() } label: { Text("noteTypes.save.cancel", bundle: .module) }
            .keyboardShortcut(.cancelAction)
            .disabled(model.confirming)
            .accessibilityIdentifier("noteTypes.delete.cancel")
    }

    private var confirm: some View {
        Button(role: .destructive) { model.confirmDeletion() } label: {
            if question.service == .meguriSync, !model.meguriUnreachable {
                Text("noteTypes.delete.upload", bundle: .module)
            } else {
                Text("noteTypes.delete.confirm", bundle: .module)
            }
        }
        .disabled(!model.canConfirmDeletion)
        .help(Text(verbatim: unavailable ?? ""))
        .accessibilityIdentifier("noteTypes.delete.confirm")
    }
}

#endif

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

#if os(macOS)
import AppKit

/// W12's native alert belongs to the window hosting the navigation route,
/// rather than a SwiftUI sheet window or whichever window happens to be key.
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
            // NSAlert supplies Escape cancellation for its Cancel button.
            cancel.keyEquivalent = "\r"
            self.alert = alert
            shown = question
            alert.beginSheetModal(for: window) { [weak self] response in
                guard let self else { return }
                // Programmatic dismissal has no model side effects. In
                // particular it must never cancel an asynchronously renewed ask.
                if self.alert === alert {
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

        func dismiss() {
            guard let alert else { return }
            self.alert = nil
            shown = nil
            alert.window.sheetParent?.endSheet(alert.window, returnCode: .abort)
        }

        isolated deinit {
            for observer in observers { NotificationCenter.default.removeObserver(observer) }
        }
    }
}
#endif
