# Mac settings search gate

Existing failure: `mac-hosted-test/20261031T205446Z-50756`, zero matched buttons.
Diagnostic start: `1c122e4cac42e3375cd7d5ee1c1f34f0596c1939`.
This is an open technical check of the current U113/U114 integration batch.

`cleanroom/20261101T004611Z-60159` adds bounded public metadata to the test.
Its controls and checks stay fixed. Independent review finds no issues;
`private/20261101T004944Z-61627` passes for the test source.

`mac-hosted-test/20261101T005000Z-61955` compiles and runs the selected method
in the dedicated VM. The window is hidden; the attached field is not hidden
but its visible rectangle is empty. There are zero matched buttons. The field
has one accessibility child and that child has two children. The diagnostic
then fails with a Swift NSArray element-type bridge error.

These facts do not prove a product identifier defect. The next test-only
experiment will show this exact synthetic window before synchronous layout
and use the public getter selectors to retain an untyped NSArray, traversed by
count/object(at:). Preserve matcher rules, both mounts, all assertions and
bounded diagnostics. Review approves that experiment. No waits or retries,
private APIs or product-control changes are needed.

The gate remains failed until its selected-method receipt passes.
