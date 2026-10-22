import type { ComponentChildren } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { api, ApiError, setUnauthorizedHandler } from './api';
import { Dashboard } from './components/Dashboard';
import { Inbox } from './components/Inbox';
import { Login } from './components/Login';
import { ProjectDetail } from './components/ProjectDetail';
import { ProjectForm } from './components/ProjectForm';
import { Projects } from './components/Projects';
import { Tokens } from './components/Tokens';
import { useRoute, type Route } from './router';
import { Toaster, toast } from './toast';
import { BRAND, documentTitle, MARK_SMALL_URL } from './brand';

type AuthState = 'checking' | 'in' | 'out';

export function App() {
  const [auth, setAuth] = useState<AuthState>('checking');
  const route = useRoute();

  useEffect(() => {
    setUnauthorizedHandler(() => setAuth('out'));
    api
      .me()
      .then(() => setAuth('in'))
      .catch((e: unknown) => {
        setAuth('out');
        if (!(e instanceof ApiError) || e.status !== 401) toast('Could not reach the server');
      });
  }, []);

  useEffect(() => {
    if (auth !== 'checking') document.title = documentTitle(auth === 'out' ? 'login' : route.name);
  }, [auth, route.name]);

  if (auth === 'checking') return <div class="login muted">Loading...</div>;
  if (auth === 'out')
    return (
      <>
        <Login onLoggedIn={() => setAuth('in')} />
        <Toaster />
      </>
    );

  const logout = async () => {
    try {
      await api.logout();
    } finally {
      setAuth('out');
    }
  };

  return (
    <div class="shell">
      <header class="topbar">
        <a class="brand" href="#/">
          <img src={MARK_SMALL_URL} alt="" width={24} height={24} />
          {BRAND}
        </a>
        <nav aria-label="Main navigation">
          <NavLink to="#/" active={route.name === 'dashboard'}>
            Dashboard
          </NavLink>
          <NavLink to="#/feedback" active={route.name === 'feedback'}>
            Feedback
          </NavLink>
          <NavLink to="#/projects" active={route.name.startsWith('project')}>
            Projects
          </NavLink>
          <NavLink to="#/tokens" active={route.name === 'tokens'}>
            API tokens
          </NavLink>
        </nav>
        <span class="spacer" />
        <button class="small ghost" onClick={logout}>
          Log out
        </button>
      </header>
      <main class="page">
        <Page route={route} />
      </main>
      <Toaster />
    </div>
  );
}

function NavLink(props: { to: string; active: boolean; children: ComponentChildren }) {
  return (
    <a href={props.to} class={props.active ? 'active' : undefined} aria-current={props.active ? 'page' : undefined}>
      {props.children}
    </a>
  );
}

function Page({ route }: { route: Route }) {
  switch (route.name) {
    case 'dashboard':
      return <Dashboard />;
    case 'feedback':
      return <Inbox params={route.params} />;
    case 'projects':
      return <Projects />;
    case 'tokens':
      return <Tokens />;
    case 'project-new':
      return <ProjectForm />;
    case 'project':
      return <ProjectDetail id={route.id} key={route.id} />;
    case 'project-edit':
      return <ProjectForm id={route.id} key={route.id} />;
    default:
      return (
        <div class="empty">
          Page not found. <a href="#/">Go to the dashboard</a>
        </div>
      );
  }
}
