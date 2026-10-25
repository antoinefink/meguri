# Media recovery in the Mac preview

The user reported a broken card image. An in-process Xcode debugger check found zero files in the active media directory. Sanitized policy inspection showed the completed collection operation still at MediaIncomplete / AwaitingConsent, with no media downloads. No source, account values or card content were exported.

The ad-hoc Xcode MCP product had no team entitlement, private Keychain group or embedded profile. Its login failed before protected credential persistence could complete. The existing team-signed standalone host build passed (20261025T155349Z-24602); opening that exact product restored the saved sign-in automatically. No credential-store fallback or security change was introduced.

The integrator used the existing media-only recovery decision in the app. The operation listed 2,954 files, began receiving files and reached the existing complete outcome. The integrator then opened the reviewer and visually confirmed a downloaded image rendered without the prior placeholder. No answer was rated during this check.

Audio playback remains unverified. The pending media decision needs clear presentation. Preserve explicit consent and partial outcomes. Server upload, mobile checks and release acceptance remain pending. U107 remains unaccepted.
