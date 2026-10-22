# apps/admin

Admin SPA: Preact + TypeScript + Vite. Built to `dist/` and embedded into the server binary at `/admin/`.

## Commands

- `pnpm --filter @feedback-harvest/admin dev` (Vite dev server, proxies `/api` to `http://127.0.0.1:8080`; run the server too)
- `pnpm --filter @feedback-harvest/admin build`
- `pnpm --filter @feedback-harvest/admin typecheck`
- `pnpm --filter @feedback-harvest/admin test` (vitest)

## Gotchas

- Vite `base` is `/admin/`. Asset URLs must stay relative to it.
- The server serves `index.html` for unknown `/admin/*` paths (SPA fallback).
- CSP is `self` only: no CDNs, remote fonts, inline scripts or `eval`. Inline styles are allowed.
- Auth is the `fh_session` cookie (HttpOnly). The UI never sees the token; a `401` from any call means "show login".
- Never render server data with `innerHTML` / `dangerouslySetInnerHTML`. Feedback text is attacker controlled.
- All API types follow `docs/api.md`. Change the contract first, then `src/types.ts` and `src/api.ts`.
- Keep the bundle small: no UI frameworks or chart libraries; charts are hand-written SVG.
