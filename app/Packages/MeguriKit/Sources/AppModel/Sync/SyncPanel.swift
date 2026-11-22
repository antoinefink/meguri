import CoreBridge
import Foundation

/// N23: the status panel's moving part and its buttons. Display only: the
/// status itself is `SyncMainStatus` (T19.4, T19.5), the same as the Sync
/// section's status row, so the two never disagree.
public enum SyncPanel {
    /// N23.1: one linear bar while something moves.
    public enum Progress: Equatable, Sendable {
        case indeterminate, fraction(Double)
        public var fraction: Double? { if case .fraction(let value) = self { value } else { nil } }
    }

    /// N23.3: what follows Sync Settings in the trailing button row.
    public enum Slot: Equatable, Sendable {
        /// Sync Settings alone, as the default button (sync is off).
        case none
        /// Sync Settings alone, with no default: the status needs the person
        /// and has no built action (N23.3).
        case nothingToRun
        /// Sync Now, the default button, disabled while it cannot run.
        case syncNow
        /// The status row's action.
        case action(SyncStatusAction)
    }

    /// N23.3's table. `offered`: the action can run now (`SyncStatusActionButton.offered`).
    public static func slot(_ status: SyncMainStatus, serviceSelected: Bool, offered: Bool) -> Slot {
        // An issues row's action is in its list, never the status's (T19.4 rule 3).
        if let action = status.action, status.kind != .issues {
            if offered { return .action(action) }
            if status.needsYou { return .nothingToRun }
        } else if status.needsYou {
            return .nothingToRun
        }
        return serviceSelected ? .syncNow : .none
    }

    /// N23.3: the panel's Sync Now is disabled while the status moves, even
    /// where a press would join the running exchange (P12.1).
    public static func syncNowEnabled(available: Bool, moving: Bool) -> Bool { available && !moving }

    /// The button Return runs on Mac (N23.3): Sync Settings when it is alone;
    /// otherwise the slot's button, unless it has the cancel role.
    public static func settingsIsDefault(_ slot: Slot) -> Bool { slot == .none }
    public static func slotIsDefault(_ slot: Slot) -> Bool {
        switch slot {
        case .none, .nothingToRun: false
        case .syncNow: true
        case .action(let action): !action.neverDefault
        }
    }

    /// N23.1: Meguri Sync's setup work uses the status row's value (MP6.5);
    /// a round's exchange and iCloud's collection or media transfer are
    /// indeterminate. A status that needs the person has no bar.
    /// `meguriMoving`: Meguri Sync signs in or a round exchanges, which the
    /// status row shows no setup value for.
    public static func progress(_ status: SyncMainStatus, meguri: Bool, setupValue: MeguriProgressValue?,
                                meguriMoving: Bool, cloudState: CloudPolicyStatusState?) -> Progress? {
        guard !status.needsYou else { return nil }
        if meguri {
            switch setupValue {
            case .fraction(let value)?: return .fraction(value)
            case .indeterminate?: return .indeterminate
            case nil: return meguriMoving ? .indeterminate : nil
            }
        }
        // The bar shows with the moving symbol only: a transfer under an
        // up-to-date state shows none (N23.1).
        switch cloudState {
        case .syncingCollection?, .syncingMedia?: return .indeterminate
        default: return nil
        }
    }
}

extension SyncMainStatus {
    /// N23.1: the detail key shown under the title; a detail that repeats the
    /// title ("Setting Up…") is not shown, in the panel or the Sync section.
    public var shownDetailKey: String? { detailKey == titleKey ? nil : detailKey }
}

extension SyncStatusAction {
    /// N23.3: never the default button. Cancel Setup, and the actions that run
    /// a collection replacement at once.
    public var neverDefault: Bool {
        switch self {
        case .meguriCancel, .meguriFinishSetup, .resumeOp, .meguriUploadStructure: true
        default: false
        }
    }
}
