// SPDX-License-Identifier: MIT
import { watch } from "node:fs";
import { access } from "node:fs/promises";
// Filesystem readiness wait only: no API request is retried.
const watcher = watch("/out");
const timer = setTimeout(() => { watcher.close(); process.exit(1); }, 90000);
async function check() {
  try { await access("/out/ready"); clearTimeout(timer); watcher.close(); process.exit(0); } catch {}
  try { await access("/out/fixture-failure.txt"); clearTimeout(timer); watcher.close(); process.exit(1); } catch {}
}
watcher.on("change", () => void check());
await check();
