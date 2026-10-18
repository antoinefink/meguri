import XCTest

extension XCUIElement {
    @MainActor
    func clickOrTap() {
        #if os(macOS)
        click()
        #else
        tap()
        #endif
    }
}

extension XCUICoordinate {
    @MainActor
    func clickOrTap() {
        #if os(macOS)
        click()
        #else
        tap()
        #endif
    }
}
