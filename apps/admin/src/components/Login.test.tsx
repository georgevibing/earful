import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../app';
import { mockFetch } from '../test-utils';

const emptyStats = {
  totals: { total: 0, open: 0, new: 0, bugs: 0, feedback: 0, spam: 0, last_7d: 0 },
  daily: [],
  blocked: {},
  embeddings: { enabled: true, model: 'hash-v1-384', embedded: 0, pending: 0 },
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  location.hash = '';
});

describe('login flow', () => {
  it('shows the login form, rejects a wrong password, then signs in', async () => {
    let authed = false;
    const calls = mockFetch({
      'GET /api/admin/me': () => (authed ? { body: { ok: true } } : { status: 401, body: { error: 'unauthorized' } }),
      'POST /api/admin/login': (c) => {
        if ((c.body as { password: string }).password === 'right') {
          authed = true;
          return { status: 204 };
        }
        return { status: 401, body: { error: 'invalid_credentials', message: 'wrong password' } };
      },
      'GET /api/admin/projects': () => ({ body: [] }),
      'GET /api/admin/stats': () => ({ body: emptyStats }),
    });

    render(<App />);
    const input = await screen.findByLabelText('Admin password');

    fireEvent.input(input, { target: { value: 'wrong' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'Wrong password.');

    fireEvent.input(input, { target: { value: 'right' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    await screen.findByRole('heading', { name: 'Dashboard' });
    expect(calls.filter((c) => c.url === '/api/admin/login')).toHaveLength(2);
    expect(calls.find((c) => c.url === '/api/admin/login')?.body).toEqual({ password: 'wrong' });
  });

  it('explains rate limiting', async () => {
    mockFetch({
      'GET /api/admin/me': () => ({ status: 401, body: { error: 'unauthorized' } }),
      'POST /api/admin/login': () => ({ status: 429, body: { error: 'rate_limited' }, headers: { 'Retry-After': '600' } }),
    });
    render(<App />);
    fireEvent.input(await screen.findByLabelText('Admin password'), { target: { value: 'x' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Too many attempts. Try again in 10 min.'));
  });

  it('returns to the login screen when a session expires', async () => {
    let expired = false;
    mockFetch({
      'GET /api/admin/me': () => ({ body: { ok: true } }),
      'GET /api/admin/projects': () => ({ body: [] }),
      'GET /api/admin/stats': () =>
        expired ? { status: 401, body: { error: 'unauthorized' } } : { body: emptyStats },
    });
    expired = true;
    render(<App />);
    await screen.findByLabelText('Admin password');
  });
});
