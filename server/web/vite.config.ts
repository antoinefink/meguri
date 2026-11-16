// SPDX-License-Identifier: MIT
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
const records = JSON.parse(readFileSync(new URL("./upstream-records.json", import.meta.url), "utf8"));
for (const record of records.inputs) {
  const bytes = readFileSync(new URL(record.adapted_into, import.meta.url));
  if (createHash("sha256").update(bytes).digest("hex") !== record.adapted_sha256) throw new Error("Adapted source hash differs: " + record.adapted_into);
}
const noticePath = process.env.MEGURI_WEB_NOTICES_FILE;
if (!noticePath) throw new Error("Generate dependency notices before building");
const shadcnNotice = readFileSync(new URL("./THIRD_PARTY_NOTICES.txt", import.meta.url), "utf8");
const runtimeNotices = readFileSync(noticePath, "utf8");
if (!shadcnNotice.trim() || !runtimeNotices.trim()) throw new Error("Required license notices are empty");
const notices = "SPDX-License-Identifier: MIT\n" + shadcnNotice + "\n" + runtimeNotices;
if (notices.includes("*/")) throw new Error("License notice needs safe banner encoding review");
export default defineConfig({
  plugins: [tailwindcss()],
  build: { sourcemap: false, assetsInlineLimit: 0, cssCodeSplit: false, manifest: true,
    rolldownOptions: { output: { postBanner: "/*!\n" + notices + "\n*/", entryFileNames: "assets/[name]-[hash].js", chunkFileNames: "assets/[name]-[hash].js", assetFileNames: "assets/[name]-[hash][extname]" } } },
});
