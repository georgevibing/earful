import type { Route } from './router';

export const BRAND = 'Feedback Harvest';

/** Public URL of the logo mark (served from `public/`, respects Vite's base). */
export const LOGO_URL = `${import.meta.env.BASE_URL}logo.svg`;
/** Bolder cut of the mark for small sizes (navbar, favicon). */
export const MARK_SMALL_URL = `${import.meta.env.BASE_URL}favicon.svg`;

const PAGE_TITLES: Record<Route['name'], string> = {
  dashboard: 'Dashboard',
  feedback: 'Inbox',
  projects: 'Projects',
  'project-new': 'New project',
  project: 'Project',
  'project-edit': 'Edit project',
  tokens: 'API tokens',
  'not-found': 'Not found',
};

export function documentTitle(page: Route['name'] | 'login'): string {
  const label = page === 'login' ? 'Sign in' : PAGE_TITLES[page];
  return `${label} · ${BRAND}`;
}
