import { useEffect, useState } from 'preact/hooks';
import { api } from '../api';
import { toast } from '../toast';
import type { Project } from '../types';
import { relativeTime } from '../util';

export function Projects() {
  const [projects, setProjects] = useState<Project[] | null>(null);

  useEffect(() => {
    api
      .projects()
      .then(setProjects)
      .catch(() => {
        setProjects([]);
        toast('Could not load projects');
      });
  }, []);

  return (
    <div>
      <div class="page-head">
        <h1>Projects</h1>
        <span class="spacer" />
        <a class="btn primary" href="#/projects/new">
          New project
        </a>
      </div>
      <div class="card">
        {projects === null ? (
          <div class="empty">Loading...</div>
        ) : projects.length === 0 ? (
          <div class="empty">
            No projects yet. <a href="#/projects/new">Create your first one</a> to get an ingest key.
          </div>
        ) : (
          <div class="table-wrap">
            <table class="data">
              <thead>
                <tr>
                  <th>Title</th>
                  <th>Type</th>
                  <th>Slug</th>
                  <th>Status</th>
                  <th class="num">Open</th>
                  <th class="num">Total</th>
                  <th class="num">Spam</th>
                  <th>Last submission</th>
                </tr>
              </thead>
              <tbody>
                {projects.map((p) => (
                  <tr key={p.id}>
                    <td>
                      <a href={`#/projects/${p.id}`}>{p.title}</a>
                    </td>
                    <td>{p.project_type}</td>
                    <td class="mono">{p.slug}</td>
                    <td>{p.accepting ? <span class="badge">accepting</span> : <span class="badge">paused</span>}</td>
                    <td class="num">{p.stats?.open ?? 0}</td>
                    <td class="num">{p.stats?.total ?? 0}</td>
                    <td class="num">{p.stats?.spam ?? 0}</td>
                    <td class="muted">{p.stats?.last_at ? relativeTime(p.stats.last_at) : 'never'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
