import Foundation
import SwiftUI

extension EnvironmentValues {
    /// The folder the system Files pickers for importing and exporting a
    /// collection open in. Nil (the default, and always outside test hooks)
    /// keeps the system's own choice. Test hooks set it to the folder
    /// `-MeguriImportSource` stages its package in, so UI tests pick that real
    /// file through the real picker without driving Files' own navigation.
    @Entry public var filesPickerDirectory: URL? = nil
}
