import { useEffect, useState } from 'preact/hooks';
import { api, ApiError } from '../api';
import { navigate } from '../router';
import { toast } from '../toast';
import { PROJECT_TYPES, type ProjectType } from '../types';
import { slugify, validSlug } from '../util';

/** Create (no id) or edit (with id) a project. */
export function ProjectForm({ id }: { id?: number }) {
  const [title, setTitle] = useState('');
  const [slug, setSlug] = useState('');
  const [slugTouched, setSlugTouched] = useState(false);
  const [type, setType] = useState<ProjectType>('game');
  const [description, setDescription] = useState('');
  const [accepting, setAccepting] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(id === undefined);

  useEffect(() => {
    if (id === undefined) return;
    api
      .project(id)
      .then((p) => {
        setTitle(p.title);
        setSlug(p.slug);
        setSlugTouched(true);
        setType(p.project_type);
        setDescription(p.description);
        setAccepting(p.accepting);
        setLoaded(true);
      })
      .catch(() => setError('Project not found.'));
  }, [id]);

  const onTitle = (v: string) => {
    setTitle(v);
    if (!slugTouched) setSlug(slugify(v));
  };

  const submit = async (e: Event) => {
    e.preventDefault();
    setError(null);
    if (!title.trim()) return setError('Title is required.');
    if (slug && !validSlug(slug)) return setError('Slug may only contain lowercase letters, digits and hyphens.');
    setBusy(true);
    const input = {
      title: title.trim(),
      slug: slug || undefined,
      project_type: type,
      description,
      accepting,
    };
    try {
      const p = id === undefined ? await api.createProject(input) : await api.updateProject(id, input);
      toast(id === undefined ? 'Project created' : 'Project saved');
      navigate(`/projects/${p.id}`);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) setError('That slug is already taken.');
      else if (err instanceof ApiError && err.status === 400) setError(err.message);
      else setError('Could not save the project.');
    } finally {
      setBusy(false);
    }
  };

  if (!loaded) return <div class="empty">{error ?? 'Loading...'}</div>;

  return (
    <div>
      <div class="page-head">
        <h1>{id === undefined ? 'New project' : 'Edit project'}</h1>
      </div>
      <form class="card stack" onSubmit={submit} style={{ maxWidth: '720px' }}>
        <div class="form-grid">
          <label class="field full">
            Title
            <input type="text" name="title" value={title} onInput={(e) => onTitle(e.currentTarget.value)} autoFocus maxLength={200} />
          </label>
          <label class="field">
            Slug (used in the hosted page URL)
            <input
              type="text"
              name="slug"
              value={slug}
              maxLength={63}
              onInput={(e) => {
                setSlugTouched(true);
                setSlug(e.currentTarget.value);
              }}
            />
          </label>
          <label class="field">
            Type
            <select name="project_type" value={type} onChange={(e) => setType(e.currentTarget.value as ProjectType)}>
              {PROJECT_TYPES.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </label>
          <label class="field full">
            Description (shown on the hosted page)
            <textarea name="description" rows={3} value={description} onInput={(e) => setDescription(e.currentTarget.value)} />
          </label>
          <label class="check full">
            <input type="checkbox" name="accepting" checked={accepting} onChange={(e) => setAccepting(e.currentTarget.checked)} />
            Accepting submissions
          </label>
        </div>
        {error && (
          <div class="error" role="alert">
            {error}
          </div>
        )}
        <div class="row">
          <button class="primary" type="submit" disabled={busy}>
            {id === undefined ? 'Create project' : 'Save'}
          </button>
          <a class="btn" href={id === undefined ? '#/projects' : `#/projects/${id}`}>
            Cancel
          </a>
        </div>
      </form>
    </div>
  );
}
