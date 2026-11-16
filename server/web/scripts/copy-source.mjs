// SPDX-License-Identifier: MIT
import { copyFile, lstat, mkdir, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Only root-level dependency/build outputs are absent from the copied web
// source. Keep this directory policy aligned with server/build.rs.
const generated = new Set(["node_modules", "dist", ".test-build", "playwright-report", "test-results"]);

export async function copySource(source, target, root = true) {
  const stat = await lstat(source);
  if (stat.isSymbolicLink()) throw new Error("Linked build input");
  if (stat.isDirectory()) {
    await mkdir(target, { recursive: true });
    for (const name of (await readdir(source)).sort()) {
      const child = join(source, name);
      if (root && generated.has(name) && (await lstat(child)).isDirectory()) continue;
      await copySource(child, join(target, name), false);
    }
  } else {
    if (!stat.isFile()) throw new Error("Non-file build input");
    await copyFile(source, target);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [source, target] = process.argv.slice(2);
  if (!source || !target) throw new Error("Usage: copy-source.mjs <web-source> <destination>");
  await copySource(resolve(source), resolve(target));
}
