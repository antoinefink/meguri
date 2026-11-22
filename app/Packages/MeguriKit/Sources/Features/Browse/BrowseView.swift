import AppModel
import SwiftUI

/// Browse's detail beside its sidebar (N21.1, N21.3): the window title
/// "Browse" and the results area. U270 builds the section shell; the table
/// (U270a) takes the place of `BrowseResultsSeam`.
public struct BrowseView: View {
    private let model: BrowseModel

    public init(model: BrowseModel) {
        self.model = model
    }

    public var body: some View {
        BrowseResultsSeam()
            .navigationTitle(Text("browse.title", bundle: .module))
    }
}

/// Where the results table goes (U270a).
struct BrowseResultsSeam: View {
    var body: some View {
        ContentUnavailableView {
            Label {
                Text("browse.seam.title", bundle: .module)
            } icon: {
                Image(systemName: "tablecells")
            }
        } description: {
            Text("browse.seam.message", bundle: .module)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("browse.results.seam")
    }
}
