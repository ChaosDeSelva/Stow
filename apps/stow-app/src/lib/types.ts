// TS mirrors of crates/stow-core/src/models.rs plus the extra types from
// docs/superpowers/plans/tauri-contract.md. Returned structs keep snake_case.

export type ItemKind = "video" | "article" | "other";
export type JobStatus = "none" | "pending" | "running" | "done" | "failed";
export type ParentType = "bucket" | "item";
export type TargetType = "bucket" | "item" | "task";
export type AiAction =
  | "summarize_page"
  | "summarize_notes"
  | "summarize_recording"
  | "suggest_tasks"
  | "extract_metadata";
export type AiProvider = "anthropic" | "openai";
export type WhisperModel = "tiny.en" | "base.en" | "small.en";

export interface Bucket {
  id: string;
  name: string;
  emoji: string | null;
  description_md: string;
  notes_md: string;
  position: number;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
}

export interface Item {
  id: string;
  bucket_id: string | null;
  url: string;
  canonical_url: string;
  title: string | null;
  description: string | null;
  site_name: string | null;
  kind: ItemKind;
  favicon_path: string | null;
  image_path: string | null;
  notes_md: string;
  snapshot_status: JobStatus;
  snapshot_error: string | null;
  snapshot_fetched_at: string | null;
  /** Watched/read; null = still to do. Older backends omit it. */
  done_at?: string | null;
  position: number;
  created_at: string;
  updated_at: string;
}

export interface Task {
  id: string;
  parent_type: ParentType;
  parent_id: string;
  title: string;
  done_at: string | null;
  position: number;
  created_at: string;
}

export interface Reminder {
  id: string;
  target_type: TargetType;
  target_id: string;
  fire_at: string;
  message: string | null;
  fired_at: string | null;
  snoozed_until: string | null;
  created_at: string;
}

export interface Attachment {
  id: string;
  parent_type: ParentType;
  parent_id: string;
  filename: string;
  path: string;
  mime: string;
  size_bytes: number;
  created_at: string;
}

export interface Recording {
  id: string;
  parent_type: ParentType;
  parent_id: string;
  audio_path: string;
  duration_ms: number;
  transcript_md: string | null;
  transcript_status: JobStatus;
  transcript_error: string | null;
  created_at: string;
}

export interface AiRun {
  id: string;
  parent_type: ParentType;
  parent_id: string;
  action: AiAction;
  provider: string;
  model: string;
  input_hash: string;
  output_md: string | null;
  status: JobStatus;
  error: string | null;
  created_at: string;
}

export interface SearchHit {
  doc_type: string;
  doc_id: string;
  item_id: string | null;
  bucket_id: string | null;
  title: string;
  snippet: string;
}

export type ItemFilter = { kind: "inbox" } | { kind: "bucket"; id: string } | { kind: "all" };

export interface BucketPatch {
  name?: string;
  emoji?: string | null;
  description_md?: string;
  notes_md?: string;
}

export type ThemeSetting = "light" | "dark" | "system";

export interface Settings {
  theme: ThemeSetting;
  close_tab_after_capture: boolean;
  hotkey: string;
  ai_provider: AiProvider;
  ai_model: string;
  whisper_model: WhisperModel;
  data_dir: string;
  extension_port: number;
  extension_token: string;
  /** Pins the local server's CORS to one Chrome extension id (32 letters a–p); "" = any extension. */
  extension_id?: string;
  /** Fetch readable copies of pages on private/local addresses. Takes effect after a restart. */
  allow_local_urls?: boolean;
  // v0.1 hardening fields (older backends omit them; treat missing as defaults).
  hotkey_registered?: boolean;
  hotkey_error?: string | null;
  onboarding_dismissed?: boolean;
  onboarding_steps?: Partial<Record<OnboardingStep, boolean>>;
  close_to_tray_explained?: boolean;
  quit_on_close?: boolean;
  open_at_login?: boolean;
  ai_consent_given?: boolean;
}

/** What the capture window may read: `capture_settings()` / `set_close_tab_after_capture()`. */
export interface CaptureSettings {
  close_tab_after_capture: boolean;
  hotkey: string;
  theme: ThemeSetting;
}

export type OnboardingStep = "captured" | "automation" | "notifications" | "extension" | "ai_key" | "whisper";

export interface AddItemResult {
  item: Item;
  created: boolean;
}
export interface CloseTabResult {
  closed: boolean;
}
export type AutomationStatus = "granted" | "denied" | "not_determined" | "chrome_not_running";
export type MicrophoneStatus = "granted" | "denied" | "not_determined" | "restricted";
export interface PermissionStatus {
  automation: AutomationStatus;
  microphone: MicrophoneStatus;
}
export type PrivacyPane = "automation" | "microphone" | "notifications";
export interface ExportResult {
  path: string;
  buckets: number;
  items: number;
}
export interface BackupResult {
  path: string;
  bytes: number;
}
/** `delete_whisper_models` */
export interface DeletedModels {
  deleted_bytes: number;
}

export interface ChromeTab {
  window_index: number;
  tab_index: number;
  url: string;
  title: string;
}

export interface ImportResult {
  imported: number;
  skipped: number;
}

export interface ReminderAgenda {
  overdue: Reminder[];
  today: Reminder[];
  upcoming: Reminder[];
}

export interface AppInfo {
  name: string;
  version: string;
  data_dir: string;
  /** Bundled Chrome extension folder (null when the backend can't locate it). */
  extension_dir?: string | null;
  /** How the binary was built (Settings → About); older backends omit it. */
  build?: BuildInfo;
}
export interface BuildInfo {
  version: string;
  /** `STOW_COMMIT` at build time; null for local builds. */
  commit: string | null;
  /** RFC 3339 UTC. */
  built_at: string;
}

// Event payloads
export interface IdPayload {
  id: string;
}
export interface ModelDownloadProgress {
  /** What is downloading, e.g. "whisper_model". */
  kind?: string;
  received: number;
  total: number | null;
}
export interface ReminderFiredPayload {
  reminder_id: string;
  target_type: TargetType;
  target_id: string;
  title: string;
}
export interface NavigatePayload {
  target_type: TargetType;
  target_id: string;
}

export interface PairRequestPayload {
  id: string;
  name: string;
}
export interface RecordingStatePayload {
  recording: boolean;
}
export interface HotkeyStatusPayload {
  hotkey: string;
  registered: boolean;
  error: string | null;
}

export interface EventPayloads {
  "item-added": IdPayload;
  "item-updated": IdPayload;
  "app-will-quit": Record<string, never>;
  "buckets-changed": Record<string, never>;
  "recording-updated": IdPayload;
  "ai-run-updated": IdPayload;
  "model-download-progress": ModelDownloadProgress;
  "reminder-fired": ReminderFiredPayload;
  navigate: NavigatePayload;
  "capture-open": Record<string, never>;
  "pair-request": PairRequestPayload;
  "hotkey-status": HotkeyStatusPayload;
  /** Tray "Settings…" → the main window opens its Settings view. */
  "open-settings": Record<string, never>;
  /** Recording started or stopped by any path (command or tray Stop). */
  "recording-state": RecordingStatePayload;
}
export type EventName = keyof EventPayloads;
