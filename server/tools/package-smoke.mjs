// SPDX-License-Identifier: MIT
// Private HTTPS fixture, adapted from the existing browser fixture. The server
// process lives in the actual runtime container; this image supplies clients only.
import { createServer, request as httpsRequest } from 'node:https';
import { request as httpRequest } from 'node:http';
import { execFileSync } from 'node:child_process';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { createHash, randomBytes, X509Certificate } from 'node:crypto';
import { once } from 'node:events';
import assert from 'node:assert/strict';
const phase = process.argv[2];
// The accountless diagnostic remains health-only. The explicit owned journey
// shares its route/TLS lifecycle, with disposable local API setup before ready.
if (phase === 'native-route-fixture' || phase === 'native-owned-journey-fixture') {
  const journey = phase === 'native-owned-journey-fixture';
  let nativeProxy;
  // Catch reports only the current source-literal stage, never exception data.
  let nativeStage = 'MEGURI_NATIVE_FIXTURE_STAGE_ARGUMENTS_SOURCE_BINDING';
  try {
    assert.equal(process.argv.length, 12);
    const [gateway, origin, source, run, owner, runtime, tool, scriptSha, fixture] = process.argv.slice(3);
    const octets = gateway.split('.');
    assert.equal(octets.length, 4);
    assert(octets.every(x => /^(0|[1-9][0-9]{0,2})$/.test(x) && Number(x) <= 255));
    const [a, b] = octets.map(Number);
    assert(a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168));
    const url = new URL(origin);
    assert.equal(url.hostname, gateway); assert.equal(url.protocol, 'https:');
    assert(/^[1-9][0-9]{3,4}$/.test(url.port) && Number(url.port) >= 1024 && Number(url.port) <= 65535);
    assert.equal(origin, `https://${gateway}:${url.port}`);
    assert(/^[0-9a-f]{40}$/.test(source)); assert(/^[A-Za-z0-9_-]{1,100}$/.test(run));
    if (journey) assert(/^[A-Za-z0-9][A-Za-z0-9_-]{0,95}$/.test(run));
    assert(/^[0-9a-f]{32}$/.test(owner)); assert(/^[0-9a-f]{64}$/.test(runtime));
    assert(/^sha256:[0-9a-f]{64}$/.test(tool)); assert(/^[0-9a-f]{64}$/.test(scriptSha));
    assert(/^meguri-native-fixture-[A-Za-z0-9_-]{1,100}$/.test(fixture));
    assert.equal(createHash('sha256').update(await readFile('/smoke/package-smoke.mjs')).digest('hex'), scriptSha);
    nativeStage = 'MEGURI_NATIVE_FIXTURE_STAGE_CERTIFICATE';
    await mkdir('/tmp/native-route', { mode: 0o700 });
    const openssl = args => execFileSync('openssl', args, { stdio: 'ignore', timeout: 15000,
      env: { PATH: '/usr/bin:/bin', OPENSSL_CONF: '/dev/null' } });
    openssl(['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
      '-subj', '/CN=Meguri Native Route CA', '-addext', 'basicConstraints=critical,CA:TRUE',
      '-addext', 'keyUsage=critical,keyCertSign', '-keyout', '/tmp/native-route/ca-key.pem',
      '-out', '/tmp/native-route/root-ca.pem']);
    openssl(['req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
      '-keyout', '/tmp/native-route/leaf-key.pem', '-out', '/tmp/native-route/leaf.csr']);
    await writeFile('/tmp/native-route/leaf.ext',
      `basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1,IP:${gateway}\n`,
      { mode: 0o600, flag: 'wx' });
    openssl(['x509', '-req', '-in', '/tmp/native-route/leaf.csr', '-CA', '/tmp/native-route/root-ca.pem',
      '-CAkey', '/tmp/native-route/ca-key.pem', '-set_serial', '1', '-days', '1',
      '-extfile', '/tmp/native-route/leaf.ext', '-out', '/tmp/native-route/leaf.pem']);
    const root = await readFile('/tmp/native-route/root-ca.pem');
    const leaf = await readFile('/tmp/native-route/leaf.pem');
    const ca = new X509Certificate(root); const server = new X509Certificate(leaf);
    assert.equal(ca.ca, true); assert.equal(server.ca, false);
    assert.equal(server.subjectAltName, `DNS:localhost, IP Address:127.0.0.1, IP Address:${gateway}`);
    assert.equal(server.checkIP(gateway), gateway); assert.equal(server.checkIP('127.0.0.1'), '127.0.0.1');
    assert.equal(server.checkHost('localhost', { subject: 'never' }), 'localhost');
    assert(ca.verify(ca.publicKey)); assert(server.verify(ca.publicKey));
    for (const c of [ca, server]) {
      const start = Date.parse(c.validFrom); const end = Date.parse(c.validTo);
      assert(end > Date.now() && start <= Date.now() && end - start === 86400000);
    }
    await writeFile('/tmp/native-route/binding.json', JSON.stringify({ gateway, origin, source, run, owner,
      runtime, tool, scriptSha, fixture, caSha: createHash('sha256').update(root).digest('hex'), caBytes: root.length }),
      { mode: 0o600, flag: 'wx' });
    nativeStage = 'MEGURI_NATIVE_FIXTURE_STAGE_HTTPS_LISTENER';
    // Exact first-party protocol methods (protocol/mod.rs and media.rs).
    // No API/setup/browser path is exposed by the journey's public proxy.
    const sync = new Set(['hostKey', 'meta', 'upload', 'download', 'start', 'applyGraves',
      'applyChanges', 'chunk', 'applyChunk', 'sanityCheck2', 'finish', 'abort']);
    const media = new Set(['begin', 'mediaChanges', 'uploadChanges', 'downloadFiles', 'mediaSanity']);
    nativeProxy = createServer({ key: await readFile('/tmp/native-route/leaf-key.pem'), cert: leaf }, (incoming, outgoing) => {
      const path = incoming.url;
      const allowed = incoming.method === 'GET' && path === '/health'
        || journey && incoming.method === 'POST' && (
          path.startsWith('/sync/') && sync.has(path.slice(6))
          || path.startsWith('/msync/') && media.has(path.slice(7)));
      if (!allowed) { outgoing.writeHead(404); outgoing.end(); return; }
      const headers = { ...incoming.headers, 'x-forwarded-for': '127.0.0.1', 'x-forwarded-proto': 'https' };
      delete headers['x-forwarded-host'];
      const upstream = httpRequest({ hostname: '127.0.0.1', port: 8080, method: incoming.method, path: incoming.url, headers }, response => {
        outgoing.writeHead(response.statusCode, response.headers); response.pipe(outgoing);
      });
      upstream.on('error', () => { if (!outgoing.headersSent) outgoing.writeHead(502); outgoing.end(); });
      incoming.on('aborted', () => upstream.destroy()); outgoing.on('close', () => upstream.destroy()); incoming.pipe(upstream);
    });
    nativeProxy.listen(18443, '0.0.0.0'); await once(nativeProxy, 'listening');
    if (journey) {
      nativeStage = 'MEGURI_NATIVE_FIXTURE_STAGE_SYNTHETIC_SETUP';
      // The image-owned volume is mounted read-only only in this phase. Setup
      // and identity checks go to the actual API over the internal loopback;
      // no setup token, cookie or response is logged or passed as an argument.
      const local = (path, body, cookie = '') => new Promise((resolve, reject) => {
        const req = httpRequest({ hostname: '127.0.0.1', port: 8080, path,
          method: body === undefined ? 'GET' : 'POST', headers: {
            Origin: origin, 'x-forwarded-proto': 'https',
            ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
            ...(cookie ? { Cookie: cookie } : {}),
          } }, response => {
          const chunks = [];
          response.on('data', chunk => chunks.push(chunk)); response.on('error', reject);
          response.on('end', () => resolve({ status: response.statusCode,
            headers: response.headers, bytes: Buffer.concat(chunks) }));
        });
        req.setTimeout(30000, () => req.destroy(new Error('Request deadline')));
        req.on('error', reject); req.end(body === undefined ? undefined : JSON.stringify(body));
      });
      const username = `owned.${randomBytes(12).toString('hex')}`;
      const password = randomBytes(32).toString('base64url');
      const token = (await readFile('/data/owner/state/setup-token', 'utf8')).trim();
      assert.equal((await local('/api/setup', { token, username, password })).status, 201);
      const login = await local('/api/login', { username, password });
      assert.equal(login.status, 200);
      assert.equal(login.headers['set-cookie'].length, 1);
      const cookie = login.headers['set-cookie'][0].split(';')[0];
      const me = await local('/api/me', undefined, cookie); assert.equal(me.status, 200);
      const account = JSON.parse(me.bytes).data;
      assert.equal(account.username, username); assert.equal(account.role, 'admin');
      assert.equal(typeof account.id, 'string'); assert(account.id.length > 0);
      assert.equal((await local('/api/logout', {}, cookie)).status, 200);
      await writeFile('/tmp/native-route/journey.json', JSON.stringify({
        run_id: run, username, password, account_id: account.id,
      }) + '\n', { mode: 0o600, flag: 'wx' });
    }
    nativeStage = 'MEGURI_NATIVE_FIXTURE_STAGE_READINESS';
    await new Promise(resolve => {
      const stop = () => {
        process.removeListener('SIGTERM', stop); process.removeListener('SIGINT', stop); resolve();
      };
      process.once('SIGTERM', stop); process.once('SIGINT', stop);
      console.log('MEGURI_NATIVE_ROUTE_READY');
    });
  } catch {
    console.error(nativeStage);
    console.error('Packaged service smoke failed: native-route-fixture'); process.exitCode = 1;
  } finally {
    if (nativeProxy) { nativeProxy.closeAllConnections(); await new Promise(resolve => nativeProxy.close(resolve)); }
  }
  process.exit(process.exitCode || 0);
}
const browserMode = process.argv[3] === 'browser';
assert(['seed', 'check', 'provider-fixture'].includes(phase));
const providerFixture = phase === 'provider-fixture';
if (providerFixture) assert.equal(process.argv.length, 3);
const origin = 'https://localhost:18443';
const credentials = { username: 'package.admin', password: 'Synthetic package password 2026' };
let cert;
const request = (path, body, cookie = '', extra = {}) => new Promise((resolve, reject) => {
  const req = httpsRequest(origin + path, { ca: cert,
    family: 4, method: body === undefined ? 'GET' : 'POST', headers: { Origin: origin,
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(cookie ? { Cookie: cookie } : {}), ...extra } }, response => {
    const chunks = [];
    response.on('data', chunk => chunks.push(chunk)); response.on('error', reject);
    response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, bytes: Buffer.concat(chunks) }));
  });
  req.setTimeout(30000, () => req.destroy(new Error('Request deadline')));
  req.on('error', reject); req.end(body === undefined ? undefined : JSON.stringify(body));
});
let proxy;
let browserStep = 'launch';
try {
  await mkdir('/tmp/tls');
  if (providerFixture) {
    execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
      '-subj', '/CN=Meguri Provider Fixture CA', '-addext', 'basicConstraints=critical,CA:TRUE',
      '-addext', 'keyUsage=critical,keyCertSign', '-keyout', '/tmp/tls/ca-key.pem', '-out', '/tmp/tls/ca.pem'], { stdio: 'ignore' });
    execFileSync('openssl', ['req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
      '-keyout', '/tmp/tls/key.pem', '-out', '/tmp/tls/server.csr'], { stdio: 'ignore' });
    await writeFile('/tmp/tls/server.ext', 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n',
      { mode: 0o600, flag: 'wx' });
    execFileSync('openssl', ['x509', '-req', '-in', '/tmp/tls/server.csr', '-CA', '/tmp/tls/ca.pem',
      '-CAkey', '/tmp/tls/ca-key.pem', '-set_serial', '1', '-days', '1', '-extfile', '/tmp/tls/server.ext',
      '-out', '/tmp/tls/cert.pem'], { stdio: 'ignore' });
  } else {
    execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=localhost',
      '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1', '-keyout', '/tmp/tls/key.pem', '-out', '/tmp/tls/cert.pem'], { stdio: 'ignore' });
  }
  const serverCert = await readFile('/tmp/tls/cert.pem');
  cert = providerFixture ? await readFile('/tmp/tls/ca.pem') : serverCert;
  if (providerFixture) {
    const caCertificate = new X509Certificate(cert);
    const serverCertificate = new X509Certificate(serverCert);
    assert(caCertificate.ca === true, 'Provider fixture CA role required');
    assert(serverCertificate.ca === false, 'Provider fixture server leaf role required');
    assert(serverCertificate.checkHost('localhost', { subject: 'never' }) === 'localhost', 'Provider fixture localhost SAN required');
    assert(serverCertificate.verify(caCertificate.publicKey), 'Provider fixture CA signature required');
  }
  proxy = createServer({ key: await readFile('/tmp/tls/key.pem'), cert: serverCert }, (incoming, outgoing) => {
    const headers = { ...incoming.headers, 'x-forwarded-for': '127.0.0.1', 'x-forwarded-proto': 'https' };
    delete headers['x-forwarded-host'];
    const upstream = httpRequest({ hostname: '127.0.0.1', port: 8080, method: incoming.method, path: incoming.url, headers }, response => {
      outgoing.writeHead(response.statusCode, response.headers); response.pipe(outgoing);
    });
    upstream.on('error', () => { if (!outgoing.headersSent) outgoing.writeHead(502); outgoing.end(); });
    incoming.on('aborted', () => upstream.destroy()); outgoing.on('close', () => upstream.destroy()); incoming.pipe(upstream);
  });
  proxy.listen(18443, '127.0.0.1'); await once(proxy, 'listening');
  if (providerFixture) {
    // This lane provisions only its fresh, disposable packaged runtime. Clients
    // execute in other containers, leaving this HTTPS proxy's event loop free.
    const username = 'provider.fixture';
    const password = randomBytes(32).toString('base64url');
    const token = (await readFile('/data/owner/state/setup-token', 'utf8')).trim();
    const setup = await request('/api/setup', { token, username, password });
    assert.equal(setup.status, 201);
    await mkdir('/tmp/provider-fixture', { mode: 0o700 });
    execFileSync('openssl', ['x509', '-in', '/tmp/tls/ca.pem', '-outform', 'DER',
      '-out', '/tmp/provider-fixture/ca.der'], { stdio: 'ignore' });
    // Set an explicit private mode (OpenSSL's output follows umask).
    const der = await readFile('/tmp/provider-fixture/ca.der');
    const { chmod } = await import('node:fs/promises');
    await chmod('/tmp/provider-fixture/ca.der', 0o600);
    assert(der.length > 0);
    await writeFile('/tmp/provider-fixture/input.json', JSON.stringify({
      format: 1, origin, username, password, now_ms: Date.now(),
    }) + '\n', { mode: 0o600, flag: 'wx' });
    await new Promise(resolve => {
      const stop = () => {
        process.removeListener('SIGTERM', stop); process.removeListener('SIGINT', stop); resolve();
      };
      process.once('SIGTERM', stop); process.once('SIGINT', stop);
      console.log('MEGURI_PROVIDER_FIXTURE_READY');
    });
  } else if (browserMode) {
    // Installed locked tooling lives only in the existing browser container.
    const { chromium, expect } = await import('@playwright/test');
    const browser = await chromium.launch({ headless: true, timeout: 30000 });
    try {
      const context = await browser.newContext({ baseURL: origin, ignoreHTTPSErrors: true });
      context.setDefaultTimeout(8000); context.setDefaultNavigationTimeout(30000);
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.name));
      // Each step waits for the API answer its next screen depends on, so a
      // failure names the step and the answer's status (never its body).
      const answered = async (step, path, act) => {
        browserStep = step;
        const response = page.waitForResponse(r => new URL(r.url()).pathname === path);
        response.catch(() => {}); // Awaited below; a failed act() must not leave it unhandled.
        await act();
        const answer = await response, status = answer.status();
        if (status < 200 || status > 299) {
          // Only the server's fixed error code (for example storage_unavailable), never other body text.
          const code = await answer.json().then(body => body?.error?.code, () => undefined);
          browserStep = `${step}: ${path} answered ${status} ${/^[a-z_]{1,40}$/.test(code ?? '') ? code : 'without an error code'}`;
          throw new Error(browserStep);
        }
      };
      if (phase === 'seed') {
        browserStep = 'setup form';
        const token = (await readFile('/data/owner/state/setup-token', 'utf8')).trim();
        await page.goto(`/setup#token=${encodeURIComponent(token)}`);
        await expect(page.getByRole('heading', { name: 'Set up Meguri', exact: true })).toBeVisible();
        await page.getByLabel('Username', { exact: true }).fill(credentials.username);
        await page.getByLabel('New password', { exact: true }).fill(credentials.password);
        await page.getByLabel('Confirm new password', { exact: true }).fill(credentials.password);
        await answered('setup', '/api/setup', () => page.getByRole('button', { name: 'Set up Meguri', exact: true }).click());
        await expect(page.getByRole('heading', { name: 'Sign in', exact: true })).toBeVisible();
      }
      browserStep = 'login form';
      await page.goto('/login');
      await expect(page.getByRole('heading', { name: 'Sign in', exact: true })).toBeVisible();
      await page.getByLabel('Username', { exact: true }).fill(credentials.username);
      await page.getByLabel('Password', { exact: true }).fill(credentials.password);
      await answered('login', '/api/login', () => page.getByRole('button', { name: 'Sign in', exact: true }).click());
      browserStep = 'collection';
      await expect(page).toHaveURL(/\/collection$/);
      await expect(page.getByRole('heading', { name: 'Collection', exact: true })).toBeVisible();
      await answered('connection', '/api/me/mcp-grants', () => page.getByRole('link', { name: 'Connection', exact: true }).click());
      await expect(page.getByRole('heading', { name: 'Connect your devices', exact: true })).toBeVisible();
      await expect(page.getByLabel('Server URL', { exact: true })).toHaveValue(`${origin}/`);
      await expect(page.getByLabel('Username', { exact: true })).toHaveValue(credentials.username);
      const cookie = (await context.cookies()).find(item => item.name === '__Host-meguri-session');
      assert(cookie?.secure && cookie.httpOnly && cookie.sameSite === 'Strict');
      await answered('accounts', '/api/admin/accounts', () => page.getByRole('link', { name: 'Accounts', exact: true }).click());
      await expect(page.getByRole('heading', { name: `${credentials.username} (you)`, exact: true })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Create invitation', exact: true })).toBeVisible();
      await page.screenshot({ path: `/out/browser-${phase}.png`, fullPage: true });
      browserStep = 'sign out';
      await page.getByRole('button', { name: 'Sign out', exact: true }).click();
      await expect(page.getByRole('heading', { name: 'Sign in', exact: true })).toBeVisible();
      assert.deepEqual(errors, []);
      await writeFile(`/out/browser-${phase}.json`, JSON.stringify({ phase,
        checks: [phase === 'seed' ? 'rendered-setup-form' : 'account-after-runtime-restart',
          'rendered-login-form', 'connection-details', 'secure-cookie', 'admin-account', 'sign-out'],
        browser: browser.version() }) + '\n');
    } finally { await browser.close(); }
  } else {
    const health = await request('/health'); assert.equal(health.status, 200);
    const login = await request('/api/login', credentials); assert.equal(login.status, 200);
    const setCookie = login.headers['set-cookie']; assert.equal(setCookie.length, 1);
    assert.match(setCookie[0], /; Secure/); assert.match(setCookie[0], /; HttpOnly/); assert.match(setCookie[0], /SameSite=Strict/);
    const cookie = setCookie[0].split(';')[0];
    const me = await request('/api/me', undefined, cookie); assert.equal(me.status, 200);
    assert.equal(JSON.parse(me.bytes).data.username, credentials.username);
    // The proxy strips spoofed authority; the exact browser Origin still governs.
    const badOrigin = await request('/api/login', credentials, '', { Origin: 'https://wrong.example.test' });
    assert.equal(badOrigin.status, 403);
    const assets = JSON.parse(await readFile('/package/assets.json', 'utf8'));
    assert(assets.some(asset => asset.path.endsWith('.js')));
    for (const asset of assets) {
      const response = await request(asset.path === '/index.html' ? '/login' : asset.path);
      assert.equal(response.status, 200); assert.equal(response.bytes.length, asset.bytes);
      assert.equal(createHash('sha256').update(response.bytes).digest('hex'), asset.sha256);
    }
    execFileSync('/smoke/native', [phase], { stdio: ['ignore', 'inherit', 'inherit'] });
    const logout = await request('/api/logout', {}, cookie); assert.equal(logout.status, 200);
    await writeFile(`/out/${phase}.json`, JSON.stringify({ phase, checks: ['health', 'https-login', 'secure-cookie', 'account', 'origin', 'embedded-assets', 'native-collection-review-media'], assets: assets.length }) + '\n');
  }
} catch {
  // No API bodies, secrets or capability URLs in ordinary logs.
  console.error(`Packaged service smoke failed: ${browserMode ? `browser-${phase} at ${browserStep}` : phase}`); process.exitCode = 1;
} finally {
  if (proxy) { proxy.closeAllConnections(); await new Promise(resolve => proxy.close(resolve)); }
}
