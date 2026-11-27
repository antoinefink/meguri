# Browse search before save

Base `be8c979e9d9286d7e234230dd4e3aa8d80c84c37`. [Master package-smoke](https://github.com/antoinefink/meguri/actions/runs/45995015105/job/140492297598) timed out waiting for the search request in `a list read begun before a save cannot restore old excerpts`. The same case passed in the local baseline `sync-web-verify/20261127T142640Z-13019`.

Opening the selected note loads its detail and gives the matched field focus. The test now waits for that focus before filling Search. It still holds the old list response, saves the edit, then checks that the old response was aborted and the new excerpt stayed visible.

Independent review `s-20261127T142910Z-b4a5343b` found no source issues and classified both complete files and the two session records as public. The review verified the following SHA-256 digests.

- `server/web/tests/browser/browse.spec.ts` `5b91ea83795f61e2ec88871e699c908d623d91cb808e325d29cdb566e7ec1da2`
- `server/web/scripts/audit-assessment.json` `2bd1e3cdb5ed27b8913727a648e22ddb82d93a8f1ddf75ae0cf18e655eb10256`
