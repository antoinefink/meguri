// SPDX-License-Identifier: MIT
import { readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
const [lockPath, output] = process.argv.slice(2);
const lock = JSON.parse(await readFile(lockPath, "utf8"));
const results = [];
for (const [path, entry] of Object.entries(lock.packages)) {
  if (!path) continue;
  const name = path.split("node_modules/").at(-1);
  const metadata = JSON.parse(execFileSync("npm", ["view", `${name}@${entry.version}`, "name", "version", "engines", "license", "dist", "optionalDependencies", "time", "--fetch-retries=0", "--json"], { encoding: "utf8" }));
  results.push({ path, metadata });
  await writeFile(output, JSON.stringify(results, null, 2) + "\n");
}
