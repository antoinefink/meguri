// SPDX-License-Identifier: MIT
import test from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { cp, lstat, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual, promisify } from "node:util";

const run = promisify(execFile);
// This test executes from .test-build/tests/, as compiled by tsconfig.tests.json.
const web = fileURLToPath(new URL("../../", import.meta.url));
const generated = new Set(["node_modules", "dist", ".test-build", "playwright-report", "test-results"]);

async function inputSnapshot(directory: string, prefix = ""): Promise<[string, Buffer][]> {
  const files: [string, Buffer][] = [];
  for (const name of (await readdir(directory)).sort()) {
    const path = join(directory, name);
    // Read only the prepared snapshot, independently of its input-selection logic.
    if ((await lstat(path)).isDirectory()) files.push(...await inputSnapshot(path, `${prefix}${name}/`));
    else files.push([prefix + name, await readFile(path)]);
  }
  return files;
}

for (const change of [
  { name: "editing a public file under tests", path: "tests/prepared-input.fixture.html", before: '<div class="text-red-500"></div>\n', after: '<div class="text-blue-500"></div>\n' },
  { name: "adding a public file under tests", path: "tests/new-prepared-input.fixture.html", before: undefined, after: '<div class="text-green-500"></div>\n' },
  { name: "changing .gitignore discovery configuration", path: ".gitignore", before: "node_modules/\n", after: "node_modules/\ntests/\n" },
]) {
  test(`prepared input binding changes after ${change.name}`, async t => {
    const root = await mkdtemp(join(tmpdir(), "meguri-web-inputs-"));
    t.after(() => rm(root, { recursive: true, force: true }));
    const source = join(root, "web"), dist = join(root, "dist"), notices = join(root, "runtime-notices.txt");
    await cp(web, source, {
      recursive: true,
      filter: path => !generated.has(relative(web, path).split(sep)[0] ?? ""),
    });
    if (change.before !== undefined) await writeFile(join(source, change.path), change.before);

    // Synthetic dist data exercises preparation only; it is never built or served.
    const runtimeNotice = "Synthetic dependency notice for the preparation regression.\n";
    await writeFile(notices, runtimeNotice);
    await mkdir(join(dist, "assets"), { recursive: true });
    await writeFile(join(dist, "index.html"), '<script type="module" src="/assets/fixture-12345678.js"></script>\n');
    await writeFile(join(dist, "assets/fixture-12345678.js"), Buffer.concat([
      Buffer.from("/*!\n"), await readFile(join(source, "THIRD_PARTY_NOTICES.txt")),
      Buffer.from(`\n${runtimeNotice}*/\nexport {};\n`),
    ]));
    async function prepare(name: string) {
      const output = join(root, name);
      await run(process.execPath, [join(source, "scripts/embed.mjs"), dist, output], {
        cwd: source,
        env: { ...process.env, MEGURI_WEB_NOTICES_FILE: notices },
        timeout: 30_000,
      });
      return inputSnapshot(join(output, "inputs"));
    }
    const before = await prepare("before");
    assert.ok(before.length > 0, "the real embed script must produce a nonempty input snapshot");
    await writeFile(join(source, change.path), change.after);
    const after = await prepare("after");
    assert.ok(!isDeepStrictEqual(before, after), `${change.path} changed but the prepared input binding stayed identical`);
  });
}
