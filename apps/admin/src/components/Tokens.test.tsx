import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mcpSnippets } from '../snippets';
import { mockFetch } from '../test-utils';
import type { ApiToken } from '../types';
import { Tokens } from './Tokens';

const RAW = `eat_${'ab'.repeat(32)}`;

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('mcpSnippets', () => {
  it('uses a placeholder until a token exists', () => {
    const [cli, json] = mcpSnippets('https://fb.example.com');
    expect(cli!.code).toBe(
      'claude mcp add --transport http earful https://fb.example.com/mcp --header "Authorization: Bearer <TOKEN>"',
    );
    expect(JSON.parse(json!.code)).toEqual({
      mcpServers: {
        'earful': { type: 'http', url: 'https://fb.example.com/mcp', headers: { Authorization: 'Bearer <TOKEN>' } },
      },
    });
    expect(mcpSnippets('https://x', 'eat_1')[0]!.code).toContain('Bearer eat_1');
  });
});

describe('Tokens screen', () => {
  it('lists, creates (showing the raw token once) and revokes', async () => {
    let tokens: ApiToken[] = [
      { id: 1, name: 'old script', scope: 'read', prefix: 'eat_1234', created_at: 1_700_000_000, last_used_at: null },
    ];
    const calls = mockFetch({
      'GET /api/admin/tokens': () => ({ body: tokens }),
      'POST /api/admin/tokens': (c) => {
        const b = c.body as { name: string; scope: 'read' | 'write' };
        const t: ApiToken = { id: 2, name: b.name, scope: b.scope, prefix: 'eat_abab', created_at: 1_700_000_100, last_used_at: null };
        tokens = [t, ...tokens];
        return { status: 201, body: { ...t, token: RAW } };
      },
      'DELETE /api/admin/tokens/1': () => {
        tokens = tokens.filter((t) => t.id !== 1);
        return { status: 204 };
      },
    });
    vi.stubGlobal('confirm', vi.fn(() => true));

    render(<Tokens />);
    expect(await screen.findByText('old script')).toBeTruthy();
    expect(screen.getByText('eat_1234...')).toBeTruthy();
    expect(screen.getAllByText(/Bearer <TOKEN>/).length).toBeGreaterThan(0);

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'claude laptop' } });
    fireEvent.change(screen.getByLabelText('Scope'), { target: { value: 'write' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create token' }));

    const box = await screen.findByTestId('new-token');
    expect(box.textContent).toContain(RAW);
    expect(box.textContent).toContain('you will not see this again');
    expect(calls.find((c) => c.method === 'POST')!.body).toEqual({ name: 'claude laptop', scope: 'write' });
    expect(screen.getAllByText(new RegExp(`Bearer ${RAW}`)).length).toBe(2);
    expect(await screen.findByText('claude laptop')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByTestId('new-token')).toBeNull();
    expect(document.body.textContent).not.toContain(RAW);

    fireEvent.click(screen.getByRole('button', { name: 'Revoke old script' }));
    await waitFor(() => expect(screen.queryByText('old script')).toBeNull());
    expect(calls.some((c) => c.method === 'DELETE' && c.url === '/api/admin/tokens/1')).toBe(true);
  });

  it('requires a name', async () => {
    const calls = mockFetch({ 'GET /api/admin/tokens': () => ({ body: [] }) });
    render(<Tokens />);
    await screen.findByText('No tokens yet.');
    fireEvent.click(screen.getByRole('button', { name: 'Create token' }));
    expect(screen.getByRole('alert').textContent).toContain('Give the token a name');
    expect(calls.some((c) => c.method === 'POST')).toBe(false);
  });
});
