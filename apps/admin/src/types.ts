export const PROJECT_TYPES = ['game', 'webapp', 'mobile', 'desktop', 'service', 'library', 'other'] as const;
export type ProjectType = (typeof PROJECT_TYPES)[number];

export const KINDS = ['bug', 'feedback'] as const;
export type Kind = (typeof KINDS)[number];

export const STATUSES = ['new', 'triaged', 'resolved', 'archived', 'spam'] as const;
export type Status = (typeof STATUSES)[number];

/** Status filter accepted by `GET /api/admin/feedback`. */
export type StatusFilter = 'open' | 'all' | 'any' | Status;

export const MODES = ['hybrid', 'fuzzy', 'keyword', 'semantic'] as const;
export type Mode = (typeof MODES)[number];

export interface ProjectStats {
  total: number;
  open: number;
  new: number;
  bugs: number;
  feedback: number;
  spam: number;
  last_at: number | null;
}

export interface Project {
  id: number;
  slug: string;
  title: string;
  project_type: ProjectType;
  description: string;
  ingest_key: string;
  accepting: boolean;
  created_at: number;
  updated_at: number;
  stats?: ProjectStats;
}

export interface ProjectInput {
  title?: string;
  slug?: string;
  project_type?: ProjectType;
  description?: string;
  accepting?: boolean;
}

export const LABEL_COLORS = ['slate', 'blue', 'green', 'amber', 'red', 'violet', 'pink'] as const;
export type LabelColor = (typeof LABEL_COLORS)[number];
export interface Label {
  id: number;
  project_id: number;
  name: string;
  color: LabelColor;
  description: string;
  created_at: number;
  updated_at: number;
}
export interface LabelSummary extends Label {
  feedback_count: number;
  open_count: number;
}
export interface LabelInput { name: string; color: LabelColor; description: string }
export type LabelAction = 'add' | 'remove' | 'set';

export interface Feedback {
  labels: Label[];
  id: number;
  project_id: number;
  kind: Kind;
  title: string;
  body: string;
  contact: string | null;
  rating: number | null;
  meta: unknown;
  source: string;
  status: Status;
  spam_reason: string | null;
  spam_score: number;
  admin_note: string;
  dup_count: number;
  created_at: number;
  updated_at: number;
  last_seen_at: number;
  score?: number;
}

export interface FeedbackList {
  items: Feedback[];
  total: number;
  mode: Mode;
  semantic: boolean;
}

export interface FeedbackDetail {
  item: Feedback;
  similar: Feedback[];
}

export interface FeedbackPatch {
  status?: Status;
  admin_note?: string;
  kind?: Kind;
}

export type BulkAction = { action: 'set_status'; status: Status } | { action: 'delete' };

export interface DailyPoint {
  day: string;
  bug: number;
  feedback: number;
}

export interface Stats {
  totals: {
    total: number;
    open: number;
    new: number;
    bugs: number;
    feedback: number;
    spam: number;
    last_7d: number;
  };
  daily: DailyPoint[];
  blocked: Record<string, number>;
  embeddings: { enabled: boolean; model: string; embedded: number; pending: number };
}

export const TOKEN_SCOPES = ['read', 'write'] as const;
export type TokenScope = (typeof TOKEN_SCOPES)[number];

export interface ApiToken {
  id: number;
  name: string;
  scope: TokenScope;
  prefix: string;
  created_at: number;
  last_used_at: number | null;
}

/** Returned once by `POST /api/admin/tokens`; `token` is never shown again. */
export interface CreatedToken extends ApiToken {
  token: string;
}
