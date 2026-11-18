// SPDX-License-Identifier: MIT
// Locked installation inventory, including full packaged license text. Review is external.
import { readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
const [root, output] = process.argv.slice(2);
if (!root || !output) throw new Error("Usage: notices.mjs <installed-project> <output-directory>");
const lock = JSON.parse(await readFile(resolve(root, "package-lock.json"), "utf8"));
// Exact supplements only; acquisition URLs and archive identities remain in notices/acquisition-records.json.
const supplements = {
  "node_modules/@rolldown/binding-linux-arm64-gnu": {
    name: "@rolldown/binding-linux-arm64-gnu", version: "1.2.19",
    qualification: "Official rolldown 1.2.19 archive LICENSE and THIRD-PARTY-LICENSE; same version, repository and package directory as the binding.",
    files: [["rolldown-LICENSE.txt", "23ecfff35a5a2e80d92142f75228912c3b1abc4b5a8337a821ff4397e2f9f734"], ["rolldown-THIRD-PARTY-LICENSE.txt", "a877291d800ed43692f3f9ae09d8e01cc6f7293ad39d43896059c188ffbb8b7c"]]
  },
  "node_modules/@rolldown/binding-linux-x64-gnu": {
    name: "@rolldown/binding-linux-x64-gnu", version: "1.2.19",
    qualification: "Official rolldown 1.2.19 archive LICENSE and THIRD-PARTY-LICENSE; same version, repository and package directory as the binding.",
    files: [["rolldown-LICENSE.txt", "23ecfff35a5a2e80d92142f75228912c3b1abc4b5a8337a821ff4397e2f9f734"], ["rolldown-THIRD-PARTY-LICENSE.txt", "a877291d800ed43692f3f9ae09d8e01cc6f7293ad39d43896059c188ffbb8b7c"]]
  },
  "node_modules/@tailwindcss/oxide-wasm32-wasi": {
    name: "@tailwindcss/oxide-wasm32-wasi", version: "4.3.5",
    qualification: "Official @tailwindcss/oxide 4.3.5 archive MIT notice.",
    files: [["tailwindcss-LICENSE.txt", "60e0b68c0f35c078eef3a5d29419d0b03ff84ec1df9c3f9d6e39a519a5ae7985"]]
  },
  "node_modules/@tailwindcss/oxide-wasm32-wasi/node_modules/@napi-rs/wasm-runtime": {
    name: "@napi-rs/wasm-runtime", version: "1.2.2",
    qualification: "Official repository root LICENSE at matching package gitHead 5b344c367eeed81d1917f7c4d8e65b1adc16de12; omitted from the archive.",
    files: [["napi-rs-LICENSE.txt", "3f1ce66533302df3a32edbfdfc0b78f0dd34659e4c1f5817162e5ea3c2297215"]]
  },
  "node_modules/@tailwindcss/oxide-wasm32-wasi/node_modules/@tybys/wasm-util": {
    name: "@tybys/wasm-util", version: "0.10.3",
    qualification: "Package metadata declares MIT, but its archive and gitHead omit LICENSE. Official immutable commit 51836d7bf26fb7f43b05dd6477b3ca4260c945cf supplies Toyobayashi's MIT notice. The helper credits Node lib/path.js; the exact Node/Joyent MIT excerpt is also retained.",
    files: [["wasm-util-LICENSE.txt", "09e436100bf926e78df875ec80cf3d0c643dfec779b52cfe2aa96afa0de714cb"], ["node-MIT-NOTICES.txt", "86d2d599e5aca4ce609f66171863a5eb405c3a520b27ef869c5740ea12cc9887"]]
  }
};
const packages = [], notices = [], runtimeNotices = [];
// These build packages emit the preload helper, interop helper and CSS preflight.
const emittedBuildPackages = new Set(["node_modules/vite", "node_modules/rolldown", "node_modules/tailwindcss"]);
for (const [path, entry] of Object.entries(lock.packages)) {
  if (!path) continue;
  if (!path.startsWith("node_modules/") || path.includes("..")) throw new Error("Unexpected locked path");
  const directory = resolve(root, path);
  let metadata;
  try { metadata = JSON.parse(await readFile(resolve(directory, "package.json"), "utf8")); }
  catch (error) { if (entry.optional && error.code === "ENOENT") { packages.push({ path, version: entry.version, optional_not_installed: true }); continue; } throw error; }
  if (metadata.version !== entry.version || metadata.name !== path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length)) throw new Error(`Installed identity differs from lock: ${path}`);
  const names = (await readdir(directory)).filter(name => /^(licen[sc]e|copying|notice|third-party-license|thirdpartynotice|copyrightnotice)(\.|$)/i.test(name)).sort();
  const texts = [], emittedBuildTexts = [];
  let emittedLicensePresent = false, interopNoticePresent = false;
  for (const name of names) {
    try {
      const bytes = await readFile(resolve(directory, name));
      const text = bytes.toString("utf8");
      texts.push(text);
      if (emittedBuildPackages.has(path) && /^(licen[sc]e|third-party-license)(\.|$)/i.test(name) && text.trim()) {
        let emittedText = text;
        if (path === "node_modules/vite") {
          // Only this pinned DOM preload helper uses the complete Vite core MIT section.
          // The full packaged license, including build-only bundles, stays in texts above.
          if (metadata.name !== "vite" || metadata.version !== "8.3.8" || entry.dev !== true || entry.integrity !== "sha512-LzOn3h9rG/0vhLdXrV4yJ4QFA9eEoqCl+Yx+kGJd7aB9w6nfNgVHKSYwLPZUHrz4ZGa+t+kXjV9itvcHDLQouA==" || name !== "LICENSE.md") throw new Error("Vite helper notice identity mismatch");
          if (bytes.length !== 98196 || createHash("sha256").update(bytes).digest("hex") !== "a22152baa1aef74402443008e767e7c689a524c0909d19b0431d97f5ab066a13") throw new Error("Vite full license hash mismatch");
          const boundary = bytes.indexOf(Buffer.from("# Licenses of bundled dependencies"));
          const core = bytes.subarray(0, boundary);
          if (!text.startsWith("# Vite core license") || boundary !== 1163 || createHash("sha256").update(core).digest("hex") !== "444c5ec3bf7f798af5c5179d734673cbcb115b22b1ef48e6d85bc941ac8777a2") throw new Error("Vite core MIT section mismatch");
          emittedText = core.toString("utf8");
        }
        emittedBuildTexts.push(emittedText);
        if (/^licen[sc]e(\.|$)/i.test(name)) emittedLicensePresent = true;
        if (/^third-party-license(\.|$)/i.test(name)) interopNoticePresent = true;
      }
    }
    catch (error) { if (error.code !== "EISDIR") throw error; }
  }
  const supplement = supplements[path], supplementalFiles = [];
  if (supplement) {
    if (metadata.name !== supplement.name || metadata.version !== supplement.version) throw new Error(`Supplement identity mismatch: ${path}`);
    for (const [file, sha256] of supplement.files) {
      const bytes = await readFile(resolve(root, "notices", file));
      if (createHash("sha256").update(bytes).digest("hex") !== sha256) throw new Error(`Supplement hash mismatch: ${file}`);
      supplementalFiles.push({ file: `notices/${file}`, sha256 });
      texts.push(bytes.toString("utf8"));
    }
  }
  if (path === "node_modules/typescript") {
    // This embedded vendor notice is absent from the root NOTICE; tooling only.
    if (metadata.name !== "typescript" || metadata.version !== "7.0.2" || entry.dev !== true || entry.integrity !== "sha512-8FYau96o3NKOhbjKi/qNvG/W5jhzxkbdm5sj9AbZ/5T5sWqn3hJgLfGx27sRKZWTvyzCP8dLRBTf5tBTSRVUNA==") throw new Error("Embedded TypeScript notice identity mismatch");
    const file = "vendor/vscode-jsonrpc/License.txt";
    const bytes = await readFile(resolve(directory, file));
    if (createHash("sha256").update(bytes).digest("hex") !== "ec9ee83580841e8eb687aca9867f221503809ba6426c7f876ede17d91b9fcfd0") throw new Error("Embedded TypeScript notice hash mismatch");
    names.push(file);
    texts.push(bytes.toString("utf8"));
  }
  // Bundled package bytes are covered by the enclosing archive, not a separate integrity.
  const archivePath = entry.inBundle ? path.slice(0, path.lastIndexOf("/node_modules/")) : path;
  const archiveIntegrity = lock.packages[archivePath]?.integrity;
  if (!archiveIntegrity) throw new Error(`Missing archive integrity coverage: ${path}`);
  packages.push({ path, name: metadata.name, version: metadata.version, license: metadata.license ?? null, integrity: entry.integrity ?? null, integrity_coverage: { path: archivePath, integrity: archiveIntegrity }, scripts: metadata.scripts ?? {}, notice_files: names, supplemental_notice_files: supplementalFiles, supplemental_notice_qualification: supplement?.qualification ?? null, license_text_present: texts.some(text => text.trim()) });
  const notice = `\n===== ${metadata.name}@${metadata.version} (${metadata.license ?? "UNKNOWN"}) =====\n${texts.join("\n") || "MISSING FULL LICENSE TEXT — review required"}`;
  notices.push(notice);
  if (!entry.dev) runtimeNotices.push(notice);
  else if (emittedBuildPackages.has(path)) {
    if (!emittedLicensePresent) throw new Error(`Missing or empty emitted-code license: ${path}`);
    if (path === "node_modules/rolldown" && !interopNoticePresent) throw new Error("Missing or empty Rolldown interop third-party notices");
    runtimeNotices.push(`\n===== ${metadata.name}@${metadata.version} (emitted helper/style code) =====\n${emittedBuildTexts.join("\n")}`);
  }
  emittedBuildPackages.delete(path);
}
if (emittedBuildPackages.size) throw new Error("Missing installed packages that emit distributed code");
await writeFile(resolve(output, "dependency-inventory.json"), JSON.stringify({ review: "pending", packages }, null, 2) + "\n");
await writeFile(resolve(output, "dependency-notices.txt"), notices.join("\n") + "\n");
await writeFile(resolve(output, "runtime-notices.txt"), runtimeNotices.join("\n") + "\n");
if (packages.some(p => !p.optional_not_installed && (!p.license || !p.license_text_present))) throw new Error("Missing packaged license evidence; review dependency-inventory.json");
