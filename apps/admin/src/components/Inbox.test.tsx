import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mockFetch } from '../test-utils';
import type { Feedback } from '../types';
import { Inbox } from './Inbox';

function item(id: number, over: Partial<Feedback> = {}): Feedback {
  return {
    id,
    project_id: 1,
    kind: 'bug',
    title: `Crash number ${id}`,
    body: 'The game crashes when opening the inventory.',
    contact: null,
    rating: null,
    meta: {},
    source: 'api',
    status: 'new',
    spam_reason: null,
    spam_score: 0,
    admin_note: '',
    dup_count: 0,
    created_at: Math.floor(Date.now() / 1000) - 120,
    updated_at: 0,
    last_seen_at: 0,
    ...over,
  };
}

const project = { id: 1, slug: 'game', title: 'My Game', project_type: 'game', description: '', ingest_key: 'fhk_x', accepting: true, created_at: 0, updated_at: 0 };

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  location.hash = '';
});

describe('Inbox', () => {
  it('renders items with highlights, badges and the semantic notice', async () => {
    const calls = mockFetch({
      'GET /api/admin/projects': () => ({ body: [project] }),
      'GET /api/admin/feedback': () => ({
        body: { items: [item(1, { dup_count: 3, rating: 4 }), item(2, { kind: 'feedback', title: 'Love it', body: 'great' })], total: 2, mode: 'hybrid', semantic: false },
      }),
    });
    render(<Inbox params={new URLSearchParams('q=crash')} />);

    const rows = await screen.findAllByTestId('feedback-row');
    expect(rows).toHaveLength(2);
    const marks = rows[0]!.querySelectorAll('mark');
    expect([...marks].map((m) => m.textContent?.toLowerCase())).toContain('crash');
    expect(rows[0]!.textContent).toContain('+3');
    expect(rows[0]!.textContent).toContain('★★★★☆');
    expect(await screen.findAllByText('My Game', { selector: '.meta span' })).toHaveLength(2);
    expect(screen.getByText(/Semantic search is unavailable/)).toBeTruthy();

    const listCall = calls.find((c) => c.url.startsWith('/api/admin/feedback'))!;
    const qs = new URLSearchParams(listCall.url.split('?')[1]);
    expect(qs.get('q')).toBe('crash');
    expect(qs.get('mode')).toBe('hybrid');
    expect(qs.get('status')).toBe('open');
    expect(qs.get('limit')).toBe('50');
  });

  it('runs bulk actions on the selection and refetches', async () => {
    const calls = mockFetch({
      'GET /api/admin/projects': () => ({ body: [project] }),
      'GET /api/admin/feedback': () => ({ body: { items: [item(1), item(2), item(3)], total: 3, mode: 'hybrid', semantic: true } }),
      'POST /api/admin/feedback/bulk': (c) => ({ body: { affected: (c.body as { ids: number[] }).ids.length } }),
    });
    render(<Inbox params={new URLSearchParams()} />);
    await screen.findAllByTestId('feedback-row');

    fireEvent.click(screen.getByLabelText('Select Crash number 1'));
    fireEvent.click(screen.getByLabelText('Select Crash number 3'));
    expect(screen.getByText('2 selected')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Archive' }));

    await waitFor(() => expect(calls.some((c) => c.url === '/api/admin/feedback/bulk')).toBe(true));
    const bulk = calls.find((c) => c.url === '/api/admin/feedback/bulk')!;
    expect(bulk.method).toBe('POST');
    expect(bulk.body).toEqual({ ids: [1, 3], action: 'set_status', status: 'archived' });
    await waitFor(() => expect(calls.filter((c) => c.url.startsWith('/api/admin/feedback?')).length).toBe(2));
  });

  it('deletes only after confirmation', async () => {
    const calls = mockFetch({
      'GET /api/admin/projects': () => ({ body: [] }),
      'GET /api/admin/feedback': () => ({ body: { items: [item(1)], total: 1, mode: 'hybrid', semantic: true } }),
      'POST /api/admin/feedback/bulk': () => ({ body: { affected: 1 } }),
    });
    const confirmSpy = vi.fn(() => false);
    vi.stubGlobal('confirm', confirmSpy);
    render(<Inbox params={new URLSearchParams()} />);
    await screen.findAllByTestId('feedback-row');
    fireEvent.click(screen.getByLabelText('Select all'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(confirmSpy).toHaveBeenCalled();
    expect(calls.some((c) => c.url === '/api/admin/feedback/bulk')).toBe(false);
  });

  it('focuses search with / and opens the cursor item with j and Enter', async () => {
    mockFetch({
      'GET /api/admin/projects': () => ({ body: [] }),
      'GET /api/admin/feedback': () => ({ body: { items: [item(1), item(2)], total: 2, mode: 'hybrid', semantic: true } }),
    });
    render(<Inbox params={new URLSearchParams()} />);
    await screen.findAllByTestId('feedback-row');
    fireEvent.keyDown(document.body, { key: '/' });
    expect(document.activeElement).toBe(screen.getByLabelText('Search'));
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: 'j' });
    fireEvent.keyDown(document.body, { key: 'Enter' });
    await waitFor(() => expect(location.hash).toContain('open=2'));
  });
});
