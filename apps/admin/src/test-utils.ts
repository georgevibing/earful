import { vi } from 'vitest';

export interface Call {
  method: string;
  url: string;
  body: unknown;
}

type Handler = (call: Call) => { status?: number; body?: unknown; headers?: Record<string, string> };

/** Installs a fetch mock routed by `METHOD path` prefix. Returns the recorded calls. */
export function mockFetch(routes: Record<string, Handler>): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET';
      const call: Call = { method, url: input, body: init?.body ? JSON.parse(String(init.body)) : undefined };
      calls.push(call);
      const path = input.split('?')[0];
      const key = Object.keys(routes).find((k) => k === `${method} ${path}`);
      if (!key) return new Response(JSON.stringify({ error: 'not_found', message: 'not found' }), { status: 404 });
      const r = routes[key]!(call);
      const status = r.status ?? 200;
      return new Response(status === 204 ? null : JSON.stringify(r.body ?? {}), {
        status,
        headers: { 'Content-Type': 'application/json', ...(r.headers ?? {}) },
      });
    }),
  );
  return calls;
}
