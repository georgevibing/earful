import { useEffect, useState } from 'preact/hooks';
import { api } from '../api';
import { toast } from '../toast';
import { KINDS, STATUSES, type Feedback, type FeedbackDetail as Detail, type FeedbackPatch, type Kind } from '../types';
import { formatDateTime, relativeTime } from '../util';
import { Highlight } from './Highlight';
import { FeedbackLabels } from './Labels';

interface Props {
  id: number;
  terms: string[];
  projectName: (id: number) => string;
  openHref: (id: number) => string;
  onClose: () => void;
  onChanged: (item: Feedback) => void;
  onDeleted: (id: number) => void;
}

export function FeedbackDetail({ id, terms, projectName, openHref, onClose, onChanged, onDeleted }: Props) {
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState(false);
  const [note, setNote] = useState('');

  useEffect(() => {
    setDetail(null);
    setError(false);
    api
      .feedbackItem(id)
      .then((d) => {
        setDetail(d);
        setNote(d.item.admin_note);
      })
      .catch(() => setError(true));
  }, [id]);

  if (error)
    return (
      <aside class="detail" aria-label="Feedback detail">
        <div class="row">
          <span class="muted">Item not found.</span>
          <span class="spacer" />
          <button class="small ghost" onClick={onClose} aria-label="Close">
            Close
          </button>
        </div>
      </aside>
    );
  if (!detail)
    return (
      <aside class="detail" aria-label="Feedback detail">
        <div class="muted">Loading...</div>
      </aside>
    );

  const f = detail.item;

  const patch = async (p: FeedbackPatch, message?: string) => {
    try {
      const item = await api.updateFeedback(f.id, p);
      setDetail({ ...detail, item });
      onChanged(item);
      if (message) toast(message);
    } catch {
      toast('Update failed');
    }
  };

  const saveNote = () => {
    if (note !== f.admin_note) void patch({ admin_note: note }, 'Note saved');
  };

  const remove = async () => {
    if (!confirm('Delete this item permanently?')) return;
    await api.deleteFeedback(f.id);
    toast('Deleted');
    onDeleted(f.id);
  };

  const metaText = f.meta && typeof f.meta === 'object' && Object.keys(f.meta).length > 0 ? JSON.stringify(f.meta, null, 2) : null;

  return (
    <aside class="detail" aria-label="Feedback detail">
      <div class="row">
        <span class={`dot ${f.kind}`} aria-hidden="true" />
        <select
          aria-label="Kind"
          value={f.kind}
          onChange={(e) => void patch({ kind: e.currentTarget.value as Kind }, 'Kind changed')}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
        <span class="muted small">#{f.id}</span>
        <span class="spacer" />
        <button class="small ghost" onClick={onClose} aria-label="Close detail" title="Close (Esc)">
          Close
        </button>
      </div>
      <h2>
        <Highlight text={f.title} terms={terms} />
      </h2>
      <div class="body">
        <Highlight text={f.body} terms={terms} />
      </div>

      <div class="seg-buttons" role="group" aria-label="Status">
        {STATUSES.map((s) => (
          <button
            key={s}
            class={`small${f.status === s ? ' active' : ''}`}
            aria-pressed={f.status === s}
            onClick={() => f.status !== s && void patch({ status: s }, `Marked ${s}`)}
          >
            {s}
          </button>
        ))}
      </div>

      <FeedbackLabels item={f} onChanged={(item) => { setDetail({ ...detail, item }); onChanged(item); }} />

      <dl>
        <dt>Project</dt>
        <dd>{projectName(f.project_id)}</dd>
        <dt>Received</dt>
        <dd>
          {formatDateTime(f.created_at)} ({relativeTime(f.created_at)})
        </dd>
        {f.dup_count > 0 && (
          <>
            <dt>Duplicates</dt>
            <dd>
              Sent {f.dup_count} more time{f.dup_count === 1 ? '' : 's'}, last {relativeTime(f.last_seen_at)}
            </dd>
          </>
        )}
        <dt>Contact</dt>
        <dd>{f.contact || <span class="muted">none</span>}</dd>
        {f.rating !== null && (
          <>
            <dt>Rating</dt>
            <dd>
              <span class="stars" aria-label={`${f.rating} of 5`}>
                {'★'.repeat(f.rating)}
                {'☆'.repeat(5 - f.rating)}
              </span>
            </dd>
          </>
        )}
        <dt>Source</dt>
        <dd>{f.source}</dd>
        <dt>Spam score</dt>
        <dd>
          {f.spam_score.toFixed(2)}
          {f.spam_reason && <span class="badge spam" style={{ marginLeft: '6px' }}>{f.spam_reason}</span>}
        </dd>
      </dl>

      {metaText && (
        <div>
          <h3 style={{ marginBottom: '4px' }}>Metadata</h3>
          <pre>{metaText}</pre>
        </div>
      )}

      <label class="field">
        Admin note (saved when you leave the field)
        <textarea rows={3} value={note} onInput={(e) => setNote(e.currentTarget.value)} onBlur={saveNote} />
      </label>

      {detail.similar.length > 0 && (
        <div class="similar">
          <h3 style={{ marginBottom: '4px' }}>Similar</h3>
          {detail.similar.map((s) => (
            <a key={s.id} href={openHref(s.id)}>
              <span class={`dot ${s.kind}`} aria-hidden="true" /> {s.title}{' '}
              <span class="muted small">
                {s.score !== undefined ? `${Math.round(s.score * 100)}% ` : ''}
                {s.status}
              </span>
            </a>
          ))}
        </div>
      )}

      <div class="row">
        <span class="spacer" />
        <button class="danger small" onClick={remove}>
          Delete
        </button>
      </div>
    </aside>
  );
}
