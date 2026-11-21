# Security

## Reporting a vulnerability

This is a personal project maintained by one person. If you find a security problem, please report it privately through a [GitHub security advisory](https://github.com/georgevibing/earful/security/advisories/new) instead of opening a public issue. Include steps to reproduce and the version or commit you tested. You will get a reply as soon as I can manage, usually within a few days.

## Supported versions

Only the latest commit on `main` is supported.

## Security model

earful has two audiences with very different trust levels.

**Anonymous submitters.** The ingestion API (`/api/v1/*`, `/s/<slug>`, `/widget.js`) is intentionally unauthenticated, because players and users without accounts are its audience. Ingest keys ship inside clients and are public by design; they scope requests to a project and can be rotated, but they are not credentials. Abuse is handled by layered anti-spam: rate limits, adaptive proof of work, single-use signed challenges, a honeypot, a minimum fill time, duplicate detection and heuristics. Raw IP addresses are never stored, only keyed HMAC pseudonyms.

**The admin.** There is exactly one admin, authenticated with a password verified by argon2id. Sessions are random 256 bit tokens stored as SHA-256 hashes, in an `HttpOnly`, `SameSite=Strict`, `Secure` cookie. State changing requests with a foreign `Origin` are rejected, failed logins are rate limited, and without a configured password hash every login fails.

**Agents.** The MCP endpoint (`/mcp`) requires an admin-issued bearer token with `read` or `write` scope. Tokens are shown once, stored as SHA-256 hashes, revocable instantly, and failed attempts are rate limited per IP. Requests with a foreign `Origin` are rejected to prevent DNS rebinding.

**Network exposure.** The server binds `127.0.0.1` by default. The Docker image binds `0.0.0.0` inside the container, which is acceptable only because the container port is never published publicly: Coolify reaches it over its private Docker network and the compose file publishes to `127.0.0.1` only. Always put the app behind an HTTPS reverse proxy.

**Browser hardening.** The admin UI ships a `self`-only Content Security Policy, `X-Frame-Options: DENY`, `nosniff` and `Referrer-Policy: no-referrer`. The widget renders inside Shadow DOM and never assigns user or server data to `innerHTML`.

## Known limits

- A determined attacker with many IP addresses can still trickle in junk. It lands in the spam bucket or is easy to bulk archive, and per project caps bound the damage.
- Single admin only. There are no roles or multi-user accounts.
- The built-in `hash` embedding provider sends nothing anywhere. If you configure an external embedding API, feedback text is sent to that provider.
