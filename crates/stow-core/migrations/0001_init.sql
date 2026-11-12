CREATE TABLE buckets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  emoji TEXT,
  description_md TEXT NOT NULL DEFAULT '',
  notes_md TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);

CREATE TABLE items (
  id TEXT PRIMARY KEY,
  bucket_id TEXT REFERENCES buckets(id) ON DELETE SET NULL,
  url TEXT NOT NULL,
  canonical_url TEXT NOT NULL,
  title TEXT,
  description TEXT,
  site_name TEXT,
  kind TEXT NOT NULL CHECK (kind IN ('video','article','other')),
  favicon_path TEXT,
  image_path TEXT,
  notes_md TEXT NOT NULL DEFAULT '',
  snapshot_status TEXT NOT NULL DEFAULT 'none' CHECK (snapshot_status IN ('none','pending','running','done','failed')),
  snapshot_error TEXT,
  snapshot_fetched_at TEXT,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX items_bucket_position ON items(bucket_id, position);
CREATE UNIQUE INDEX items_canonical_url ON items(canonical_url);

CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  parent_type TEXT NOT NULL CHECK (parent_type IN ('bucket','item')),
  parent_id TEXT NOT NULL,
  title TEXT NOT NULL,
  done_at TEXT,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX tasks_parent ON tasks(parent_type, parent_id, position);

CREATE TABLE reminders (
  id TEXT PRIMARY KEY,
  target_type TEXT NOT NULL CHECK (target_type IN ('bucket','item','task')),
  target_id TEXT NOT NULL,
  fire_at TEXT NOT NULL,
  message TEXT,
  fired_at TEXT,
  snoozed_until TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX reminders_fire_at ON reminders(fire_at);
CREATE INDEX reminders_target ON reminders(target_type, target_id);

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  parent_type TEXT NOT NULL CHECK (parent_type IN ('bucket','item')),
  parent_id TEXT NOT NULL,
  filename TEXT NOT NULL,
  path TEXT NOT NULL,
  mime TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX attachments_parent ON attachments(parent_type, parent_id);

CREATE TABLE recordings (
  id TEXT PRIMARY KEY,
  parent_type TEXT NOT NULL CHECK (parent_type IN ('bucket','item')),
  parent_id TEXT NOT NULL,
  audio_path TEXT NOT NULL,
  duration_ms INTEGER NOT NULL,
  transcript_md TEXT,
  transcript_status TEXT NOT NULL DEFAULT 'pending' CHECK (transcript_status IN ('none','pending','running','done','failed')),
  transcript_error TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX recordings_parent ON recordings(parent_type, parent_id);

CREATE TABLE ai_runs (
  id TEXT PRIMARY KEY,
  parent_type TEXT NOT NULL CHECK (parent_type IN ('bucket','item')),
  parent_id TEXT NOT NULL,
  action TEXT NOT NULL CHECK (action IN ('summarize_page','summarize_notes','summarize_recording','suggest_tasks','extract_metadata')),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  output_md TEXT,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('none','pending','running','done','failed')),
  error TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX ai_runs_parent ON ai_runs(parent_type, parent_id);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE VIRTUAL TABLE search USING fts5(
  doc_type UNINDEXED,
  doc_id UNINDEXED,
  item_id UNINDEXED,
  bucket_id UNINDEXED,
  title,
  body,
  tokenize = 'porter unicode61'
);
