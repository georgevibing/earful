# MCP server

feedback-harvest ships a [Model Context Protocol](https://modelcontextprotocol.io) server
inside the same binary, at `POST /mcp`. Point Claude Code, Claude Desktop, Cursor, VS Code
or any MCP client at it and your agent can manage projects, triage the inbox, search
feedback by meaning, group reports into reusable themes, import reports from other channels and draft release notes.

- Transport: Streamable HTTP with plain JSON responses (no SSE), stateless.
- Auth: admin-issued bearer tokens, `read` or `write` scoped, revocable, stored hashed.
- 23 tools, 3 resources, 4 prompts. Tools call the exact same code as the admin API.

## Quickstart

### 1. Create a token

In the admin UI open **API tokens**, pick a name (for example `claude-code laptop`) and a
scope, and copy the `fht_...` token. It is shown once.

| Scope | Can do |
| --- | --- |
| `read` | list, get, search, similar, stats, export, integration snippets |
| `write` | everything above plus create, update, rotate keys, bulk triage, delete, reindex |

Prefer `read` for agents that only analyse. Use `write` when you want the agent to triage.

Or with curl (the session cookie needs `FH_COOKIE_SECURE=false` on plain http localhost):

```bash
HOST=https://feedback.example.com
curl -s -c /tmp/fh.cookies -X POST "$HOST/api/admin/login" \
  -H 'content-type: application/json' -d '{"password":"<admin password>"}'
curl -s -b /tmp/fh.cookies -X POST "$HOST/api/admin/tokens" \
  -H 'content-type: application/json' -d '{"name":"claude-code","scope":"write"}'
# => {"token":"fht_...","id":1,"name":"claude-code","scope":"write","prefix":"fht_1a2b",...}
```

### 2. Connect your client

Replace `https://feedback.example.com` with your deployment and `fht_...` with the token.

**Claude Code**

```bash
claude mcp add --transport http feedback-harvest https://feedback.example.com/mcp \
  --header "Authorization: Bearer fht_..."
```

Add `--scope user` to make it available in every project. To share the config in a repo
without committing the token, use `.mcp.json` with an environment variable:

```json
{
  "mcpServers": {
    "feedback-harvest": {
      "type": "http",
      "url": "https://feedback.example.com/mcp",
      "headers": { "Authorization": "Bearer ${FH_MCP_TOKEN}" }
    }
  }
}
```

**Cursor** (`~/.cursor/mcp.json` or `.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "feedback-harvest": {
      "url": "https://feedback.example.com/mcp",
      "headers": { "Authorization": "Bearer fht_..." }
    }
  }
}
```

**VS Code** (`.vscode/mcp.json`, prompts for the token once and stores it securely)

```json
{
  "inputs": [
    { "type": "promptString", "id": "fh-token", "description": "feedback-harvest API token", "password": true }
  ],
  "servers": {
    "feedback-harvest": {
      "type": "http",
      "url": "https://feedback.example.com/mcp",
      "headers": { "Authorization": "Bearer ${input:fh-token}" }
    }
  }
}
```

**Claude Desktop and other stdio-only clients**

Clients that cannot send custom headers to a remote server can use the built-in stdio
bridge. It reads JSON-RPC from stdin, forwards it over HTTPS with your token and writes
replies to stdout.

```json
{
  "mcpServers": {
    "feedback-harvest": {
      "command": "/usr/local/bin/feedback-harvest",
      "args": ["mcp-stdio"],
      "env": {
        "FH_MCP_URL": "https://feedback.example.com/mcp",
        "FH_MCP_TOKEN": "fht_..."
      }
    }
  }
}
```

No local binary? Run the bridge from the Docker image instead:
`"command": "docker", "args": ["run", "--rm", "-i", "-e", "FH_MCP_URL", "-e", "FH_MCP_TOKEN", "<your image>", "mcp-stdio"]`.
`FH_MCP_TOKEN_FILE` works too. The bridge refuses plain `http://` except for localhost.

**Anything else**

Any client that speaks Streamable HTTP works with: URL `https://<host>/mcp`, header
`Authorization: Bearer fht_...`. A raw call looks like:

```bash
curl -s https://feedback.example.com/mcp \
  -H "Authorization: Bearer fht_..." -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}'
```

### 3. Try it

Ask your agent things like:

- "What are the top bugs in starfall this week?"
- "Triage the new feedback for my website." (or run the `triage_inbox` prompt)
- "Did anyone already report the inventory crash? If not, file this Discord message as a bug."
- "Write release notes from everything resolved since October 1st."
- "Create a project for my new iOS app and give me the widget snippet."
- "Group the open reports in starfall into themes, reuse existing labels, and show what needs work."

## Tools

Projects can be referenced by id or slug everywhere. Dates accept ISO 8601
(`2026-10-01`, `2026-10-01T12:00:00Z`) or Unix seconds.

| Tool | Scope | What it does |
| --- | --- | --- |
| `list_projects` | read | All projects with counters (open, new, bugs, feedback, spam, last activity). |
| `get_project` | read | One project by id or slug, including its ingest key. |
| `list_feedback` | read | Browse newest first. Filters: project, kind, status (default `open`), from, to, days, label_ids, label_match (all/any), unlabeled, limit, offset. Optional `q` switches to relevance order. |
| `search_feedback` | read | Relevance search with `mode` `hybrid` (default), `keyword`, `fuzzy` or `semantic`. Same filters, default status `all`. |
| `get_feedback` | read | One item in full plus up to 5 similar items (duplicate spotting). |
| `find_similar_feedback` | read | Items similar to an id or to free text. Falls back to lexical search when embeddings are off. |
| `get_stats` | read | Totals, 30 day bug vs feedback series, anti-spam block counts, embedding status. |
| `export_feedback` | read | All items of a project, oldest first, as JSON or CSV, paginated (max 1000 per call). |
| `get_integration_snippets` | read | Hosted page URL, widget tags, ESM client and curl examples for a project. |
| `list_labels` | read | Label definitions and non-spam/open report counts, optionally scoped by project. |
| `create_label` | write | Project label with name, color and optional classification guidance in description. |
| `update_label` | write | Rename or edit color/description by id; assignments follow the stable id. |
| `delete_label` | write | Delete a definition and its assignments, keeping feedback. Needs `confirm: true`. |
| `label_feedback` | write | Atomic add/remove/set label assignments on up to 500 feedback ids. Add/remove preserve unrelated labels. |
| `create_project` | write | New project (title, slug, type, description, accepting). |
| `update_project` | write | Change any project field, including pausing with `accepting=false`. |
| `rotate_project_key` | write | New ingest key; the old one stops working at once. |
| `delete_project` | write | Deletes a project and all its feedback. Needs `confirm_slug`. |
| `create_feedback` | write | Admin-side insert for imports (Discord, email, store reviews). Skips anti-spam, `source` is `mcp`, optional backdated `created_at`. |
| `update_feedback` | write | Status, kind, `admin_note` (replace) or `append_note` (dated line). |
| `bulk_update_feedback` | write | Same status on up to 500 ids. |
| `delete_feedback` | write | Permanent delete. Needs `confirm: true`; archiving is usually better. |
| `reindex_embeddings` | write | Re-embed everything after changing the embedding provider or model. |

Every tool declares an `inputSchema`, an `outputSchema` and annotations
(`readOnlyHint`, `destructiveHint`, `idempotentHint`), returns `structuredContent` and a
compact text rendering for clients that only show text.

**Statuses:** `new` (untriaged), `triaged` (accepted, actionable), `resolved` (fixed or
done), `archived` (no action: thanks, noise, duplicates, won't fix), `spam`
(quarantined). Filters also accept `open` (new + triaged), `all` (everything but spam)
and `any` (everything).

**Search modes:** `hybrid` fuses keyword, fuzzy and semantic results with reciprocal rank
fusion and is the right default. `keyword` matches stemmed words (good for error strings),
`fuzzy` tolerates typos and partial words, `semantic` matches meaning ("game freezes"
finds "stuck on the loading screen"). The response field `semantic` tells you whether
vectors were used.

**Errors:** problems the agent can fix come back as tool results with `isError: true`
and a message that starts with a stable code, for example `invalid_body: body must be
between 3 and 10000 characters`, `not_found: no feedback with id 42`,
`confirmation_required: confirm_slug must be exactly "starfall"` or
`forbidden: ... needs a write scoped token`.

## Resources

| URI | Content |
| --- | --- |
| `feedback-harvest://projects` | All projects with counters. |
| `feedback-harvest://projects/{slug}` | One project, its label definitions with counts, and its 25 newest open items. |
| `feedback-harvest://feedback/{id}` | One item in full plus similar items. |

All are `application/json`. Concrete project URIs appear in `resources/list`.

## Prompts

| Prompt | Arguments | Workflow |
| --- | --- | --- |
| `group_feedback_themes` | `project` (required) | Review and reuse a project taxonomy, page reports, propose themes, apply labels when authorized, and verify counts and evidence. |
| `triage_inbox` | `project` (optional), `limit` (default 20) | Read every new item, spot duplicates with similar items, quarantine junk, triage actionable bugs with severity notes, archive the rest, report the top 3 issues. |
| `summarize_feedback` | `project`, `days` (default 30) | Volume and trend, themes with counts and example ids, severe bugs, sentiment, recommended actions. |
| `release_notes_from_resolved` | `project`, `since` (ISO date or days, default 14) | Friendly Fixes and Improvements notes from resolved items, with the source ids listed for review. |

## Labels and thematic analysis

Use the [agent theme guide](agent-themes.md) for classification workflows, MCP examples,
pagination rules, and taxonomy maintenance. List/search results and feedback resources
include labels. Label filters combine with existing status, date, project and search
filters before pagination. `label_ids` is an integer array; `label_match` defaults to
`all`, or use `any`. `unlabeled: true` finds reports without assignments and cannot be
combined with label IDs. `summarize_feedback` analyzes themes without changing them.

## Example workflows

**Import a bug from Discord without duplicating it**

1. `find_similar_feedback` with `text` = the message and `project` = `starfall`.
2. If a close match exists: `update_feedback` on it with `append_note: "also reported on Discord by @neo"`.
3. Otherwise: `create_feedback` with `kind: "bug"`, the message as `body`, `meta: {"origin": "discord"}`.

**Close the loop after a release**

1. `search_feedback` with `q: "inventory crash"`, `status: "open"`.
2. `bulk_update_feedback` with the matching ids and `status: "resolved"`.
3. Run the `release_notes_from_resolved` prompt.

## Security notes

- Tokens are random 256 bit secrets (`fht_` + 64 hex). Only their SHA-256 is stored and
  the list endpoint never returns them. Revoke a token in **API tokens** and it stops
  working on the next request.
- Read tokens cannot see or call write tools. Give agents the smallest scope that works.
- Destructive tools need explicit confirmation arguments, and annotations mark them as
  destructive so clients can ask before running them.
- 20 failed bearer attempts from one IP in 15 minutes lock that IP out for the rest of
  the window.
- Requests with an `Origin` header that does not match `Host` are rejected, which stops
  browser based DNS rebinding attacks against a local server.
- Use HTTPS in production (Coolify terminates TLS for you). Tokens sent over plain HTTP
  can be sniffed; the stdio bridge refuses non-localhost `http://` URLs for that reason.
- `/mcp` shares the listener with the admin API and is reachable wherever the server is,
  guarded by the token. The bind and proxy rules in `docs/deploy-coolify.md` apply unchanged.


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
