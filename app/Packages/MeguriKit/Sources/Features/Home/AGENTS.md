# Home

- `HomeDeckListPreview` renders the production tree with DTO fixtures. Use it for native appearance checks; it does not prove saved selection or disclosure.
- `home/deck-list/*` routes leave the production collection closed. A full `-MeguriFixture` launch replaces its destination collection; the `.dev` id does not isolate the current user's development data. Use an isolated test container or the VM for that launch.
