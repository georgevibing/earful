import { useEffect, useState } from 'preact/hooks';

export type Route =
  | { name: 'dashboard'; params: URLSearchParams }
  | { name: 'projects'; params: URLSearchParams }
  | { name: 'project-new'; params: URLSearchParams }
  | { name: 'project'; id: number; params: URLSearchParams }
  | { name: 'project-edit'; id: number; params: URLSearchParams }
  | { name: 'feedback'; params: URLSearchParams }
  | { name: 'tokens'; params: URLSearchParams }
  | { name: 'not-found'; params: URLSearchParams };

/** Parses a location hash like `#/projects/3?x=1` into a route. */
export function parseHash(hash: string): Route {
  const raw = hash.replace(/^#/, '') || '/';
  const qIdx = raw.indexOf('?');
  const path = (qIdx >= 0 ? raw.slice(0, qIdx) : raw).replace(/\/+$/, '') || '/';
  const params = new URLSearchParams(qIdx >= 0 ? raw.slice(qIdx + 1) : '');
  const parts = path.split('/').filter(Boolean);

  if (parts.length === 0) return { name: 'dashboard', params };
  if (parts[0] === 'feedback' && parts.length === 1) return { name: 'feedback', params };
  if (parts[0] === 'tokens' && parts.length === 1) return { name: 'tokens', params };
  if (parts[0] === 'projects') {
    if (parts.length === 1) return { name: 'projects', params };
    if (parts[1] === 'new' && parts.length === 2) return { name: 'project-new', params };
    const id = Number(parts[1]);
    if (Number.isInteger(id) && id > 0) {
      if (parts.length === 2) return { name: 'project', id, params };
      if (parts.length === 3 && parts[2] === 'edit') return { name: 'project-edit', id, params };
    }
  }
  return { name: 'not-found', params };
}

export function href(path: string, params?: URLSearchParams | Record<string, string>): string {
  const sp = params instanceof URLSearchParams ? params : new URLSearchParams(params);
  const qs = sp.toString();
  return `#${path}${qs ? `?${qs}` : ''}`;
}

export function navigate(to: string, replace = false): void {
  const target = to.startsWith('#') ? to : `#${to}`;
  if (replace) {
    history.replaceState(null, '', target);
    window.dispatchEvent(new HashChangeEvent('hashchange'));
  } else {
    location.hash = target;
  }
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseHash(location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseHash(location.hash));
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return route;
}
