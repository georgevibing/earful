import type {
  ApiToken,
  LabelSummary, LabelInput, LabelAction,
  BulkAction,
  CreatedToken,
  Feedback,
  FeedbackDetail,
  FeedbackList,
  FeedbackPatch,
  Project,
  ProjectInput,
  Stats,
  TokenScope,
} from './types';

/** Error thrown for any non-2xx response. `code` is the server's stable error code. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly retryAfter: number | null = null,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

let onUnauthorized: () => void = () => {};

/** Registers the handler invoked whenever the server answers 401 (session missing or expired). */
export function setUnauthorizedHandler(fn: () => void): void {
  onUnauthorized = fn;
}

type Query = Record<string, string | number | boolean | undefined | null>;

export function buildQuery(params: Query): string {
  const sp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') sp.set(k, String(v));
  }
  const s = sp.toString();
  return s ? `?${s}` : '';
}

async function request<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const init: RequestInit = {
    method,
    credentials: 'same-origin',
    headers: { Accept: 'application/json' },
    signal,
  };
  if (body !== undefined) {
    init.headers = { ...init.headers, 'Content-Type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const res = await fetch(path, init);
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  if (!res.ok) {
    const err = (data ?? {}) as { error?: string; message?: string };
    const retry = res.headers.get('Retry-After');
    const apiErr = new ApiError(
      res.status,
      err.error ?? `http_${res.status}`,
      err.message ?? res.statusText ?? 'request failed',
      retry ? Number(retry) : null,
    );
    if (res.status === 401 && !path.endsWith('/login')) onUnauthorized();
    throw apiErr;
  }
  return data as T;
}

export interface FeedbackQuery {
  project_id?: number;
  kind?: string;
  status?: string;
  q?: string;
  mode?: string;
  from?: number;
  to?: number;
  limit?: number;
  offset?: number;
  label_ids?: string;
  label_match?: 'all' | 'any';
  unlabeled?: boolean;
}

export const api = {
  login: (password: string) => request<void>('POST', '/api/admin/login', { password }),
  logout: () => request<void>('POST', '/api/admin/logout'),
  me: () => request<{ ok: boolean }>('GET', '/api/admin/me'),

  projects: () => request<Project[]>('GET', '/api/admin/projects'),
  project: (id: number) => request<Project>('GET', `/api/admin/projects/${id}`),
  createProject: (input: ProjectInput) => request<Project>('POST', '/api/admin/projects', input),
  updateProject: (id: number, input: ProjectInput) => request<Project>('PATCH', `/api/admin/projects/${id}`, input),
  deleteProject: (id: number) => request<void>('DELETE', `/api/admin/projects/${id}`),
  rotateKey: (id: number) => request<Project>('POST', `/api/admin/projects/${id}/rotate-key`),
  exportUrl: (id: number, format: 'json' | 'csv') => `/api/admin/projects/${id}/export?format=${format}`,

  feedback: (q: FeedbackQuery, signal?: AbortSignal) =>
    request<FeedbackList>('GET', `/api/admin/feedback${buildQuery({ ...q })}`, undefined, signal),
  feedbackItem: (id: number) => request<FeedbackDetail>('GET', `/api/admin/feedback/${id}`),
  updateFeedback: (id: number, patch: FeedbackPatch) => request<Feedback>('PATCH', `/api/admin/feedback/${id}`, patch),
  deleteFeedback: (id: number) => request<void>('DELETE', `/api/admin/feedback/${id}`),
  bulk: (ids: number[], action: BulkAction) =>
    request<{ affected: number }>('POST', '/api/admin/feedback/bulk', { ids, ...action }),

  labels: (projectId?: number) => request<LabelSummary[]>('GET', `/api/admin/labels${buildQuery({ project_id: projectId })}`),
  createLabel: (projectId: number, input: LabelInput) => request<LabelSummary>('POST', '/api/admin/labels', { ...input, project_id: projectId }),
  updateLabel: (id: number, input: Partial<LabelInput>) => request<LabelSummary>('PATCH', `/api/admin/labels/${id}`, input),
  deleteLabel: (id: number) => request<void>('DELETE', `/api/admin/labels/${id}`),
  assignLabels: (ids: number[], labelIds: number[], action: LabelAction) => request<{ affected: number }>('POST', '/api/admin/feedback/labels', { ids, label_ids: labelIds, action }),

  stats: (projectId?: number) => request<Stats>('GET', `/api/admin/stats${buildQuery({ project_id: projectId })}`),
  reindex: () => request<{ ok: boolean }>('POST', '/api/admin/reindex'),

  tokens: () => request<ApiToken[]>('GET', '/api/admin/tokens'),
  createToken: (name: string, scope: TokenScope) => request<CreatedToken>('POST', '/api/admin/tokens', { name, scope }),
  revokeToken: (id: number) => request<void>('DELETE', `/api/admin/tokens/${id}`),
};
