import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { api } from '../api';
import { filtersFromParams, filtersToParams, toApiQuery, type Filters } from '../filters';
import { href, navigate } from '../router';
import { toast } from '../toast';
import { MODES, type BulkAction, type Feedback, type Mode, type Project, type StatusFilter } from '../types';
import { formatDate, relativeTime, searchTerms, snippet } from '../util';
import { FeedbackDetail } from './FeedbackDetail';
import { Highlight } from './Highlight';

const PAGE = 50;

const TABS: { label: string; value: StatusFilter }[] = [
  { label: 'Open', value: 'open' },
  { label: 'New', value: 'new' },
  { label: 'Triaged', value: 'triaged' },
  { label: 'Resolved', value: 'resolved' },
  { label: 'Archived', value: 'archived' },
  { label: 'Spam', value: 'spam' },
  { label: 'All', value: 'all' },
];

const MODE_LABELS: Record<Mode, string> = {
  hybrid: 'Hybrid',
  fuzzy: 'Fuzzy',
  keyword: 'Keyword',
  semantic: 'Semantic',
};

const PRESETS = [7, 30, 90] as const;

function daysAgo(n: number): string {
  return formatDate(Math.floor(Date.now() / 1000) - n * 86400);
}

function presetOf(f: Filters): string {
  if (!f.from && !f.to) return 'any';
  if (!f.to) for (const n of PRESETS) if (f.from === daysAgo(n)) return String(n);
  return 'custom';
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

export function Inbox({ params }: { params: URLSearchParams }) {
  const filters = useMemo(() => filtersFromParams(params), [params.toString()]);
  const listKey = filtersToParams({ ...filters, open: undefined }).toString();

  const [projects, setProjects] = useState<Project[]>([]);
  const [items, setItems] = useState<Feedback[]>([]);
  const [total, setTotal] = useState(0);
  const [semantic, setSemantic] = useState(true);
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [cursor, setCursor] = useState(0);
  const [qInput, setQInput] = useState(filters.q);
  const [reload, setReload] = useState(0);
  const [customRange, setCustomRange] = useState(presetOf(filters) === 'custom');
  const searchRef = useRef<HTMLInputElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  const terms = useMemo(() => searchTerms(filters.q), [filters.q]);

  const update = useCallback(
    (patch: Partial<Filters>, replace = false) => {
      navigate(href('/feedback', filtersToParams({ ...filters, ...patch })), replace);
    },
    [filters],
  );

  useEffect(() => {
    api.projects().then(setProjects).catch(() => toast('Could not load projects'));
  }, []);

  // Keep the input in sync when the URL changes from elsewhere (back button, links).
  useEffect(() => setQInput(filters.q), [filters.q]);

  // Debounced live search.
  useEffect(() => {
    if (qInput === filters.q) return;
    const t = setTimeout(() => update({ q: qInput, open: undefined }, true), 200);
    return () => clearTimeout(t);
  }, [qInput]);

  useEffect(() => {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setLoading(true);
    api
      .feedback(toApiQuery(filters, PAGE, 0), ctrl.signal)
      .then((res) => {
        setItems(res.items);
        setTotal(res.total);
        setSemantic(res.semantic);
        setSelected(new Set());
        setCursor(0);
        setLoaded(true);
      })
      .catch((e: unknown) => {
        if ((e as Error).name !== 'AbortError') toast('Could not load feedback');
      })
      .finally(() => {
        if (abortRef.current === ctrl) setLoading(false);
      });
    return () => ctrl.abort();
  }, [listKey, reload]);

  const loadMore = async () => {
    setLoading(true);
    try {
      const res = await api.feedback(toApiQuery(filters, PAGE, items.length));
      setItems((prev) => [...prev, ...res.items.filter((i) => !prev.some((p) => p.id === i.id))]);
      setTotal(res.total);
    } catch {
      toast('Could not load more');
    } finally {
      setLoading(false);
    }
  };

  const projectName = useCallback((id: number) => projects.find((p) => p.id === id)?.title ?? `#${id}`, [projects]);
  const openHref = useCallback(
    (id: number) => href('/feedback', filtersToParams({ ...filters, open: id })),
    [filters],
  );
  const open = (id: number) => update({ open: id });
  const close = () => update({ open: undefined });

  const runBulk = async (ids: number[], action: BulkAction, label: string) => {
    if (ids.length === 0) return;
    if (action.action === 'delete' && !confirm(`Delete ${ids.length} item${ids.length === 1 ? '' : 's'} permanently?`)) return;
    try {
      const res = await api.bulk(ids, action);
      toast(`${label}: ${res.affected}`);
      if (filters.open && ids.includes(filters.open) && action.action === 'delete') close();
      setReload((n) => n + 1);
    } catch {
      toast('Bulk action failed');
    }
  };

  const selectedIds = [...selected];
  const toggle = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const allSelected = items.length > 0 && items.every((i) => selected.has(i.id));

  // Keyboard shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === 'Escape') {
        if (document.activeElement === searchRef.current) searchRef.current?.blur();
        else if (filters.open) close();
        return;
      }
      if (isTyping(e.target)) return;
      if (e.key === '/') {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.key === 'j') {
        setCursor((c) => Math.min(c + 1, items.length - 1));
      } else if (e.key === 'k') {
        setCursor((c) => Math.max(c - 1, 0));
      } else if (e.key === 'Enter') {
        const item = items[cursor];
        if (item) open(item.id);
      } else if (e.key === 'e') {
        const item = items[cursor];
        if (item) void runBulk([item.id], { action: 'set_status', status: 'archived' }, 'Archived');
      } else if (e.key === 'x') {
        const item = items[cursor];
        if (item) toggle(item.id);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  });

  useEffect(() => {
    document.querySelector<HTMLElement>('.item.cursor')?.scrollIntoView?.({ block: 'nearest' });
  }, [cursor]);

  const preset = customRange ? 'custom' : presetOf(filters);
  const onPreset = (v: string) => {
    if (v === 'custom') {
      setCustomRange(true);
      return;
    }
    setCustomRange(false);
    if (v === 'any') update({ from: undefined, to: undefined });
    else update({ from: daysAgo(Number(v)), to: undefined });
  };

  return (
    <div>
      <div class="filters" role="search">
        <input
          ref={searchRef}
          type="search"
          placeholder="Search feedback (press / to focus)"
          aria-label="Search"
          value={qInput}
          onInput={(e) => setQInput(e.currentTarget.value)}
        />
        <select aria-label="Search mode" value={filters.mode} onChange={(e) => update({ mode: e.currentTarget.value as Mode })}>
          {MODES.map((m) => (
            <option key={m} value={m}>
              {MODE_LABELS[m]}
            </option>
          ))}
        </select>
        <select
          aria-label="Project"
          value={filters.projectId ?? ''}
          onChange={(e) => update({ projectId: e.currentTarget.value ? Number(e.currentTarget.value) : undefined, open: undefined })}
        >
          <option value="">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.title}
            </option>
          ))}
        </select>
        <select
          aria-label="Kind"
          value={filters.kind ?? ''}
          onChange={(e) => update({ kind: (e.currentTarget.value || undefined) as Filters['kind'], open: undefined })}
        >
          <option value="">Bugs and feedback</option>
          <option value="bug">Bugs</option>
          <option value="feedback">Feedback</option>
        </select>
        <select aria-label="Date range" value={preset} onChange={(e) => onPreset(e.currentTarget.value)}>
          <option value="any">Any time</option>
          {PRESETS.map((n) => (
            <option key={n} value={String(n)}>
              Last {n} days
            </option>
          ))}
          <option value="custom">Custom range</option>
        </select>
        {preset === 'custom' && (
          <>
            <input
              type="date"
              aria-label="From"
              value={filters.from ?? ''}
              onChange={(e) => update({ from: e.currentTarget.value || undefined })}
            />
            <input
              type="date"
              aria-label="To"
              value={filters.to ?? ''}
              onChange={(e) => update({ to: e.currentTarget.value || undefined })}
            />
          </>
        )}
      </div>

      <div class="tabs" role="tablist" aria-label="Status">
        {TABS.map((t) => (
          <button
            key={t.value}
            role="tab"
            aria-selected={filters.status === t.value}
            class={filters.status === t.value ? 'active' : undefined}
            onClick={() => update({ status: t.value, open: undefined })}
          >
            {t.label}
          </button>
        ))}
      </div>

      {filters.q && !semantic && (filters.mode === 'semantic' || filters.mode === 'hybrid') && (
        <div class="notice">Semantic search is unavailable right now, results use keyword and fuzzy matching only.</div>
      )}

      {selected.size > 0 && (
        <div class="bulkbar" role="toolbar" aria-label="Bulk actions">
          <strong>{selected.size} selected</strong>
          <button class="small" onClick={() => runBulk(selectedIds, { action: 'set_status', status: 'triaged' }, 'Triaged')}>
            Triage
          </button>
          <button class="small" onClick={() => runBulk(selectedIds, { action: 'set_status', status: 'resolved' }, 'Resolved')}>
            Resolve
          </button>
          <button class="small" onClick={() => runBulk(selectedIds, { action: 'set_status', status: 'archived' }, 'Archived')}>
            Archive
          </button>
          <button class="small" onClick={() => runBulk(selectedIds, { action: 'set_status', status: 'spam' }, 'Marked spam')}>
            Mark spam
          </button>
          <button class="small" onClick={() => runBulk(selectedIds, { action: 'set_status', status: 'new' }, 'Restored')}>
            Not spam
          </button>
          <button class="small danger" onClick={() => runBulk(selectedIds, { action: 'delete' }, 'Deleted')}>
            Delete
          </button>
          <span class="spacer" />
          <button class="small ghost" onClick={() => setSelected(new Set())}>
            Clear
          </button>
        </div>
      )}

      <div class={`inbox${filters.open ? ' with-detail' : ''}`}>
        <div class={`list${loading && loaded ? ' loading' : ''}`} aria-busy={loading}>
          {items.length > 0 && (
            <div class="list-foot" style={{ borderTop: 'none', borderBottom: '1px solid var(--grid)' }}>
              <label class="check">
                <input
                  type="checkbox"
                  aria-label="Select all"
                  checked={allSelected}
                  onChange={() => setSelected(allSelected ? new Set() : new Set(items.map((i) => i.id)))}
                />
                {total} item{total === 1 ? '' : 's'}
              </label>
              <span class="spacer" />
              <span class="small">
                <kbd>j</kbd>/<kbd>k</kbd> move, <kbd>Enter</kbd> open, <kbd>x</kbd> select, <kbd>e</kbd> archive
              </span>
            </div>
          )}
          {!loaded ? (
            <div class="empty">Loading...</div>
          ) : items.length === 0 ? (
            <div class="empty">{filters.q ? 'No matches.' : 'Nothing here. Inbox zero.'}</div>
          ) : (
            items.map((f, i) => (
              <Row
                key={f.id}
                item={f}
                terms={terms}
                project={projectName(f.project_id)}
                isCursor={i === cursor}
                isOpen={filters.open === f.id}
                checked={selected.has(f.id)}
                onToggle={() => toggle(f.id)}
                onOpen={() => {
                  setCursor(i);
                  open(f.id);
                }}
              />
            ))
          )}
          {items.length < total && (
            <div class="list-foot">
              <span>
                Showing {items.length} of {total}
              </span>
              <span class="spacer" />
              <button class="small" onClick={loadMore} disabled={loading}>
                Load more
              </button>
            </div>
          )}
        </div>

        {filters.open && (
          <FeedbackDetail
            key={filters.open}
            id={filters.open}
            terms={terms}
            projectName={projectName}
            openHref={openHref}
            onClose={close}
            onChanged={(item) => setItems((prev) => prev.map((p) => (p.id === item.id ? { ...item, score: p.score } : p)))}
            onDeleted={(id) => {
              setItems((prev) => prev.filter((p) => p.id !== id));
              setTotal((t) => Math.max(0, t - 1));
              close();
            }}
          />
        )}
      </div>
    </div>
  );
}

interface RowProps {
  item: Feedback;
  terms: string[];
  project: string;
  isCursor: boolean;
  isOpen: boolean;
  checked: boolean;
  onToggle: () => void;
  onOpen: () => void;
}

function Row({ item: f, terms, project, isCursor, isOpen, checked, onToggle, onOpen }: RowProps) {
  const cls = ['item', isCursor && 'cursor', isOpen && 'open'].filter(Boolean).join(' ');
  return (
    <div class={cls} onClick={onOpen} data-testid="feedback-row">
      <input
        type="checkbox"
        aria-label={`Select ${f.title}`}
        checked={checked}
        onClick={(e) => e.stopPropagation()}
        onChange={onToggle}
      />
      <div style={{ minWidth: 0 }}>
        <div class="title">
          <Highlight text={f.title} terms={terms} />
        </div>
        <div class="snip">
          <Highlight text={snippet(f.body, terms)} terms={terms} />
        </div>
        <div class="meta">
          <span class="badge">
            <span class={`dot ${f.kind}`} aria-hidden="true" />
            {f.kind}
          </span>
          <span>{project}</span>
          <span title={new Date(f.created_at * 1000).toLocaleString()}>{relativeTime(f.created_at)}</span>
          <span class="badge">{f.status}</span>
          {f.dup_count > 0 && (
            <span class="badge dup" title={`${f.dup_count} duplicate submissions merged`}>
              +{f.dup_count}
            </span>
          )}
          {f.rating !== null && (
            <span class="stars" aria-label={`${f.rating} of 5`}>
              {'★'.repeat(f.rating)}
              {'☆'.repeat(5 - f.rating)}
            </span>
          )}
          {f.spam_reason && <span class="badge spam">{f.spam_reason}</span>}
          {f.score !== undefined && <span class="mono">{f.score.toFixed(3)}</span>}
        </div>
      </div>
    </div>
  );
}
