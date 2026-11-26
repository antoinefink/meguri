# Sign out from the Meguri Sync Account page

The unit started at `4003069045e9c43646639fb4613dd1588b78f936`. The Account destination had two informational rows. The only way to remove its sign-in was to navigate back and select Off in the Service picker.

The implementation uses the existing service-change confirmation, admission checks and credential-removing handoff from the Account button. A successful change returns to Sync. The user can then select Meguri Sync and enter a different address and username.

## Independent review

Reviewer session `s-20261126T142350Z-e01c62ff` reviewed the clean-room export in run `20261126T142450Z-7281`. It found that the generated screen inventory needed updating and the initial test could not establish a successful sign-out or reconnect. It also identified the missing foreground checks for Mac input.

The contracts were regenerated. The synthetic native test covers the Account page, the confirmation and Cancel. The existing disposable-provider journey now exercises successful removal, selection of Meguri Sync again, field editing and cleanup. Follow-up review `20261126T143639Z-91067` cleared those changes.

The independent reviewer read the complete session records and classified each as public operational metadata. The coordinator verified the committed SHA-256 digests.

| Session | SHA-256 |
| --- | --- |
| s-20261126T141739Z-fa348f3a | 67affaa6e96db1f93ecdc9eaa2edc628ad78a59a6cbd317ab30f724afee4f610 |
| s-20261126T142350Z-e01c62ff | 2ec83d538a822ba321b7d1ab4c9391058e73dd1e7ee6ed5b88d7945474dbfd51 |
| s-20261126T143153Z-ee77be77 | 0bb9c5a6fd83e1398b1b8b6e3a71ea6f506b8bc8063815f1cd98df3a99fa1d85 |

## Verification

- The focused Swift suites passed all 18 cases, including service changes against the native provider bridge (`swift-test/20261126T142708Z-23514`).
- The macOS Account and Cancel journey passed in the VM (`ui-test/20261126T143509Z-79059`). Its two Settings-window captures are attached to [PR #240](https://github.com/antoinefink/meguri/pull/240).
- Design documentation and the session, trailer and retry checks passed (`design-docs/20261126T143847Z-6314`, `static-checks/20261126T143850Z-6313`).
- The first disposable-provider run stopped before sign-in at the menu title check (`ui-test/20261126T150405Z-17266`). The native menu reports the accessibility name “iCloud, Beta” specified by T19.2. The test now checks that name and the exact order of the three service options. See the PR for the subsequent run and hosted checks.
