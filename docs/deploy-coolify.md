# Deploying on Coolify

Coolify builds the repository's `Dockerfile` and puts its own reverse proxy (Traefik by default, or Caddy) in front with HTTPS. The container binds `0.0.0.0:8080` inside its private Docker network; the proxy is the only thing that talks to it. No host port is ever published.

## 1. Prepare the admin password hash

On any machine with the repo:

```sh
printf '%s' 'choose-a-long-password' | cargo run -q -p feedback-harvest -- hash-password
# or, with a locally built image:
printf '%s' 'choose-a-long-password' | docker run --rm -i feedback-harvest:local hash-password
```

Keep the output (`$argon2id$v=19$...`) for step 4.

## 2. Create the application

1. In Coolify: **Projects**, pick or create a project and environment, **+ New**, then **Public Repository** or **Private Repository** (GitHub App or deploy key).
2. Select the repository and branch `main`.
3. **Build Pack**: `Dockerfile`. Base directory `/`, Dockerfile location `/Dockerfile`.

## 3. Network settings

- **Ports Exposes**: `8080`. This tells the proxy where to route; it does not publish anything on the host.
- **Ports Mappings**: leave **empty**. A mapping like `8080:8080` would publish the app on every host interface and bypass HTTPS. Do not add one.
- **Domains**: e.g. `https://feedback.example.com`. Coolify's proxy obtains a Let's Encrypt certificate and redirects http to https.

## 4. Environment variables

| Variable | Value | Notes |
| --- | --- | --- |
| `FH_ADMIN_PASSWORD_HASH` | the hash from step 1 | Mark it **literal** (Coolify's "Is Literal?" option) so the `$` characters are not interpolated, and make sure it is not a build variable. Without it, every admin login is rejected. |
| `FH_TRUSTED_PROXY_HOPS` | `1` | Already the image default. Use `2` only if a Cloudflare proxy (orange cloud) sits in front of Coolify, see below. |
| `FH_EMBED_PROVIDER` and friends | optional | See "Semantic search with Ollama" below. |
| `RUST_LOG` | `info` | Optional. |

Leave `FH_BIND`, `FH_DATABASE_PATH` and `FH_COOKIE_SECURE` at their image defaults.

### Proxy hops and client IPs

Rate limits work per client IP, so the app must see the real one:

- **Coolify proxy only**: the proxy appends the client address to `X-Forwarded-For`; `FH_TRUSTED_PROXY_HOPS=1` takes the rightmost entry. Values the client sends itself are ignored.
- **Cloudflare proxy + Coolify proxy**: set `FH_TRUSTED_PROXY_HOPS=2`, and configure the Coolify proxy to trust Cloudflare's IP ranges for forwarded headers (Traefik: `entryPoints.https.forwardedHeaders.trustedIPs` with the ranges from https://www.cloudflare.com/ips/). Without that, Traefik discards Cloudflare's `X-Forwarded-For`, the chain is too short, and every visitor collapses onto one address.

If the hop count is too low, all users share the proxy's address and hit the rate limits together. If it is too high, clients can spoof their IP. When in doubt, submit from two different networks and confirm they are rate limited independently.

## 5. Persistent storage

**Storages**, **+ Add**, **Volume Mount**: destination path `/data`. The SQLite database lives at `/data/feedback-harvest.db`.

A named volume is initialised from the image with the right ownership (uid 65532, the distroless `nonroot` user). If you use a **bind mount** instead, `chown 65532:65532` the host directory first or the app cannot write.

## 6. Health check

The image declares a Docker `HEALTHCHECK` that runs `/feedback-harvest healthcheck` (an HTTP probe against `/healthz` inside the container). The runtime image is distroless and ships no shell, `curl` or `wget`, so leave Coolify's own health check option disabled (it runs `curl`/`wget` inside the container) and rely on the image's check.

## 7. Deploy and verify

1. **Deploy**. The first build takes a few minutes (Rust release build); later builds reuse cached dependency layers.
2. `https://feedback.example.com/healthz` returns `{"ok":true}`.
3. `https://feedback.example.com/api/admin/me` returns `401` when not logged in.
4. Log in at `https://feedback.example.com/admin/`, create a project, open its hosted page `/s/<slug>` and send a test submission.
5. From the server, confirm nothing listens publicly on 8080: `ss -ltnp | grep 8080` should show nothing bound to the host (only the proxy on 80/443).

## Semantic search with Ollama (optional)

1. In the same Coolify project and environment, **+ New**, **Docker Image** `ollama/ollama:latest` (or the Ollama one-click service). Add a volume mount at `/root/.ollama`. No domain, no port mappings.
2. Make sure both resources share a network (same environment, or enable "Connect to Predefined Network"), and note the Ollama container or service name.
3. Pull a model once from the Ollama container terminal: `ollama pull nomic-embed-text`.
4. On feedback-harvest set `FH_EMBED_PROVIDER=openai`, `FH_EMBED_URL=http://<ollama-name>:11434/v1`, `FH_EMBED_MODEL=nomic-embed-text` and redeploy. Existing items are re-embedded in the background; progress shows on the dashboard.

Using OpenAI instead: `FH_EMBED_PROVIDER=openai`, `FH_EMBED_MODEL=text-embedding-3-small`, `FH_EMBED_API_KEY=<key>` (mark it as a secret, not a build variable).

## Backups

The database runs in WAL mode, so copy it with SQLite's backup API rather than `cp`:

```sh
# On the Coolify host. Find the volume path with: docker volume inspect <volume-name>
sqlite3 /var/lib/docker/volumes/<volume-name>/_data/feedback-harvest.db \
  ".backup '/root/backups/feedback-harvest-$(date +%F).db'"
```

Schedule it with cron, or run [Litestream](https://litestream.io) as a separate container mounting the same volume to replicate continuously to S3 compatible storage. Coolify's built-in database backups do not apply here because this is an application volume, not a managed database.

Restore: stop the app, replace `feedback-harvest.db` in the volume (delete any stale `-wal` and `-shm` files), start the app.

## Upgrades

Push to `main` and redeploy (or enable automatic deployments on push). Migrations run on boot and are append only, so upgrades keep existing data. Take a backup before upgrading across many versions.
