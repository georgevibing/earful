import { useEffect, useState } from 'preact/hooks';
import { ProjectLabels } from './Labels';
import { api } from '../api';
import { DEFAULT_FILTERS, filtersToParams } from '../filters';
import { href, navigate } from '../router';
import { integrationSnippets } from '../snippets';
import { toast } from '../toast';
import type { Project } from '../types';
import { copyText, formatDateTime, relativeTime } from '../util';

export function ProjectDetail({ id }: { id: number }) {
  const [project, setProject] = useState<Project | null>(null);
  const [missing, setMissing] = useState(false);
  const [confirmSlug, setConfirmSlug] = useState('');

  useEffect(() => {
    api.project(id).then(setProject).catch(() => setMissing(true));
  }, [id]);

  if (missing)
    return (
      <div class="empty">
        Project not found. <a href="#/projects">Back to projects</a>
      </div>
    );
  if (!project) return <div class="empty">Loading...</div>;
  const p = project;

  const copy = async (text: string, what: string) => toast((await copyText(text)) ? `${what} copied` : 'Copy failed');

  const rotate = async () => {
    if (!confirm('Rotate the ingest key? Clients using the old key stop working immediately.')) return;
    setProject(await api.rotateKey(p.id));
    toast('Ingest key rotated');
  };

  const toggleAccepting = async () => {
    setProject(await api.updateProject(p.id, { accepting: !p.accepting }));
    toast(p.accepting ? 'Submissions paused' : 'Submissions resumed');
  };

  const remove = async () => {
    if (confirmSlug !== p.slug) return;
    await api.deleteProject(p.id);
    toast('Project deleted');
    navigate('/projects');
  };

  const origin = location.origin;
  const s = p.stats;

  return (
    <div class="stack">
      <div class="page-head" style={{ marginBottom: 0 }}>
        <h1>{p.title}</h1>
        <span class="badge">{p.project_type}</span>
        {!p.accepting && <span class="badge">paused</span>}
        <span class="spacer" />
        <a class="btn" href={href('/feedback', filtersToParams({ ...DEFAULT_FILTERS, projectId: p.id }))}>
          View feedback
        </a>
        <a class="btn" href={`#/projects/${p.id}/edit`}>
          Edit
        </a>
      </div>
      {p.description && <p class="muted" style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{p.description}</p>}

      <section class="card stack">
        <h2>Ingest key</h2>
        <div class="key-box">
          <code data-testid="ingest-key">{p.ingest_key}</code>
          <button class="small" onClick={() => copy(p.ingest_key, 'Key')}>
            Copy
          </button>
          <button class="small" onClick={rotate}>
            Rotate
          </button>
        </div>
        <div class="row small muted">
          The key ships inside your client, so it is public by design. Rotate it if a project gets abused.
        </div>
        <div class="row">
          <button onClick={toggleAccepting}>{p.accepting ? 'Pause submissions' : 'Resume submissions'}</button>
          <a class="btn" href={api.exportUrl(p.id, 'csv')} download>
            Export CSV
          </a>
          <a class="btn" href={api.exportUrl(p.id, 'json')} download>
            Export JSON
          </a>
        </div>
        {s && (
          <div class="small muted">
            {s.total} total, {s.open} open, {s.bugs} bugs, {s.feedback} feedback, {s.spam} in spam. Last submission{' '}
            {s.last_at ? relativeTime(s.last_at) : 'never'}. Created {formatDateTime(p.created_at)}.
          </div>
        )}
      </section>

      <section class="card">
        <h2>Integrate</h2>
        {integrationSnippets(p, origin).map((sn) => (
          <div class="snippet" key={sn.title}>
            <div class="row">
              <h3>{sn.title}</h3>
              <span class="muted small">{sn.description}</span>
              <span class="spacer" />
              <button class="small" onClick={() => copy(sn.code, sn.title)}>
                Copy
              </button>
            </div>
            <pre>{sn.code}</pre>
          </div>
        ))}
      </section>

      <ProjectLabels projectId={p.id} />

      <section class="card danger-zone stack">
        <h2>Delete project</h2>
        <div class="small muted">
          Deletes the project and all of its feedback permanently. Type <code>{p.slug}</code> to confirm.
        </div>
        <div class="row">
          <input
            type="text"
            aria-label="Confirm slug"
            value={confirmSlug}
            onInput={(e) => setConfirmSlug(e.currentTarget.value)}
            placeholder={p.slug}
          />
          <button class="danger" disabled={confirmSlug !== p.slug} onClick={remove}>
            Delete forever
          </button>
        </div>
      </section>
    </div>
  );
}
