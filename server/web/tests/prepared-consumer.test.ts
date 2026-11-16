// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const web = fileURLToPath(new URL("../../", import.meta.url));

test("actual Rust preparation consumer independently binds the complete copied source tree", async t => {
  const root = await mkdtemp(fileURLToPath(new URL("../meguri-web-consumer-", import.meta.url)));
  t.after(() => rm(root, { recursive: true, force: true }));
  const manifest = join(root, "server"), source = join(manifest, "web");
  const copied = join(root, "copied"), prepared = join(root, "prepared"), out = join(root, "out");
  const dist = join(root, "dist"), notices = join(root, "runtime-notices.txt");
  const consumer = join(root, "build-script");
  await run(process.execPath, [join(web, "scripts/copy-source.mjs"), web, source]);
  await mkdir(join(source, "tests/dist"), { recursive: true });
  await writeFile(join(source, "tests/dist/input.html"), '<div class="text-red-500"></div>\n');
  await writeFile(join(source, ".discovery.fixture"), "hidden source input\n");
  const generated = ["node_modules", "dist", ".test-build", "playwright-report", "test-results"];
  for (const name of generated) {
    await mkdir(join(source, name), { recursive: true });
    await writeFile(join(source, name, "generated.fixture"), "excluded output\n");
  }
  // Exercise the same copy entrypoint as run.sh, including hidden files.
  await run(process.execPath, [join(source, "scripts/copy-source.mjs"), source, copied]);
  assert.deepEqual((await readdir(copied)).filter(name => generated.includes(name)), []);
  assert.deepEqual(await readFile(join(copied, ".gitignore")), await readFile(join(source, ".gitignore")));
  assert.deepEqual(await readFile(join(copied, "tests/dist/input.html")), await readFile(join(source, "tests/dist/input.html")));

  // Synthetic assets test preparation only; they are never served or credited
  // as a real browser build. Invoke the real producer against the copied tree.
  const notice = "Synthetic consumer-boundary notice.\n";
  await writeFile(notices, notice);
  await mkdir(join(dist, "assets"), { recursive: true });
  await writeFile(join(dist, "index.html"), '<script type="module" src="/assets/fixture-12345678.js"></script>\n');
  await writeFile(join(dist, "assets/fixture-12345678.js"), Buffer.concat([
    Buffer.from("/*!\n"), await readFile(join(copied, "THIRD_PARTY_NOTICES.txt")),
    Buffer.from(`\n${notice}*/\nexport {};\n`),
  ]));
  await run(process.execPath, [join(copied, "scripts/embed.mjs"), dist, prepared], {
    cwd: copied, env: { ...process.env, MEGURI_WEB_NOTICES_FILE: notices }, timeout: 30_000,
  });
  // MG mounts this exact production file read-only for unit/check jobs. No
  // Cargo recursion, dependencies, or production test hooks are involved.
  await run("rustc", ["--edition=2024", "/server/build.rs", "-o", consumer], { cwd: root, timeout: 30_000 });
  await mkdir(out);
  const env = { ...process.env, CARGO_MANIFEST_DIR: manifest, OUT_DIR: out, MEGURI_WEB_PREPARED_DIR: prepared };
  const invoke = () => run(consumer, [], { cwd: root, env, timeout: 30_000 });
  await invoke();
  assert.deepEqual(await readFile(join(out, "meguri_web_assets.rs")), await readFile(join(prepared, "meguri_web_assets.rs")));
  assert.deepEqual(await readFile(join(out, "embedded/assets_fixture-12345678.js")), await readFile(join(prepared, "embedded/assets_fixture-12345678.js")));

  // A source-only addition must fail even though the producer's declared
  // snapshot has not changed. Exercise both inventory directions and bytes.
  for (const [directory, relative, replacement, reason] of [
    [source, "tests/dist/input.html", Buffer.from("changed CSS input\n"), /stale prepared browser input:/],
    [source, "tests/added.fixture.html", Buffer.from("new CSS input\n"), /stale prepared browser input inventory/],
    [source, ".gitignore", Buffer.from("tests/\n"), /stale prepared browser input:/],
    [source, ".discovery.fixture", undefined, /stale prepared browser input inventory/],
    [join(prepared, "inputs"), "omitted-by-producer.fixture", Buffer.from("extra input\n"), /stale prepared browser input inventory/],
    [join(prepared, "inputs"), "index.html", undefined, /stale prepared browser input inventory/],
    [join(prepared, "inputs"), ".gitignore", Buffer.from("changed snapshot\n"), /stale prepared browser input:/],
  ] as const) {
    const path = join(directory, relative);
    const original = await readFile(path).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
      return undefined;
    });
    try {
      if (replacement === undefined) await rm(path); else await writeFile(path, replacement);
      await assert.rejects(invoke, (error: Error & { stderr?: string }) => {
        assert.match(error.stderr ?? "", reason, relative);
        return true;
      });
    } finally {
      if (original === undefined) await rm(path, { force: true }); else await writeFile(path, original);
    }
  }
  // Exclusion is directory-only and root-only on both sides.
  for (const name of generated) await writeFile(join(source, name, "generated.fixture"), "new output\n");
  await invoke();
  await rm(join(source, "dist"), { recursive: true });
  await writeFile(join(source, "dist"), "a source file is not an excluded directory\n");
  await assert.rejects(invoke, /stale prepared browser input inventory/);
  // Unsafe inputs remain refusals, even at an otherwise excluded name.
  await rm(join(source, "dist"));
  await symlink(join(root, "dist"), join(source, "dist"));
  await assert.rejects(invoke, /linked browser input/);
  await assert.rejects(run(process.execPath, [join(source, "scripts/copy-source.mjs"), source, join(root, "unsafe-copy")]), /Linked build input/);
  await rm(join(source, "dist"));
  await invoke();
});
