import DesignSystem

/// A colour token's declaration, by name, so fixtures can add tokens the
/// real set does not have.
struct TokenSpec {
    enum Role {
        case background
        case foreground(on: [String])
    }

    let name: String
    let role: Role

    init(_ name: String, _ role: Role) {
        self.name = name
        self.role = role
    }

    static var declared: [TokenSpec] {
        ColorToken.allCases.map { token in
            switch token.role {
            case .background: TokenSpec(token.rawValue, .background)
            case let .foreground(_, backgrounds): TokenSpec(token.rawValue, .foreground(on: backgrounds.map(\.rawValue)))
            }
        }
    }
}

enum TokenCoverage {
    /// Tokens that take part in no contrast pairing, or pair wrongly, one
    /// line each: a foreground must name at least one background, only
    /// background tokens can be drawn on, and every background must have a
    /// foreground drawn on it.
    static func problems(_ specs: [TokenSpec]) -> [String] {
        let backgrounds = Set(specs.compactMap { spec -> String? in
            if case .background = spec.role { return spec.name }
            return nil
        })
        var used = Set<String>()
        var problems: [String] = []
        for spec in specs {
            guard case let .foreground(on) = spec.role else { continue }
            if on.isEmpty {
                problems.append("\(spec.name) declares no background to be drawn on")
            }
            for background in on {
                if backgrounds.contains(background) {
                    used.insert(background)
                } else {
                    problems.append("\(spec.name) is drawn on \(background), which is not a background token")
                }
            }
        }
        for spec in specs where backgrounds.contains(spec.name) && !used.contains(spec.name) {
            problems.append("\(spec.name) is a background no foreground is drawn on")
        }
        return problems
    }
}
