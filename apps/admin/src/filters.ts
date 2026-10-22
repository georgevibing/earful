import type { FeedbackQuery } from './api';
import { KINDS, MODES, STATUSES, type Kind, type Mode, type StatusFilter } from './types';

export interface Filters {
  labelIds?: number[];
  labelMatch?: 'all' | 'any';
  unlabeled?: boolean;
  projectId?: number;
  kind?: Kind;
  status: StatusFilter;
  q: string;
  mode: Mode;
  /** Inclusive local dates, `YYYY-MM-DD`. */
  from?: string;
  to?: string;
  /** Feedback id open in the detail panel. */
  open?: number;
}

export const DEFAULT_FILTERS: Filters = { status: 'open', q: '', mode: 'hybrid' };

const STATUS_FILTERS: readonly string[] = ['open', 'all', 'any', ...STATUSES];
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function positiveInt(v: string | null): number | undefined {
  if (!v) return undefined;
  const n = Number(v);
  return Number.isInteger(n) && n > 0 ? n : undefined;
}

export function filtersFromParams(p: URLSearchParams): Filters {
  const kind = p.get('kind');
  const status = p.get('status');
  const mode = p.get('mode');
  const from = p.get('from');
  const to = p.get('to');
  const labelIds = [...new Set((p.get('label_ids') ?? '').split(',').map((s) => positiveInt(s.trim())).filter((id): id is number => id !== undefined))].slice(0, 20);
  const unlabeled = p.get('unlabeled') === 'true';
  return {
    ...(!unlabeled && labelIds.length ? { labelIds } : {}),
    ...(labelIds.length && !unlabeled && p.get('label_match') === 'any' ? { labelMatch: 'any' as const } : {}),
    ...(unlabeled ? { unlabeled: true } : {}),
    projectId: positiveInt(p.get('project_id')),
    kind: kind && (KINDS as readonly string[]).includes(kind) ? (kind as Kind) : undefined,
    status: status && STATUS_FILTERS.includes(status) ? (status as StatusFilter) : 'open',
    q: p.get('q') ?? '',
    mode: mode && (MODES as readonly string[]).includes(mode) ? (mode as Mode) : 'hybrid',
    from: from && DATE_RE.test(from) ? from : undefined,
    to: to && DATE_RE.test(to) ? to : undefined,
    open: positiveInt(p.get('open')),
  };
}

/** Serialises filters for the URL hash, omitting defaults so links stay short. */
export function filtersToParams(f: Filters): URLSearchParams {
  const p = new URLSearchParams();
  if (f.unlabeled) p.set('unlabeled', 'true');
  else if (f.labelIds?.length) {
    p.set('label_ids', f.labelIds.join(','));
    if (f.labelMatch === 'any') p.set('label_match', 'any');
  }
  if (f.projectId) p.set('project_id', String(f.projectId));
  if (f.kind) p.set('kind', f.kind);
  if (f.status !== 'open') p.set('status', f.status);
  if (f.q) p.set('q', f.q);
  if (f.mode !== 'hybrid') p.set('mode', f.mode);
  if (f.from) p.set('from', f.from);
  if (f.to) p.set('to', f.to);
  if (f.open) p.set('open', String(f.open));
  return p;
}

/** Local midnight of a `YYYY-MM-DD` date as Unix seconds. */
export function dayStart(date: string): number {
  const [y, m, d] = date.split('-').map(Number) as [number, number, number];
  return Math.floor(new Date(y, m - 1, d).getTime() / 1000);
}

export function toApiQuery(f: Filters, limit: number, offset: number): FeedbackQuery {
  const q = f.q.trim();
  return {
    label_ids: !f.unlabeled && f.labelIds?.length ? f.labelIds.join(',') : undefined,
    label_match: !f.unlabeled && f.labelIds?.length ? f.labelMatch ?? 'all' : undefined,
    unlabeled: f.unlabeled || undefined,
    project_id: f.projectId,
    kind: f.kind,
    status: f.status,
    q: q || undefined,
    mode: q ? f.mode : undefined,
    from: f.from ? dayStart(f.from) : undefined,
    to: f.to ? dayStart(f.to) + 86400 - 1 : undefined,
    limit,
    offset,
  };
}
