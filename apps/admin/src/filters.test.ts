import { describe, expect, it } from 'vitest';
import { DEFAULT_FILTERS, dayStart, filtersFromParams, filtersToParams, toApiQuery } from './filters';

describe('filters', () => {
  it('defaults when params are empty or invalid', () => {
    expect(filtersFromParams(new URLSearchParams())).toEqual({ ...DEFAULT_FILTERS, projectId: undefined, kind: undefined, from: undefined, to: undefined, open: undefined });
    const f = filtersFromParams(new URLSearchParams('status=bogus&mode=nope&kind=x&project_id=-3&from=yesterday'));
    expect(f.status).toBe('open');
    expect(f.mode).toBe('hybrid');
    expect(f.kind).toBeUndefined();
    expect(f.projectId).toBeUndefined();
    expect(f.from).toBeUndefined();
  });

  it('round trips through the URL and omits defaults', () => {
    const f = { status: 'spam' as const, q: 'crash', mode: 'fuzzy' as const, projectId: 3, kind: 'bug' as const, from: '2026-10-01', to: '2026-10-05', open: 9 };
    const p = filtersToParams(f);
    expect(filtersFromParams(p)).toEqual(f);
    expect(filtersToParams(DEFAULT_FILTERS).toString()).toBe('');
  });

  it('converts to the API query', () => {
    const q = toApiQuery({ ...DEFAULT_FILTERS, q: '  ', mode: 'semantic', from: '2026-10-01', to: '2026-10-01' }, 50, 100);
    expect(q.q).toBeUndefined();
    expect(q.mode).toBeUndefined();
    expect(q.status).toBe('open');
    expect(q.from).toBe(dayStart('2026-10-01'));
    expect(q.to).toBe(dayStart('2026-10-01') + 86399);
    expect(q.limit).toBe(50);
    expect(q.offset).toBe(100);
    expect(toApiQuery({ ...DEFAULT_FILTERS, q: 'x', mode: 'keyword' }, 1, 0)).toMatchObject({ q: 'x', mode: 'keyword' });
  });
});

it('preserves label matching in shareable links and API queries', () => {
  const f = { ...DEFAULT_FILTERS, projectId: 4, labelIds: [3, 9], labelMatch: 'any' as const };
  expect(filtersFromParams(filtersToParams(f))).toMatchObject(f);
  expect(toApiQuery(f, 50, 0)).toMatchObject({ project_id: 4, label_ids: '3,9', label_match: 'any' });
  const unlabeled = { ...f, unlabeled: true };
  expect(filtersToParams(unlabeled).has('label_ids')).toBe(false);
  expect(toApiQuery(unlabeled, 50, 0)).toMatchObject({ unlabeled: true, label_ids: undefined, label_match: undefined });
  expect(filtersFromParams(new URLSearchParams('label_ids=3,3,-1,9,bogus'))).toMatchObject({ labelIds: [3, 9] });
});
