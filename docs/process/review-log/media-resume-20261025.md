# Media recovery in the Mac preview

The user reported a broken card image. An in-process Xcode debugger check found zero files in the active media directory. Sanitized policy inspection showed the completed collection operation still at MediaIncomplete / AwaitingConsent, with no media downloads. No source, account values or card content were exported.

The ad-hoc Xcode MCP product had no team entitlement, private Keychain group or embedded profile. Its login failed before protected credential persistence could complete. The existing team-signed standalone host build passed (20261025T155349Z-24602); opening that exact product restored the saved sign-in automatically. No credential-store fallback or security change was introduced.

The integrator used the existing media-only recovery decision in the app. The operation listed 2,954 files, began receiving files and reached the existing complete outcome. The integrator then opened the reviewer and visually confirmed a downloaded image rendered without the prior placeholder. No answer was rated during this check.

The Xcode MCP selected-test action produced two NOT RUN results, retained at build/xcode-mcp/native-audio-tests-result.json. The same two existing real-hardware checks then ran through the project host runner: swift-test 20261025T161029Z-47183 passed both, with native playback advancement, seeking and queue completion. The exact card replay control was exercised without a visible error; the agent did not listen to its output.

Filtered fixer 20261025T160607Z-41767 makes the existing media decision a stable scroll target. Done and interactive dismissal wait for an explicit decision; Keep Media for Later still records a partial outcome. Active transfers remain closable. Independent source review found no P0–P2 findings. Signed native build 20261025T160930Z-45369 passed. In the live export and subsequent import, Done was disabled while the media decision waited and became enabled after media completion. Keep Later and system-dismissal transitions remain pending focused UI checks.

The integrator exported the saved reviews, completed media, pulled the collection back, activated it and completed media again. Returned facts matched 1,001 cards, 1,001 notes and 37 saved reviews. The study counts stayed 18 new, 2 learning, 0 due; the image rendered after replacement. Saved sign-in survived the signed app relaunch. This proves the bounded Mac workflow; exact-row comparison, changed-media upload, mobile coverage, required batch gates and release acceptance remain pending. U107 remains unaccepted.

Focused privacy check 20261025T162019Z-54000 and similarity check 20261025T162020Z-54234 passed. The native build includes its UI lint check. One signed Mac app instance remained open; free space stayed above the required reserve.
