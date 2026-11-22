import Foundation
import PDFKit
#if os(macOS)
import AppKit
#else
import UIKit
#endif

extension NativeCardBehaviorPresenter {
    public func openAttachmentPDF(_ bytes: Data) -> CardBehaviorPresentation? {
        guard isAvailable, bytes.count <= CardAttachmentPDF.maximumBytes,
              let document = PDFDocument(data: bytes), !document.isLocked, document.pageCount > 0 else { return nil }
        #if os(macOS)
        guard let window else { return nil }
        let panel = CardPDFPanel(contentRect: window.contentLayoutRect,
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        panel.isReleasedWhenClosed = false
        let pdf = PDFView(frame: .zero)
        pdf.autoScales = true
        pdf.document = document
        panel.contentView = pdf
        window.beginSheet(panel) { [weak pdf] _ in pdf?.document = nil }
        return CardBehaviorPresentation { [weak panel] in panel?.close() }
        #else
        guard let host else { return nil }
        let viewer = CardPDFController(document: document)
        let navigation = UINavigationController(rootViewController: viewer)
        host.present(navigation, animated: true)
        guard navigation.presentingViewController != nil else { return nil }
        return CardBehaviorPresentation { [weak navigation, weak viewer] in
            viewer?.pdf.document = nil
            navigation?.dismiss(animated: false)
        }
        #endif
    }
}

#if os(macOS)
private final class CardPDFPanel: NSPanel {
    override func close() {
        (contentView as? PDFView)?.document = nil
        sheetParent?.endSheet(self)
        super.close()
    }
    override func cancelOperation(_ sender: Any?) { close() }
}
#else
private final class CardPDFController: UIViewController {
    let pdf = PDFView(frame: .zero)
    init(document: PDFDocument) {
        super.init(nibName: nil, bundle: nil)
        pdf.autoScales = true
        pdf.document = document
        navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .close,
            primaryAction: UIAction { [weak self] _ in
                self?.pdf.document = nil
                self?.dismiss(animated: true)
            })
    }
    required init?(coder: NSCoder) { return nil }
    override func loadView() { view = pdf }
}
#endif
