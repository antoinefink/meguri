# Sync endpoints, server choices and login

Spec: sync/login-endpoints
Requirements: R1500, R1501 (core: plain HTTP allowed), R1502 (core holds no endpoint state); core parts of R1496, R1497, R1498, R1499, R2031, R2032
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/transport/

What a sync endpoint is and how a user-entered server URL becomes one, which servers a build offers, what the core keeps (nothing) and what each job is given, and how login turns a user name and password into a host key. The HTTP exchange itself is `transport.md`.

## L1. Endpoint normalisation (R1500)

A base URL for sync comes from the AnkiWeb default (L3), from a redirect (`transport.md` T7) or from text the user typed for a self-hosted server. The core offers one function that turns such text into a base or refuses it; the app calls it to validate the settings field (U151) and passes only its result to jobs. The steps, in order:

1. Remove white space at both ends. Empty text is refused (`empty`).
2. Parse the rest as an absolute URL with the WHATWG URL Standard (the parser browsers and the `url` crate use). A failure is refused (`unparseable`). Text without a scheme fails here or in step 3: `192.168.1.5:8080` does not parse, and `sync.local:8080` parses with the scheme `sync.local`.
3. The scheme must be `http` or `https` (the parser lowers its case). Any other scheme is refused (`unsupported_scheme`).
4. The URL must have a non-empty host (`missing_host`).
5. A user name or password in the URL is refused (`credentials`): credentials never sit in a stored URL.
6. A query or a fragment is refused (`query_or_fragment`), even an empty one (`https://h/?`).
7. If the path does not end with `/`, append `/`.
8. The result is the parser's serialisation: scheme and host in lower case, a default port (80 for `http`, 443 for `https`) dropped, an international host name in its ASCII form, the path as the parser percent-encodes it.

| Text | Base or refusal |
|---|---|
| `http://h:8080` | `http://h:8080/` |
| `https://h/anki` | `https://h/anki/` |
| `https://h/anki/` | `https://h/anki/` |
| `  http://192.168.1.10:8080  ` | `http://192.168.1.10:8080/` |
| `HTTPS://Sync.Example.ORG:443/Anki` | `https://sync.example.org/Anki/` |
| `http://[::1]:27701` | `http://[::1]:27701/` |
| `https://h/a b` | `https://h/a%20b/` |
| `ftp://h` | refused: `unsupported_scheme` |
| `h:8080` | refused: `unsupported_scheme` |
| `192.168.1.10:8080` | refused: `unparseable` |
| `http://` | refused: `unparseable` |
| `http://user:pw@h/` | refused: `credentials` |
| `https://h/anki?x=1` | refused: `query_or_fragment` |
| (empty or only spaces) | refused: `empty` |

- Plain `http` is accepted for any host (R1501); asking for Local Network permission is the app's (U7, U160).
- Anki's desktop accepts any URL its parser reads, drops a query when it builds method URLs, and turns the text into a base by resolving `./` against it, which keeps the URL up to its last `/` (`https://h/anki` becomes `https://h/`). Meguri appends the `/` instead, as R1500 asks; steps 3, 5 and 6 are Meguri's own stricter reading.

## L2. Method URLs

A base ends with `/`. A collection method's URL is the base followed by `sync/` and the method name; a media method's is the base followed by `msync/` and the method name (`transport.md` T1).

| Base | `meta` | media `begin` |
|---|---|---|
| `http://h:8080/` | `http://h:8080/sync/meta` | `http://h:8080/msync/begin` |
| `https://h/anki/` | `https://h/anki/sync/meta` | `https://h/anki/msync/begin` |

The official server serves `/sync/` and `/msync/` at the root of its address; a subpath works when a reverse proxy in front of it strips the prefix.

## L3. Server choices (R1496, R1497, R2031, R2032)

- The core exposes the list of server kinds the build offers, in this order:

  | Kind | Present | Base |
  |---|---|---|
  | `ankiweb` | only with feature `ankiweb` | the AnkiWeb default endpoint `https://sync.ankiweb.net/`, or a redirect target that T7 accepted |
  | `self_hosted` | always | the user's URL after L1, or a redirect target that T7 accepted |

- Without feature `ankiweb` the list holds only `self_hosted`, and the kind `ankiweb` does not exist at all (not even as a name in debug output). The AnkiWeb default endpoint, the AnkiWeb host suffix `ankiweb.net` of `transport.md` T7 and the AnkiWeb upload limits (U109) exist only with the feature. A case-insensitive byte scan for `ankiweb` of the core library built without `ankiweb` and without `test-hooks` finds nothing (U116 checks the app as a whole).
- The words the UI shows for each kind ("Self-hosted sync server" and so on) are the app's (U151, U160); the core gives kinds only.
- Without the feature, the core cannot tell an AnkiWeb URL that a user types as a self-hosted server from any other URL without naming AnkiWeb, and it does not try.

## L4. Endpoint state (R1502, KTD12)

- The core keeps no endpoint, host key or redirect between jobs. Every job is given: the base, the server kind, the host key (every job but login), the stall timeout (`transport.md` T8), the proxy or none (T10) and a cancel token (T9).
- A job that followed a redirect returns the new base as `newEndpoint` (`transport.md` T7); the core keeps nothing of it.
- What the app does with this (U151, informative here): it stores the host key in the Keychain and the redirected endpoint with the device's settings; it uses the redirected endpoint as the base while one is stored, otherwise the user's self-hosted URL or the AnkiWeb default; changing the server or logging out clears the host key, the redirected endpoint and the media sync state (R1502). Anki's desktop also forgets the redirected endpoint when it clears the key after an authentication failure.

## L5. Login (R1498)

- Input: base, server kind, user name, password, stall timeout, proxy, cancel token.
- The user name loses white space at both ends; the password is sent exactly as given. An empty user name (after trimming) or an empty password is an `invalid_input` error, and nothing is sent. Anki's login dialog trims the user name the same way and does not send an empty field.
- The request is `hostKey` (`transport.md` T1) with `k` empty in the header and its own session key (T4). The body is the zstd compression of the compact JSON object `{"u":<user name>,"p":<password>}`, members in that order, the strings escaped as JSON requires.
- A 2xx answer whose decompressed body is a JSON object with a member `key` holding a non-empty string gives that string as the host key; other members are ignored. A body that is not such an object (no `key`, a key that is not a string, an empty string, invalid JSON) is a protocol error and gives no key.
- A 403 is an authentication failure (L6). Any other failure is the transport's (`transport.md` T11). A redirect on `hostKey` is never followed (T7).
- The call returns the host key and nothing else. The core writes nothing to disk: no file, no collection change, no log line holding the password or the key. The password is dropped when the call returns.

Example (the official server): request body `{"u":"<user name>","p":"<password>"}`, answer 200, `anki-original-size: 50`, body `{"key":"<40 lowercase hex digits>"}`.

## L6. Authentication failure (R1499)

- A 403 on any request, login or any later method, is reported as `status` 403 (`transport.md` T11), which U106 names the authentication failure. The app then clears the stored host key and asks the user to log in again (U151).
- The official server answers 403 to a wrong password and to a user name it does not know alike, and to a host key it does not know on any later request (`transport.md` T14). A failed login leaves no host key.

## L7. The host key

- The host key is opaque: Meguri never parses it, compares it only for equality, never logs it, and sends it only as `k` in the `anki-sync` header (`transport.md` T2), never in a URL or a body.
- The official server's key is 40 lowercase hex digits and is the same for every login of an account; AnkiWeb's keys look different. Neither fact may be relied on.

## L8. Scenarios

Actor scenarios under `conformance/scenarios/sync/transport/` (`specs/conformance/sync-ops.md`). In each, the oracle plays `meguri` when it records the golden; in T1's mixed run Meguri plays it with its own identity, so a pass shows that the official server accepts what Meguri sends. They stay `pending` until Meguri's executor plays actors and declares `sync` (sync-ops.md S10.2).

| Scenario | Shows | Golden |
|---|---|---|
| `login` | two logins succeed (L5) | recorded |
| `login-wrong-password` | a wrong password gives `auth_failed`, then the right one succeeds (L5, L6) | waits for the driver's `credentials` and `auth_failed` (sync-ops.md S4.4, S4.5) |
| `login-reset` | a login whose answer is reset gives `network`, then succeeds (`transport.md` T11) | recorded |
| `identity-accepted` | after another client uploads, Meguri's status check ends with `full_download` (`transport.md` T3) | recorded |

What the official server cannot show (redirects, missing or wrong `anki-original-size`, stalls, TLS failures, proxies, cancellation) is tested in `meguri-sync` against the scripted stub (`transport.md` T13), and login, the identity and `{}` bodies also against the U13 rig.
