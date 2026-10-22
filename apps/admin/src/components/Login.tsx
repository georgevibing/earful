import { useState } from 'preact/hooks';
import { api, ApiError } from '../api';

export function Login({ onLoggedIn }: { onLoggedIn: () => void }) {
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!password || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.login(password);
      onLoggedIn();
    } catch (err) {
      if (err instanceof ApiError && err.status === 429) {
        const wait = err.retryAfter ? ` Try again in ${Math.ceil(err.retryAfter / 60)} min.` : '';
        setError(`Too many attempts.${wait}`);
      } else if (err instanceof ApiError && err.status === 401) {
        setError('Wrong password.');
      } else {
        setError('Login failed, is the server reachable?');
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="login">
      <form class="card stack" onSubmit={submit}>
        <h1>feedback-harvest</h1>
        <label class="field">
          Admin password
          <input
            type="password"
            name="password"
            autoComplete="current-password"
            autoFocus
            value={password}
            onInput={(e) => setPassword(e.currentTarget.value)}
          />
        </label>
        {error && (
          <div class="error" role="alert">
            {error}
          </div>
        )}
        <button class="primary" type="submit" disabled={busy || !password}>
          {busy ? 'Signing in...' : 'Sign in'}
        </button>
      </form>
    </div>
  );
}
