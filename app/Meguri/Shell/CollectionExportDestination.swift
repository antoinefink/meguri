import AppModel
import Settings
import SwiftUI
import UniformTypeIdentifiers

struct CollectionExportDestination: View {
    let coordinator: CollectionExportCoordinator
    var body: some View {
        CollectionExportSheet(coordinator: coordinator)
        #if os(macOS)
            .background(MacCollectionSavePanel(coordinator: coordinator))
        #else
            .sheet(isPresented: Binding(get: { coordinator.destinationPresented }, set: { _ in })) {
                if let lease = coordinator.providerLease {
                    CollectionFilesPicker(coordinator: coordinator, url: lease.url)
                        .interactiveDismissDisabled()
                }
            }
        #endif
    }
}
#if os(macOS)
import AppKit
private struct MacCollectionSavePanel: NSViewRepresentable {
    let coordinator: CollectionExportCoordinator
    func makeNSView(context: Context) -> NSView { NSView() }
    func makeCoordinator() -> Owner { Owner() }
    func updateNSView(_ view: NSView, context: Context) {
        guard coordinator.destinationPresented, context.coordinator.panel == nil, let window = view.window else { return }
        let panel = NSSavePanel()
        context.coordinator.panel = panel
        panel.allowedContentTypes = [UTType(importedAs: "com.antoinefink.meguri.collection-package")]
        panel.nameFieldStringValue = coordinator.artifact?.metadata.suggestedName ?? "Meguri-Collection.colpkg"
        panel.canCreateDirectories = true
        panel.beginSheetModal(for: window) { response in
            context.coordinator.panel = nil
            if response == .OK, let url = panel.url { coordinator.save(to: url) }
            else { coordinator.nativeCancelled() }
        }
    }
    final class Owner { var panel: NSSavePanel? }
}
#else
import UIKit
private struct CollectionFilesPicker: UIViewControllerRepresentable {
    let coordinator: CollectionExportCoordinator
    let url: URL
    func makeCoordinator() -> Delegate { Delegate(coordinator) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Delegate: NSObject, UIDocumentPickerDelegate {
        // The app-owned coordinator, rather than this view, retains the Rust lease.
        let owner: CollectionExportCoordinator
        init(_ owner: CollectionExportCoordinator) { self.owner = owner }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            if urls.count == 1 { owner.nativeSaved() }
            else { owner.nativeCancelled() }
        }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { owner.nativeCancelled() }
    }
}
#endif
