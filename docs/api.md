# HTTP API

All bodies are JSON (`Content-Type: application/json`). Timestamps are Unix seconds.
Errors always look like `{"error": "<stable_code>", "message": "<human text>"}`.
`429` responses carry a `Retry-After` header (seconds).

## Public ingestion (`/api/v1`, no auth, CORS `*`, no credentials)

### `GET /api/v1/challenge?key=<ingest_key>`

Issues a single use proof of work challenge. `Cache-Control: no-store`.

```json
200 { "challenge": "v1.<pid>.<issued_ms>.<bits>.<id32hex>.<sig32hex>", "bits": 18, "expires_in": 1800 }
```

Errors: `404 not_found` (unknown key), `403 project_paused`, `429 rate_limited`.

Difficulty is adaptive: base `EARFUL_POW_BITS` (18) plus one bit per submission the
same IP sent to this project in the last hour (max +4), plus 2 when the project is
above half its hourly cap, clamped to `EARFUL_POW_MAX_BITS`.

**Solving:** find the smallest decimal string `nonce` (any works, the reference
solver counts up from 0) such that `sha256(utf8(challenge + ":" + nonce))` has at
least `bits` leading zero bits. Shared test vector (keep Rust and TS tests in sync):
`challenge = "v1.1.1700000000000.12.0123456789abcdef0123456789abcdef.deadbeef"`,
`bits = 12`, the first valid nonce counting from 0 is `"1627"` (asserted in
`crates/server/src/pow.rs::cross_language_vector`).

### `POST /api/v1/feedback`

Body limit 32 KB (larger bodies get `413`). The body is parsed as JSON regardless of
`Content-Type`, so `text/plain` posts (no CORS preflight) and `navigator.sendBeacon` work.

```json
{
  "key": "ear_...",               // or header X-Earful-Key
  "kind": "bug" | "feedback" | "crash" | "exception" | "log",
  "title": "optional, <= 200 chars (derived from body if empty)",
  "body": "required, 3..=10000 chars",
  "contact": "optional, <= 200 chars (email, discord, whatever)",
  "rating": 1,                     // optional 1..=5
  "meta": { "version": "1.2.3" }, // optional JSON object, <= 4096 bytes serialized
  "diagnostics": { "stack_trace": "...", "logs": "..." }, // optional object, <= 65536 bytes
  "source": "web" | "api",         // optional, default "api"
  "challenge": "<from /challenge>",
  "nonce": "12345",               // decimal string (a JSON number is accepted too)
  "website": ""                    // honeypot: must be empty or absent
}
```

Success is always `202 {"ok": true}`, whether the item was stored, merged into an
exact duplicate, quarantined or silently dropped. Spammers learn nothing.

Errors (legit clients need these): `400 invalid_json`, `400 invalid_<field>` validation
(`invalid_kind|invalid_body|invalid_title|invalid_contact|invalid_rating|invalid_meta|invalid_diagnostics|invalid_source`),
`400 challenge_malformed|challenge_invalid|challenge_wrong_project|challenge_expired|challenge_used|pow_insufficient`,
`404 not_found`, `403 project_paused`, `429 rate_limited`.

Pipeline order: project lookup, honeypot (silent drop), validation, rate limits
(IP+project per minute/hour/day, IP global per day, project per hour), PoW
verify + single use, exact duplicate (same IP: drop, other IP: `dup_count += 1`
on the original), heuristic spam score + too fast check (quarantine as
`status = spam` with `spam_reason`), insert, async embedding, near duplicate check
(runs once per item; reindexing never re-quarantines an item the admin restored, and
MCP imports are never checked). Blocked outcomes are stored as per day counters, so
rejected floods do not grow the database per request.

### `GET /s/<slug>`

Hosted HTML feedback page for the project (loads `/widget.js` in inline mode).
`404` for unknown slugs, a friendly "not accepting feedback" page when paused.

### `GET /widget.js`

Embeddable widget bundle (IIFE). See `packages/widget/README.md`.

### `GET /healthz`

`200 {"ok": true}`.

## Admin (`/api/admin`, session cookie `earful_session`, HttpOnly, SameSite=Strict)

Every non-GET admin request with an `Origin` header must match `Host`, else
`403 bad_origin`. Missing or expired session gives `401 unauthorized`.

### Auth

- `POST /api/admin/login` `{ "password": "..." }` gives `204` + cookie, `401 invalid_credentials`, `429 rate_limited` (5 failed attempts per 15 min per IP, 30 failed per hour globally; successful logins are not counted). Attempts are reserved before the password check, so concurrent guesses cannot exceed the limit.
- `POST /api/admin/logout` gives `204`, clears cookie.
- `GET /api/admin/me` gives `200 {"ok": true}` or `401`.

### Projects

Project object:

```json
{ "id": 1, "slug": "my-game", "title": "My Game", "project_type": "game",
  "description": "", "ingest_key": "ear_...", "accepting": true,
  "created_at": 0, "updated_at": 0,
  "stats": { "total": 0, "open": 0, "new": 0, "bugs": 0, "feedback": 0, "spam": 0, "last_at": null } }
```

`project_type` is one of `game, webapp, mobile, desktop, service, library, other`.
`stats` is present on list and get. `open` = `new` + `triaged`; `total` excludes spam.

- `GET /api/admin/projects` gives `200 [Project]` ordered by title.
- `POST /api/admin/projects` `{ title, slug?, project_type?, description?, accepting? }` gives `201 Project`. Slug is derived from title when omitted. `400 invalid_slug|invalid_title|invalid_project_type|invalid_description` (title max 120, description max 2000 chars), `409 conflict` on duplicate slug.
- `GET /api/admin/projects/:id` gives `200 Project`.
- `PATCH /api/admin/projects/:id` with any subset of the create fields gives `200 Project`.
- `DELETE /api/admin/projects/:id` gives `204` (cascades feedback and vectors).
- `POST /api/admin/projects/:id/rotate-key` gives `200 Project` with a new `ingest_key`.
- `GET /api/admin/projects/:id/export?format=json|csv` gives a download (`Content-Disposition: attachment; filename="<slug>-feedback.<ext>"`), all statuses, oldest first. CSV cells starting with `= + - @` are prefixed with `'` to prevent formula injection. `400 invalid_format`.

### Labels and themes

Labels belong to one project. Feedback can have several labels (at most 20).
Names are trimmed, 1..64 characters, unique within a project after Unicode
lowercasing. Descriptions (up to 500 characters) define when a theme applies.
Colors are `slate` (default), `blue`, `green`, `amber`, `red`, `violet`, or `pink`.
Labels are admin-only metadata: anonymous submissions cannot create or assign them.

Label object: `{id, project_id, name, color, description, created_at, updated_at}`.
List/create/update responses also contain `feedback_count` (excluding spam) and
`open_count` (new + triaged). Counts overlap when items have multiple labels.

- `GET /api/admin/labels?project_id=<optional>` lists labels ordered by open count,
  then name and id. Omit project_id for all projects. An unknown project gives 404.
- `POST /api/admin/labels` `{project_id, name, color?, description?}` gives `201 Label`.
  At most 200 labels per project. Duplicate names give `409 conflict`.
- `PATCH /api/admin/labels/:id` `{name?, color?, description?}` gives `200 Label`.
  Rename keeps the id and assignments. A label cannot move to another project.
- `DELETE /api/admin/labels/:id` gives `204`. Removes assignments, never feedback.
- `POST /api/admin/feedback/labels` `{ids, label_ids, action}` gives `{affected}`:
  `ids` contains 1..500 feedback ids; `label_ids` contains at most 20 label ids;
  `action` is `add`, `remove`, or `set`. Add/remove preserve unrelated labels and
  are safe to retry. Set replaces the complete set; `set` with `[]` clears it.
  All ids must exist and every label must belong to every target feedback's
  project. Validation and writes are atomic: one invalid id means no changes.
  `affected` counts feedback whose assignments changed (deduplicated ids).
  Empty label_ids are valid only for set. Over-limit or invalid inputs give
  `400 invalid_labels`; unknown ids give 404. Failed writes never partially apply.

Feedback objects, similar-item responses and JSON exports include `labels: [Label]`.
CSV exports append a `labels` column containing a JSON array of label names.
Assignment changes update the feedback's updated_at, but do not change its status or
embedding. Label definitions and assignments survive restarts and redeployments.

Feedback list/search accepts `label_ids=1,2`, `label_match=all|any` (default all),
and `unlabeled=true|false` (default false). These combine with every existing
filter and search mode before pagination/counting. Unlabeled excludes every
labeled item and cannot be combined with label_ids. Invalid combinations give
`400 invalid_query`. Unknown label ids match no items; with any matching, known selected ids can still match.

### Feedback

Feedback object:

```json
{ "id": 1, "project_id": 1, "kind": "bug", "title": "", "body": "", "contact": null,
  "rating": null, "meta": {}, "source": "api", "status": "new", "spam_reason": null,
  "spam_score": 0.0, "admin_note": "", "dup_count": 0,
  "created_at": 0, "updated_at": 0, "last_seen_at": 0, "score": 0.42 }
```

`score` only appears on search results. `status` is one of
`new, triaged, resolved, archived, spam`.

- `GET /api/admin/feedback` query params, all optional:
  - `project_id`, `kind` (`bug|feedback|crash|exception|log`)
  - `status`: `open` (default, new+triaged), `all` (everything except spam), `any` (everything), or a concrete status
  - `q` search text, `mode`: `hybrid` (default) | `fuzzy` | `keyword` | `semantic`
  - `from`, `to` (Unix seconds, inclusive), `limit` (default 50, max 200), `offset`
  - Without `q`: newest first. With `q`: relevance order.
  - Response: `{ "items": [Feedback], "total": 123, "mode": "hybrid", "semantic": true }` where `mode` is the mode actually used (`semantic` falls back to `hybrid` when vectors are unavailable) and `semantic` says whether vectors were used. Invalid params give `400 invalid_query`.
- `GET /api/admin/feedback/:id` gives `{ "item": Feedback, "similar": [Feedback with score] }` (top 5 nearest neighbours in the same project).
- `PATCH /api/admin/feedback/:id` `{ status?, admin_note?, kind? }` gives `200 Feedback`, `400 invalid_status|invalid_kind|invalid_admin_note`. Moving an item out of `spam` clears `spam_reason`.
- `DELETE /api/admin/feedback/:id` gives `204`.
- `POST /api/admin/feedback/bulk` `{ "ids": [1,2], "action": "set_status", "status": "archived" }` or `{ "ids": [...], "action": "delete" }` gives `200 {"affected": 2}`. Max 500 ids. `400 invalid_ids|invalid_action|invalid_status`.

### Stats

`GET /api/admin/stats?project_id=<optional>`:

```json
{
  "totals": { "total": 0, "open": 0, "new": 0, "bugs": 0, "feedback": 0, "spam": 0, "last_7d": 0 },
  "daily": [ { "day": "2026-10-01", "bug": 0, "feedback": 0 } ],   // last 30 days, oldest first, zero filled
  "blocked": { "rate_limited": 0, "honeypot": 0, "pow_insufficient": 0, "duplicate": 0, "spam_score": 0, "too_fast": 0, "near_duplicate": 0 },  // last 30 UTC days; these keys are always present, other seen reason codes (e.g. challenge_used, challenge_expired, challenge_invalid) are added
  "embeddings": { "enabled": true, "model": "hash-v1-384", "embedded": 0, "pending": 0 }
}
```

### Maintenance

- `POST /api/admin/reindex` drops all vectors and re-embeds everything in the background. `202 {"ok": true}`.

### API tokens (for MCP and scripts)

Admin-issued bearer tokens. Only `sha256(token)` is stored; the raw token is shown once.
`scope` is `read` (list/get/search/stats/export) or `write` (everything, including deletes).

Token object: `{ "id": 1, "name": "claude-code laptop", "scope": "write", "prefix": "eat_ab12", "created_at": 0, "last_used_at": null }`

- `GET /api/admin/tokens` gives `200 [Token]` newest first.
- `POST /api/admin/tokens` `{ "name": "...", "scope": "read" | "write" }` gives `201 { "token": "eat_<64 hex>", ...Token }`. `400 invalid_name|invalid_scope`.
- `DELETE /api/admin/tokens/:id` gives `204` (revokes immediately).

## MCP (`/mcp`)

Model Context Protocol server, Streamable HTTP transport (JSON responses, no SSE,
stateless). Full guide and tool reference: [`docs/mcp.md`](mcp.md).

- `POST /mcp` takes one JSON-RPC 2.0 message or a batch. Body limit 1 MB.
  Requests get `200` with the JSON-RPC response (an array for batches); a body of only
  notifications gets `202` with no body.
- `Authorization: Bearer eat_...` is required. Missing, unknown or revoked tokens get
  `401` with `WWW-Authenticate: Bearer realm="earful"`. After 20 failed
  attempts from one IP in 15 minutes every request from that IP gets `429` until the
  window passes.
- A present `Origin` header must match `Host` (DNS rebinding defence), else `403 bad_origin`.
- `GET /mcp` and `DELETE /mcp` give `405` (no SSE stream, no session teardown needed).
- `initialize` echoes the client's `protocolVersion` when it is `2025-11-25`,
  `2025-06-18` or `2025-03-26`, otherwise answers `2025-06-18`, and returns an
  `Mcp-Session-Id` header. The id is informational; later requests do not need it.
- Methods: `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`,
  `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`, and the
  `notifications/*` family (accepted and ignored).
- JSON-RPC errors: `-32700` parse error (HTTP 400), `-32600` invalid request,
  `-32601` unknown method, `-32602` invalid params or unknown tool/prompt,
  `-32002` unknown resource, `-32603` internal.
- Scopes: read tokens only see read tools in `tools/list`. Calling a write tool with a
  read token returns a tool result with `isError: true` whose text starts with
  `forbidden:`; nothing changes. Tool argument and validation failures are also
  `isError: true` results (text starts with the admin API error code, e.g.
  `invalid_body: ...`, `not_found: ...`, `confirmation_required: ...`) so agents can
  correct themselves.
- Tools reuse the admin operations, so validation, side effects and the vector index
  stay identical to the admin API. `create_feedback` is the one MCP-only operation:
  an admin-side insert with `source = "mcp"` that skips PoW, rate limits and spam scoring.


## Diagnostic reports

The existing POST /api/v1/feedback and SDK submit accept kind crash, exception and
log alongside bug and feedback. MCP create_feedback accepts the same kinds and
diagnostics. An optional diagnostics JSON object holds at most 65536 serialized
UTF-8 bytes; omitted/null becomes {}. Other types or larger objects return
invalid_diagnostics. The public request limit is 128 KiB; existing body (3-10000
characters), meta (4096 bytes), PoW and rate limits still apply.

Diagnostics round-trip through list/detail, MCP and JSON/CSV exports. Filter by
kind with the existing status/project/label filters. Stats include kind_counts
excluding spam. The daily chart still shows bugs and feedback only. Search indexes
title/body, so include the exception signature there. See docs/crash-reporting.md
in the repository for integration and agent workflows.
