# Security

## Reporting a vulnerability

This is a personal project maintained by one person. If you find a security problem, please report it privately through a [GitHub security advisory](https://github.com/georgevibing/earful/security/advisories/new) instead of opening a public issue. Include steps to reproduce and the version or commit you tested. You will get a reply as soon as I can manage, usually within a few days.

## Supported versions

Only the latest commit on `main` is supported.

## Security model

earful has two audiences with very different trust levels.

**Anonymous submitters.** The ingestion API (`/api/v1/*`, `/s/<slug>`, `/widget.js`) is intentionally unauthenticated, because players and users without accounts are its audience. Ingest keys ship inside clients and are public by design; they scope requests to a project and can be rotated, but they are not credentials. Abuse is handled by layered anti-spam: rate limits, adaptive proof of work, single-use signed challenges, a honeypot, a minimum fill time, duplicate detection and heuristics. Raw IP addresses are never stored, only keyed HMAC pseudonyms, and those are cleared from stored feedback after two days. Above the per project hourly flood guard, submissions are quarantined rather than refused (a hard cap at three times the guard and a total daily intake cap protect the disk), and quarantined items are purged after 30 days.

**The admin.** There is exactly one admin, authenticated with a password verified by argon2id. Sessions are random 256 bit tokens stored as SHA-256 hashes, in an `HttpOnly`, `SameSite=Strict`, `Secure`, `__Host-` prefixed cookie. Changing the admin password signs out every session, and `POST /api/admin/logout-all` does the same on demand. State changing requests with a foreign `Origin` are rejected and without a configured password hash every login fails. Failed logins are rate limited per IP and globally; browsers that have signed in before carry a signed device cookie that exempts them from the global cap, so a password spray from many IPs cannot lock the admin out.

**Agents.** The MCP endpoint (`/mcp`) requires an admin-issued bearer token with `read` or `write` scope. Tokens are shown once, stored as SHA-256 hashes, revocable instantly, and failed attempts are rate limited per IP. Requests with a foreign `Origin` are rejected. This is not a DNS rebinding defence on its own (a rebound page sends a matching `Host`), which is why the endpoint relies on bearer tokens that browsers never attach implicitly. JSON-RPC batches are limited to 20 messages.

**Network exposure.** The server binds `127.0.0.1` by default. The Docker image binds `0.0.0.0` inside the container, which is acceptable only because the container port is never published publicly: Coolify reaches it over its private Docker network and the compose file publishes to `127.0.0.1` only. Always put the app behind an HTTPS reverse proxy.

**Browser hardening.** The admin UI ships a `self`-only Content Security Policy, `X-Frame-Options: DENY`, `nosniff`, `Referrer-Policy: no-referrer` and HSTS. The widget renders inside Shadow DOM, never assigns user or server data to `innerHTML`, ignores inline-element endpoints on a different origin than `widget.js`, and refuses unreasonable proof of work difficulties. CSV exports quote every field and neutralise formula prefixes.

**Supply chain.** CI actions and Docker base images are pinned to digests and kept current by Dependabot; CI runs `cargo audit` and `pnpm audit`. The bundled embedding model is pinned by revision and SHA-256, both in the image and for `earful download-model`.

## Known limits

- A determined attacker with many IP addresses can still trickle in junk. It lands in the spam bucket or is easy to bulk archive, and per project caps bound the damage.
- Single admin only. There are no roles or multi-user accounts.
- The built-in `hash` embedding provider sends nothing anywhere. If you configure an external embedding API, feedback text is sent to that provider.
