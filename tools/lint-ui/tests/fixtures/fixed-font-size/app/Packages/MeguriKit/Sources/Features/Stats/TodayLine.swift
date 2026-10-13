import SwiftUI

struct TodayLine: View {
    var body: some View {
        VStack {
            Text("Studied 42 cards")
                .font(.system(size: 17))
            Text("in 12 minutes")
                .font(.custom("Avenir", size: 13))
            Text("Today")
                .font(.system(.headline, design: .rounded))
        }
    }
}
