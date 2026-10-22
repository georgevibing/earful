import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { mockFetch } from '../test-utils';
import { DEFAULT_FILTERS } from '../filters';
import type { Feedback, LabelSummary } from '../types';
import { BulkLabels, FeedbackLabels, LabelFilters, ProjectLabels } from './Labels';

const label: LabelSummary = { id: 3, project_id: 1, name: 'Save integrity', color: 'blue', description: 'Lost or corrupted progress', created_at: 0, updated_at: 0, feedback_count: 2, open_count: 1 };
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('labels', () => {
  it('creates, renames, and deletes a definition without deleting feedback', async () => {
    let labels = [label];
    const calls = mockFetch({
      'GET /api/admin/labels': () => ({ body: labels }),
      'POST /api/admin/labels': (c) => { const next = { ...label, id: 4, ...c.body as object }; labels = [...labels, next]; return { status: 201, body: next }; },
      'PATCH /api/admin/labels/3': (c) => { labels = labels.map((l) => l.id === 3 ? { ...l, ...c.body as object } : l); return { body: labels[0] }; },
      'DELETE /api/admin/labels/3': () => { labels = labels.filter((l) => l.id !== 3); return { status: 204 }; },
    });
    vi.stubGlobal('confirm', vi.fn(() => true));
    render(<ProjectLabels projectId={1} />);
    await screen.findByRole('button', { name: 'Edit Save integrity' });
    fireEvent.input(screen.getByLabelText('Label name'), { target: { value: 'Accessibility' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create label' }));
    await screen.findByRole('button', { name: 'Edit Accessibility' });
    expect(calls.find((c) => c.method === 'POST')?.body).toMatchObject({ project_id: 1, name: 'Accessibility' });
    fireEvent.click(screen.getByRole('button', { name: 'Edit Save integrity' }));
    fireEvent.input(screen.getByLabelText('Label name'), { target: { value: 'Save reliability' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save label' }));
    await screen.findByRole('button', { name: 'Delete Save reliability' });
    fireEvent.click(screen.getByRole('button', { name: 'Delete Save reliability' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Delete Save reliability' })).toBeNull());
    expect(calls.filter((c) => c.method === 'DELETE').map((c) => c.url)).toEqual(['/api/admin/labels/3']);
  });

  it('shows validation errors and retains the entered definition', async () => {
    mockFetch({ 'GET /api/admin/labels': () => ({ body: [label] }), 'POST /api/admin/labels': () => ({ status: 409, body: { error: 'conflict', message: 'Label already exists' } }) });
    render(<ProjectLabels projectId={1} />);
    fireEvent.input(screen.getByLabelText('Label name'), { target: { value: label.name } });
    fireEvent.click(screen.getByRole('button', { name: 'Create label' }));
    await screen.findByRole('alert');
    expect((screen.getByLabelText('Label name') as HTMLInputElement).value).toBe(label.name);
  });

  it('applies an incremental assignment and refreshes the item', async () => {
    const item = { id: 8, project_id: 1, labels: [] } as unknown as Feedback;
    const updated = { ...item, labels: [label] };
    const changed = vi.fn();
    const calls = mockFetch({ 'GET /api/admin/labels': () => ({ body: [label] }), 'POST /api/admin/feedback/labels': () => ({ body: { affected: 1 } }), 'GET /api/admin/feedback/8': () => ({ body: { item: updated, similar: [] } }) });
    render(<FeedbackLabels item={item} onChanged={changed} />);
    fireEvent.click(await screen.findByRole('checkbox', { name: label.name }));
    await waitFor(() => expect(changed).toHaveBeenCalledWith(updated));
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({ ids: [8], label_ids: [3], action: 'add' });
  });

  it('rolls a failed optimistic assignment back', async () => {
    const calls = mockFetch({ 'GET /api/admin/labels': () => ({ body: [label] }), 'POST /api/admin/feedback/labels': () => ({ status: 400, body: { error: 'invalid_labels', message: 'Assignment rejected' } }) });
    const changed = vi.fn();
    render(<FeedbackLabels item={{ id: 8, project_id: 1, labels: [] } as unknown as Feedback} onChanged={changed} />);
    const checkbox = await screen.findByRole('checkbox', { name: label.name }) as HTMLInputElement;
    fireEvent.click(checkbox);
    await waitFor(() => {
      expect(calls.some((c) => c.method === 'POST')).toBe(true);
      expect(checkbox.disabled).toBe(false);
      expect(checkbox.checked).toBe(false);
    });
    expect(changed).not.toHaveBeenCalled();
  });

  it('keeps unlabeled and selected-label filters mutually exclusive', async () => {
    mockFetch({ 'GET /api/admin/labels': () => ({ body: [label] }) });
    const update = vi.fn();
    render(<LabelFilters filters={{ ...DEFAULT_FILTERS, labelIds: [3] }} update={update} />);
    await screen.findByRole('checkbox', { name: /Save integrity/ });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Unlabeled only' }));
    expect(update).toHaveBeenCalledWith({ unlabeled: true, labelIds: undefined, labelMatch: undefined, open: undefined });
  });

  it('prevents bulk labeling across projects', async () => {
    mockFetch({ 'GET /api/admin/labels': () => ({ body: [label] }) });
    render(<BulkLabels items={[{ id: 1, project_id: 1 }, { id: 2, project_id: 2 }] as Feedback[]} ids={[1, 2]} onChanged={() => {}} />);
    expect(screen.getByText('Select feedback from one project to label it.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Add label' })).toBeNull();
  });
});
