// SPDX-License-Identifier: MIT
// Synthetic local fixture only; the executable retains its production origin policy.
import { spawn, execFileSync } from "node:child_process";
import { createServer, request as httpsRequest } from "node:https";
import { request as httpRequest } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { once } from "node:events";
const binary = "/binary/meguri-sync-server", output = "/out";
const packaged = process.argv[2] === "package";
const children = [], servers = [];
let stopping = false;
async function stop(code) {
  if (stopping) return;
  stopping = true;
  for (const server of servers) server.close();
  const exited = children.map(child => child.exitCode !== null ? Promise.resolve() : once(child, "exit"));
  for (const child of children) child.kill("SIGTERM");
  const timer = setTimeout(() => { for (const child of children) child.kill("SIGKILL"); process.exit(code); }, 5000);
  await Promise.allSettled(exited);
  clearTimeout(timer);
  process.exit(code);
}
process.on("SIGTERM", () => void stop(0));
process.on("SIGINT", () => void stop(0));
function api(origin, path, body, cookie = "") {
  return new Promise((resolve, reject) => {
    const request = httpsRequest(origin + path, { rejectUnauthorized: false, method: body === undefined ? "GET" : "POST",
      headers: { Origin: origin, ...(cookie ? { Cookie: cookie } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) } }, response => {
      const chunks = [];
      response.on("data", chunk => chunks.push(chunk));
      response.on("error", reject);
      response.on("end", () => {
        if (response.statusCode < 200 || response.statusCode >= 300) return reject(new Error(`Synthetic API ${path} returned ${response.statusCode}`));
        try { resolve({ data: JSON.parse(Buffer.concat(chunks)).data, cookie: response.headers["set-cookie"]?.[0]?.split(";")[0] }); }
        catch { reject(new Error("Invalid synthetic API response")); }
      });
    });
    request.setTimeout(15000, () => request.destroy(new Error("Synthetic request timed out")));
    request.on("error", reject);
    request.end(body === undefined ? undefined : JSON.stringify(body));
  });
}
async function start(name, port) {
  const origin = `https://localhost:${port}`, root = packaged ? "/data/owner" : `/fixtures/${randomUUID()}`;
  if (!packaged) execFileSync(binary, ["init", root, origin], { stdio: "ignore" });
  const token = (await readFile(`${root}/state/setup-token`, "utf8")).trim();
  if (!packaged) {
  const child = spawn(binary, ["serve", root, origin, `127.0.0.1:${port + 1}`, "--trusted-peer", "127.0.0.1"], { stdio: ["ignore", "pipe", "pipe"] });
  children.push(child);
  // Wait for the actual executable's listening event, without request retries.
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Synthetic server did not listen")), 30000);
    let text = "";
    child.stdout.on("data", chunk => { text += chunk; if (text.includes("listening ")) { clearTimeout(timer); resolve(); } });
    child.once("error", error => { clearTimeout(timer); reject(error); });
    child.once("exit", () => { clearTimeout(timer); reject(new Error("Synthetic server exited before readiness")); });
  });
  child.on("exit", () => { if (!stopping) void stop(1); });
  }
  // Never forward user-supplied proxy authority. The backend stays in the owned namespace.
  const proxy = createServer({ key: await readFile("/tmp/tls/key.pem"), cert: await readFile("/tmp/tls/cert.pem") }, (incoming, outgoing) => {
    const headers = { ...incoming.headers, "x-forwarded-for": "127.0.0.1", "x-forwarded-proto": "https" };
    delete headers["x-forwarded-host"];
    const upstream = httpRequest({ hostname: "127.0.0.1", port: packaged ? 8080 : port + 1, method: incoming.method, path: incoming.url, headers }, response => {
      outgoing.writeHead(response.statusCode, response.headers); response.pipe(outgoing);
    });
    upstream.on("error", () => { if (!outgoing.headersSent) outgoing.writeHead(502); outgoing.end(); });
    incoming.on("aborted", () => upstream.destroy());
    outgoing.on("close", () => upstream.destroy());
    incoming.pipe(upstream);
  });
  servers.push(proxy);
  proxy.listen(port, "0.0.0.0"); await once(proxy, "listening");
  const directory = `${output}/${name}`;
  await mkdir(directory, { recursive: true });
  const descriptor = { origin, setup_token: token };
  await writeFile(`${directory}/fixture.json`, JSON.stringify(descriptor, null, 2), { mode: 0o600 });
  return descriptor;
}
try {
  await mkdir("/tmp/tls");
  execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "7", "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1", "-keyout", "/tmp/tls/key.pem", "-out", "/tmp/tls/cert.pem"], { stdio: "ignore" });
  if (packaged) {
    // Exact-image mode supplies only the existing full browser journey. The
    // service owns its volume and process in its separate scratch container.
    await start("test", 18447);
  } else {
  const preview = await start("preview", 18443);
  const setup = await start("setup", 18445);
  const test = await start("test", 18447);
  const admin = { username: "preview.admin", password: "Synthetic admin password 2026" };
  const member = { username: "preview.member", password: "Synthetic member password 2026" };
  await api(preview.origin, "/api/setup", { token: preview.setup_token, ...admin });
  const login = await api(preview.origin, "/api/login", admin);
  const invitation = await api(preview.origin, "/api/admin/invitations", {}, login.cookie);
  await api(preview.origin, "/api/invitations/claim", { token: new URL(invitation.data.url).hash.slice(7), ...member });
  // Exercise member authentication as well as claim, without changing preview UI state.
  const memberLogin = await api(preview.origin, "/api/login", member);
  await api(preview.origin, "/api/me", undefined, memberLogin.cookie);
  const unusedInvite = await api(preview.origin, "/api/admin/invitations", {}, login.cookie);
  const accounts = await api(preview.origin, "/api/admin/accounts", undefined, login.cookie);
  const memberId = accounts.data.accounts.find(account => account.username === member.username).id;
  const reset = await api(preview.origin, `/api/admin/accounts/${memberId}/reset`, {}, login.cookie);
  await api(preview.origin, "/api/logout", {}, login.cookie);
  const identity = { binary_sha256: createHash("sha256").update(await readFile(binary)).digest("hex"),
    assets: JSON.parse(await readFile("/binary/assets.json", "utf8")),
    preview: { origin: preview.origin, admin, member, member_authenticated: true, invite: unusedInvite.data, reset: reset.data },
    setup: { origin: setup.origin }, test: { origin: test.origin } };
  await writeFile(`${output}/preview.json`, JSON.stringify(identity, null, 2), { mode: 0o600 });
  }
  await writeFile(`${output}/ready`, "ready\n");
} catch (error) {
  // Do not put synthetic capability URLs or API bodies in ordinary service logs.
  await writeFile(`${output}/fixture-failure.txt`, "Synthetic fixture initialization failed; no readiness published.\n");
  console.error(error instanceof Error ? error.message : "Synthetic fixture initialization failed");
  await stop(1);
}
