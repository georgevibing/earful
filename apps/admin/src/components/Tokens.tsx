import { useEffect, useState } from 'preact/hooks';
import { api, ApiError } from '../api';
import { mcpSnippets } from '../snippets';
import { toast } from '../toast';
import type { ApiToken, CreatedToken, TokenScope } from '../types';
import { copyText, formatDateTime, relativeTime } from '../util';

export function Tokens() {
  const [tokens, setTokens] = useState<ApiToken[] | null>(null);
  const [name, setName] = useState('');
  const [scope, setScope] = useState<TokenScope>('read');
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    api
      .tokens()
      .then(setTokens)
      .catch(() => {
        setTokens([]);
        toast('Could not load tokens');
      });

  useEffect(() => {
    void load();
  }, []);

  const copy = async (text: string, what: string) => toast((await copyText(text)) ? `${what} copied` : 'Copy failed');

  const create = async (e: Event) => {
    e.preventDefault();
    setError(null);
    if (!name.trim()) return setError('Give the token a name so you know where it is used.');
    setBusy(true);
    try {
      const t = await api.createToken(name.trim(), scope);
      setCreated(t);
      setName('');
      await load();
    } catch (err) {
      setError(err instanceof ApiError && err.status === 400 ? err.message : 'Could not create the token.');
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (t: ApiToken) => {
    if (!confirm(`Revoke "${t.name}"? Clients using it stop working immediately.`)) return;
    try {
      await api.revokeToken(t.id);
      if (created?.id === t.id) setCreated(null);
      toast('Token revoked');
      await load();
    } catch {
      toast('Revoke failed');
    }
  };

  return (
    <div class="stack">
      <div class="page-head" style={{ marginBottom: 0 }}>
        <h1>API tokens</h1>
      </div>
      <p class="muted small" style={{ margin: 0 }}>
        Bearer tokens for the MCP endpoint and scripts. Read tokens can list, search, view stats and export. Write
        tokens can also change and delete things.
      </p>

      <form class="card stack" onSubmit={create}>
        <h2>Create a token</h2>
        <div class="row">
          <label class="field" style={{ flex: '1 1 240px' }}>
            Name
            <input
              type="text"
              name="name"
              placeholder="claude-code laptop"
              maxLength={100}
              value={name}
              onInput={(e) => setName(e.currentTarget.value)}
            />
          </label>
          <label class="field">
            Scope
            <select name="scope" value={scope} onChange={(e) => setScope(e.currentTarget.value as TokenScope)}>
              <option value="read">read</option>
              <option value="write">write</option>
            </select>
          </label>
          <button class="primary" type="submit" disabled={busy} style={{ alignSelf: 'end' }}>
            Create token
          </button>
        </div>
        {error && (
          <div class="error" role="alert">
            {error}
          </div>
        )}
        {created && (
          <div class="stack" style={{ gap: '6px' }} data-testid="new-token">
            <strong>
              Token "{created.name}" ({created.scope}) created. Copy it now: you will not see this again.
            </strong>
            <div class="key-box">
              <code>{created.token}</code>
              <button type="button" class="small" onClick={() => copy(created.token, 'Token')}>
                Copy
              </button>
              <button type="button" class="small ghost" onClick={() => setCreated(null)}>
                Done
              </button>
            </div>
          </div>
        )}
      </form>

      <section class="card">
        <h2>Tokens</h2>
        {tokens === null ? (
          <div class="empty">Loading...</div>
        ) : tokens.length === 0 ? (
          <div class="muted small">No tokens yet.</div>
        ) : (
          <div class="table-wrap">
            <table class="data">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Scope</th>
                  <th>Prefix</th>
                  <th>Created</th>
                  <th>Last used</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {tokens.map((t) => (
                  <tr key={t.id}>
                    <td>{t.name}</td>
                    <td>
                      <span class={`badge${t.scope === 'write' ? ' write' : ''}`}>{t.scope}</span>
                    </td>
                    <td class="mono">{t.prefix}...</td>
                    <td title={formatDateTime(t.created_at)}>{relativeTime(t.created_at)}</td>
                    <td class="muted">{t.last_used_at ? relativeTime(t.last_used_at) : 'never'}</td>
                    <td style={{ textAlign: 'right' }}>
                      <button class="small danger" onClick={() => revoke(t)} aria-label={`Revoke ${t.name}`}>
                        Revoke
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section class="card">
        <h2>Connect an MCP client</h2>
        <p class="muted small" style={{ marginTop: 0 }}>
          {created
            ? 'These snippets include the token you just created.'
            : 'Replace <TOKEN> with a token created above.'}
        </p>
        {mcpSnippets(location.origin, created?.token).map((sn) => (
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
    </div>
  );
}
