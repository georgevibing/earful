import { useEffect, useState } from 'preact/hooks';
import { api } from '../api';
import { filtersToParams, DEFAULT_FILTERS } from '../filters';
import { href } from '../router';
import { toast } from '../toast';
import type { Project, Stats } from '../types';
import { compact, relativeTime } from '../util';
import { DailyChart } from './DailyChart';

const BLOCK_LABELS: Record<string, string> = {
  rate_limited: 'Rate limited',
  honeypot: 'Honeypot tripped',
  pow_insufficient: 'Bad proof of work',
  duplicate: 'Exact duplicate',
  spam_score: 'Spam heuristics',
  too_fast: 'Submitted too fast',
  near_duplicate: 'Near duplicate',
};

function feedbackLink(extra: Partial<typeof DEFAULT_FILTERS & { projectId: number }>): string {
  return href('/feedback', filtersToParams({ ...DEFAULT_FILTERS, ...extra }));
}

export function Dashboard() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<number | undefined>(undefined);
  const [stats, setStats] = useState<Stats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.projects().then(setProjects).catch(() => toast('Could not load projects'));
  }, []);

  useEffect(() => {
    setLoading(true);
    api
      .stats(projectId)
      .then(setStats)
      .catch(() => toast('Could not load stats'))
      .finally(() => setLoading(false));
  }, [projectId]);

  const reindex = async () => {
    if (!confirm('Drop all vectors and re-embed every item in the background?')) return;
    await api.reindex();
    toast('Re-embedding started');
  };

  const t = stats?.totals;
  const blocked = stats ? Object.entries(stats.blocked).sort((a, b) => b[1] - a[1]) : [];
  const blockedMax = Math.max(1, ...blocked.map(([, v]) => v));
  const blockedTotal = blocked.reduce((s, [, v]) => s + v, 0);
  const shownProjects = projectId ? projects.filter((p) => p.id === projectId) : projects;
  const emb = stats?.embeddings;
  const embTotal = emb ? emb.embedded + emb.pending : 0;

  return (
    <div>
      <div class="page-head">
        <h1>Dashboard</h1>
        <span class="spacer" />
        <select
          aria-label="Project"
          value={projectId ?? ''}
          onChange={(e) => setProjectId(e.currentTarget.value ? Number(e.currentTarget.value) : undefined)}
        >
          <option value="">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.title}
            </option>
          ))}
        </select>
      </div>

      <div style={{ opacity: loading && stats ? 0.6 : 1, transition: 'opacity .15s' }}>
        <div class="kpis">
          <a class="tile hero" href={feedbackLink({ projectId, status: 'open' })}>
            <span class="label">Open items</span>
            <span class="value">{t ? compact(t.open) : '-'}</span>
            <span class="sub">new and triaged</span>
          </a>
          <Tile label="New" value={t?.new} link={feedbackLink({ projectId, status: 'new' })} />
          <Tile label="Bugs" value={t?.bugs} link={feedbackLink({ projectId, status: 'all', kind: 'bug' })} />
          <Tile label="Feedback" value={t?.feedback} link={feedbackLink({ projectId, status: 'all', kind: 'feedback' })} />
          <Tile label="Last 7 days" value={t?.last_7d} />
          <Tile label="In spam" value={t?.spam} link={feedbackLink({ projectId, status: 'spam' })} />
        </div>

        <div class="dash-grid">
          <section class="card">
            <h2>Submissions, last 30 days</h2>
            {stats ? <DailyChart data={stats.daily} /> : <div class="empty">Loading...</div>}
          </section>
          <div class="stack" style={{ alignContent: 'start' }}>
            <section class="card">
              <h2>Spam blocked, last 30 days</h2>
              {blockedTotal === 0 ? (
                <div class="muted small">Nothing blocked yet.</div>
              ) : (
                <table class="data">
                  <tbody>
                    {blocked.map(([reason, count]) => (
                      <tr key={reason}>
                        <td>{BLOCK_LABELS[reason] ?? reason}</td>
                        <td style={{ width: '40%' }}>
                          {count > 0 && <div class="hbar" style={{ width: `${(count / blockedMax) * 100}%` }} />}
                        </td>
                        <td class="num">{compact(count)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>
            <section class="card">
              <h2>Semantic index</h2>
              {emb && !emb.enabled && <div class="muted small">Embeddings are disabled (FH_EMBED_PROVIDER=none).</div>}
              {emb && emb.enabled && (
                <div class="stack" style={{ gap: '8px' }}>
                  <div class="small">
                    Model <code>{emb.model}</code>
                  </div>
                  <div
                    class="meter"
                    role="meter"
                    aria-valuemin={0}
                    aria-valuemax={embTotal}
                    aria-valuenow={emb.embedded}
                    aria-label="Embedded items"
                  >
                    <span style={{ width: `${embTotal ? (emb.embedded / embTotal) * 100 : 100}%` }} />
                  </div>
                  <div class="row small">
                    <span>
                      {compact(emb.embedded)} embedded, {compact(emb.pending)} pending
                    </span>
                    <span class="spacer" />
                    <button class="small" onClick={reindex}>
                      Re-embed all
                    </button>
                  </div>
                </div>
              )}
            </section>
          </div>
        </div>

        <section class="card">
          <h2>Projects</h2>
          {shownProjects.length === 0 ? (
            <div class="muted small">
              No projects yet. <a href="#/projects/new">Create one</a>
            </div>
          ) : (
            <div class="table-wrap">
              <table class="data">
                <thead>
                  <tr>
                    <th>Project</th>
                    <th class="num">Open</th>
                    <th class="num">New</th>
                    <th class="num">Bugs</th>
                    <th class="num">Feedback</th>
                    <th class="num">Spam</th>
                    <th>Last submission</th>
                  </tr>
                </thead>
                <tbody>
                  {shownProjects.map((p) => (
                    <tr key={p.id}>
                      <td>
                        <a href={feedbackLink({ projectId: p.id })}>{p.title}</a>
                        {!p.accepting && <span class="badge" style={{ marginLeft: '6px' }}>paused</span>}
                      </td>
                      <td class="num">{p.stats?.open ?? 0}</td>
                      <td class="num">{p.stats?.new ?? 0}</td>
                      <td class="num">{p.stats?.bugs ?? 0}</td>
                      <td class="num">{p.stats?.feedback ?? 0}</td>
                      <td class="num">{p.stats?.spam ?? 0}</td>
                      <td class="muted">{p.stats?.last_at ? relativeTime(p.stats.last_at) : 'never'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function Tile({ label, value, link }: { label: string; value: number | undefined; link?: string }) {
  const body = (
    <>
      <span class="label">{label}</span>
      <span class="value">{value === undefined ? '-' : compact(value)}</span>
    </>
  );
  return link ? (
    <a class="tile" href={link}>
      {body}
    </a>
  ) : (
    <div class="tile">{body}</div>
  );
}
