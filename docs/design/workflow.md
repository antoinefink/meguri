# Native design workflow

Use this with `brief.md`, the screen's contract and `rubric.md` before changing a screen. The purpose is a clear task and a good native layout, not a larger set of custom controls.

## Before code

Write down the screen's main task, its primary action, what must stay visible and what can wait until requested. Use the existing contract or change notes; a second large plan is not needed.

Use native SwiftUI controls with system styles and sizes. A custom style needs a task or accessibility reason.

Choose a system pattern for the task. Read the Apple HIG and compare system apps. Use native lists, forms, menus, navigation and Settings, with clear groups, aligned controls and plain text.

Resolve meaningful layout choices with a small visual example. Compare different task flows or control locations, not just colours. Make each option usable enough to judge. Record what the user chose and why; do not reopen that choice through later cleanup.

## Information and controls

- Give frequent actions the main space. Study is primary. Put import/export in menus and collection settings. Keep import in an empty collection because it is the first step.
- Keep the current task clear. During review, the card and answer controls lead; deck navigation and unrelated settings move out of the way.
- Keep related controls together. Test belongs beside the server address it tests. Account actions belong with account information.
- Keep frequent controls stable. Review answer buttons stay at the bottom; long content scrolls between them and the exit/status controls.
- Use one disclosure for a related detail task. Connection details contains its metadata and logs directly, without a second log disclosure.
- Make secondary information available without giving it permanent emphasis. A quiet status icon can open a clear explanation. Errors must still say what happened and what the person can do.
- Show shortcuts beside their native menu commands. Do not add a large shortcut help action beneath the primary task.
- Keep text short and specific. Sidebar empty text can say No decks; onboarding guidance belongs in the main empty state.
- State actual behavior. A past transfer is not proof that two collections match. File export copy must match whether media is included.

## Adapt the structure to the platform

| Platform | Design work to do |
|---|---|
| macOS | Use native menus, keyboard ownership, pointer targets and resizable windows. Meguri's Settings uses a separate native window with a section sidebar. Preserve section and deck selection. Top settings panes are also an Apple pattern; the sidebar is Meguri's chosen organization. |
| iPadOS | Use the available window space for navigation and detail. Check both regular and compact widths, small windows, keyboard and pointer use. Collapse navigation when the task needs focus; retain state when the layout changes. |
| iOS | Use navigation stacks and system tabs where the sections need them. Put frequent actions within reach and respect safe areas. Show settings sections as rows and detail screens when a sidebar does not fit. A focused review still needs an obvious exit and bottom answer controls. |

The task, saved data and action meanings stay the same as space changes. The visible arrangement can change. Base layout on available space and size classes, not a device-name test. Check long text, the largest supported text size, short windows and compact height; side-by-side controls may need to stack.

Inspect controls in the native app. Check field alignment and labels in Forms. Size the interaction area on the Button rather than its Text. Check each action identifier after placing it in a group.

Native SwiftUI is the implementation medium. HTML can help choose hierarchy and behavior, but its CSS is not the source for native dimensions, materials or control styling. Keep card-template content separate from app chrome.

## Feedback and evidence

Inspect both collapsed and expanded disclosures. Use leading content inside a disclosure; a Form Section belongs in its Form context.

Inspect empty, normal, loading, busy, error and long-content states. Check the main action, alignment, groups, text wrapping and control placement. No clipping does not mean the screen is clear.

Then inspect the exact native build. Record its source commit or input digest and product path. Replacing the wrong running copy invalidates the review; choose the built product by path and confirm its identity. Never use an older prototype as evidence for current production behavior.

Keep these outcomes separate:

- Functional checks: the implementation builds and its actions work.
- Design agreement: the user accepts the composition and behavior shown.
- Final verification: required state, platform, accessibility, review and integration checks pass for the actual source.

Keep all three records. A mockup does not pass a native gate. Independent agents can report issues, but the user decides how the result feels. Agent approval does not count as user approval.

For an early preview, run focused build and smoke checks. Record the full checks as pending. The work stays unaccepted until required checks pass after user feedback.

Use at most three self-directed render/fix rounds per screen, then the independent critic and required gates. Clear visible task, grouping and alignment problems before asking for design acceptance. Preserve the rubric's rule ids and severity definitions; a new rubric rule requires its normal versioned change.

## Carry decisions forward

Record user choices in the active contract and change log. Use the existing process for `decisions.md`, approved baselines and screen status. HTML approval is not native approval.

Use one self-contained, navigable HTML page when a browser example is useful. Keep example controls outside the product view and clearly identify sample data. If inline previews are unavailable on the user's device, provide the browser page directly rather than repeating the unavailable preview. Keep generated examples out of `references/` and approved screenshots.

Reference patterns: [references/index.md](references/index.md). User decisions: Antoine, 2026-10-24. Check the Apple HIG for the current layout, Settings and toolbar patterns.

