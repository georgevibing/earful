import { useCallback, useEffect, useState } from 'preact/hooks';
import { api, ApiError } from '../api';
import type { Filters } from '../filters';
import { LABEL_COLORS, type Feedback, type Label, type LabelColor, type LabelInput, type LabelSummary, type Project } from '../types';
import { filtersToParams, DEFAULT_FILTERS } from '../filters';
import { href } from '../router';
import { toast } from '../toast';

function message(e: unknown) { return e instanceof ApiError ? e.message : 'Could not update labels.'; }

function useLabels(projectId?: number, revisionKey?: number) {
  const [labels, setLabels] = useState<LabelSummary[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [revision, setRevision] = useState(0);
  const refresh = useCallback(() => setRevision((n) => n + 1), []);
  useEffect(() => {
    let active = true;
    setLabels(null); setFailed(false);
    api.labels(projectId).then((v) => { if (active) setLabels(v); }).catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [projectId, revision, revisionKey]);
  return { labels, failed, refresh };
}

export function LabelChip({ label }: { label: Label }) {
  return <span class={`label-chip label-${label.color}`} title={label.description || undefined}>{label.name}</span>;
}

export function ProjectLabels({ projectId }: { projectId: number }) {
  const { labels, failed, refresh } = useLabels(projectId);
  const [editing, setEditing] = useState<number | null>(null);
  const [input, setInput] = useState<LabelInput>({ name: '', color: 'slate', description: '' });
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const reset = () => { setEditing(null); setInput({ name: '', color: 'slate', description: '' }); setError(''); };
  const save = async (e: Event) => {
    e.preventDefault(); setBusy(true); setError('');
    try {
      if (editing === null) await api.createLabel(projectId, input);
      else await api.updateLabel(editing, input);
      reset(); refresh(); toast('Label saved');
    } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  const remove = async (label: Label) => {
    if (!confirm(`Delete label "${label.name}"? Its assignments will be removed; feedback will be kept.`)) return;
    setBusy(true);
    try { await api.deleteLabel(label.id); if (editing === label.id) reset(); refresh(); toast('Label deleted'); }
    catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  return <section class="card stack" aria-label="Project labels">
    <h2>Labels and themes</h2>
    <p class="small muted" style={{ margin: 0 }}>Group related feedback into themes. Descriptions help people and agents apply labels consistently. Counts exclude spam and overlap when feedback has multiple labels.</p>
    {failed ? <div role="alert">Could not load labels. <button onClick={refresh}>Retry</button></div>
      : labels === null ? <p class="muted">Loading labels...</p>
      : labels.length === 0 ? <p class="muted">No labels yet.</p>
      : <ul class="label-list">{labels.map((label) => <li key={label.id}>
        <div class="stack" style={{ gap: '4px' }}>
          <div class="row"><LabelChip label={label} /><span class="small muted">{label.open_count} open / {label.feedback_count} total</span></div>
          {label.description && <p class="small muted" style={{ margin: 0 }}>{label.description}</p>}
        </div>
        <div class="row">
          <a class="btn small" href={href('/feedback', filtersToParams({ ...DEFAULT_FILTERS, projectId, labelIds: [label.id] }))}>View feedback</a>
          <button class="small" aria-label={`Edit ${label.name}`} disabled={busy} onClick={() => { setEditing(label.id); setInput({ name: label.name, color: label.color, description: label.description }); setError(''); }}>Edit</button>
          <button class="small danger" aria-label={`Delete ${label.name}`} disabled={busy} onClick={() => void remove(label)}>Delete</button>
        </div>
      </li>)}</ul>}
    <form class="stack" onSubmit={save}>
      <h3>{editing === null ? 'Create a label' : 'Edit label'}</h3>
      <div class="form-grid">
        <label class="field">Label name<input type="text" required maxLength={64} value={input.name} onInput={(e) => setInput({ ...input, name: e.currentTarget.value })} /></label>
        <label class="field">Label color<select value={input.color} onChange={(e) => setInput({ ...input, color: e.currentTarget.value as LabelColor })}>{LABEL_COLORS.map((c) => <option key={c}>{c}</option>)}</select></label>
        <label class="field full">Description / when to use<textarea maxLength={500} rows={2} value={input.description} onInput={(e) => setInput({ ...input, description: e.currentTarget.value })} /></label>
      </div>
      {error && <p class="error" role="alert">{error}</p>}
      <div class="row"><button class="primary" disabled={busy}>{editing === null ? 'Create label' : 'Save label'}</button>{editing !== null && <button type="button" onClick={reset}>Cancel edit</button>}</div>
    </form>
  </section>;
}

export function FeedbackLabels({ item, onChanged }: { item: Feedback; onChanged: (f: Feedback) => void }) {
  const { labels, failed, refresh } = useLabels(item.project_id);
  const [pending, setPending] = useState<{ id: number; checked: boolean } | null>(null);
  const toggle = async (label: Label, checked: boolean) => {
    setPending({ id: label.id, checked });
    try {
      await api.assignLabels([item.id], [label.id], checked ? 'add' : 'remove');
      onChanged((await api.feedbackItem(item.id)).item);
    } catch (e) { toast(message(e)); } finally { setPending(null); }
  };
  return <section class="stack" style={{ gap: '6px' }} aria-label="Feedback labels">
    <div class="row"><h3>Labels</h3><span class="spacer" /><a class="small" href={`#/projects/${item.project_id}`}>Manage labels</a></div>
    {failed ? <div role="alert">Could not load labels. <button onClick={refresh}>Retry</button></div>
      : labels === null ? <span class="muted small">Loading labels...</span>
      : labels.length === 0 ? <span class="muted small">Create labels from the project page.</span>
      : <div class="label-options">{labels.map((label) => <label class="check" key={label.id} title={label.description}>
        <input type="checkbox" disabled={pending !== null} checked={pending?.id === label.id ? pending.checked : (item.labels ?? []).some((l) => l.id === label.id)} onChange={(e) => void toggle(label, e.currentTarget.checked)} /><LabelChip label={label} />
      </label>)}</div>}
  </section>;
}

export function LabelFilters({ filters, update, revision }: { filters: Filters; update: (patch: Partial<Filters>) => void; revision?: number }) {
  const { labels, failed, refresh } = useLabels(filters.projectId, revision);
  const selected = filters.labelIds ?? [];
  const active = !!(selected.length || filters.unlabeled);
  return <details class="label-filters" open={active || undefined}>
    <summary>Labels{filters.unlabeled ? ' (unlabeled only)' : selected.length ? ` (${selected.length})` : ''}</summary>
    <div class="stack label-filter-content">
      <div class="label-filter-controls">
        <label class="check"><input type="checkbox" checked={!!filters.unlabeled} onChange={(e) => update({ unlabeled: e.currentTarget.checked || undefined, labelIds: undefined, labelMatch: undefined, open: undefined })} />Unlabeled only</label>
        {!!labels?.length && <label class="field">Match labels<select value={filters.labelMatch ?? 'all'} disabled={!!filters.unlabeled} onChange={(e) => update({ labelMatch: e.currentTarget.value === 'any' ? 'any' : undefined, open: undefined })}><option value="all">All selected labels</option><option value="any">Any selected label</option></select></label>}
        {active && <button class="small" onClick={() => update({ labelIds: undefined, labelMatch: undefined, unlabeled: undefined, open: undefined })}>Clear labels</button>}
      </div>
      {failed ? <div role="alert">Could not load labels. <button onClick={refresh}>Retry</button></div>
        : labels === null ? <span class="muted small">Loading labels...</span>
        : labels.length === 0 ? <p class="small muted label-filter-empty">No labels yet. Create labels in a project to filter feedback by theme. <a href={filters.projectId ? `#/projects/${filters.projectId}` : '#/projects'}>Manage labels</a></p>
        : <div class="label-options label-filter-options">{labels.map((label) => <label class={`check label-filter-option${selected.includes(label.id) ? ' selected' : ''}`} key={label.id} title={label.description}>
          <input type="checkbox" disabled={!!filters.unlabeled || (selected.length >= 20 && !selected.includes(label.id))} checked={selected.includes(label.id)} onChange={(e) => update({ labelIds: e.currentTarget.checked ? [...selected, label.id] : selected.filter((id) => id !== label.id), open: undefined })} />
          <LabelChip label={label} /><span class="small muted">{label.open_count} open</span>
        </label>)}</div>}
      {selected.filter((id) => labels && !labels.some((l) => l.id === id)).length > 0 && <p class="small muted">Some selected labels are outside this scope or were deleted. Clear labels to reset this filter.</p>}
    </div>
  </details>;
}

export function BulkLabels({ items, ids, onChanged }: { items: Feedback[]; ids: number[]; onChanged: () => void }) {
  const projects = new Set(items.filter((f) => ids.includes(f.id)).map((f) => f.project_id));
  const projectId = projects.size === 1 ? [...projects][0] : undefined;
  const { labels, failed, refresh } = useLabels(projectId);
  const [labelId, setLabelId] = useState('');
  const [busy, setBusy] = useState(false);
  const valid = projects.size === 1 && !!labels?.some((l) => l.id === Number(labelId));
  const apply = async (action: 'add' | 'remove') => {
    if (!valid) return;
    setBusy(true);
    try { const res = await api.assignLabels(ids, [Number(labelId)], action); toast(`Labels updated: ${res.affected}`); onChanged(); }
    catch (e) { toast(message(e)); } finally { setBusy(false); }
  };
  return <div class="row bulk-labels">
    {projects.size !== 1 ? <span class="small muted">Select feedback from one project to label it.</span>
      : failed ? <button onClick={refresh}>Retry labels</button>
      : <><select aria-label="Bulk label" value={labelId} onChange={(e) => setLabelId(e.currentTarget.value)}><option value="">Choose label</option>{labels?.map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}</select>
        <button class="small" disabled={!valid || busy} onClick={() => void apply('add')}>Add label</button><button class="small" disabled={!valid || busy} onClick={() => void apply('remove')}>Remove label</button></>}
  </div>;
}

export function LabelOverview({ projectId, projects }: { projectId?: number; projects: Project[] }) {
  const { labels, failed, refresh } = useLabels(projectId);
  return <section class="card" aria-label="Theme overview">
    <h2>Themes</h2>
    {failed ? <div role="alert">Could not load themes. <button onClick={refresh}>Retry</button></div>
      : labels === null ? <p class="muted small">Loading themes...</p>
      : labels.length === 0 ? <p class="muted small">Create labels from a project page to group feedback into themes.</p>
      : <><p class="small muted">Top 10 by open feedback. Counts overlap when feedback has several labels.</p><ul class="label-list">{labels.slice(0, 10).map((label) => <li key={label.id}><a href={href('/feedback', filtersToParams({ ...DEFAULT_FILTERS, projectId: label.project_id, labelIds: [label.id] }))}><LabelChip label={label} /></a>{!projectId && <span class="small muted">{projects.find((p) => p.id === label.project_id)?.title ?? `Project #${label.project_id}`}</span>}<span class="small">{label.open_count} open / {label.feedback_count} total</span></li>)}</ul></>}
  </section>;
}
