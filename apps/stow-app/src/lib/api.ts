// Thin typed wrappers over Tauri `invoke`, one per command in tauri-contract.md.
// Argument keys are camelCase (Tauri converts to the Rust snake_case params).
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AiAction,
  AiProvider,
  AddItemResult,
  AiRun,
  AppInfo,
  AutomationStatus,
  BackupResult,
  CaptureSettings,
  CloseTabResult,
  DeletedModels,
  ExportResult,
  PermissionStatus,
  PrivacyPane,
  Attachment,
  Bucket,
  BucketPatch,
  ChromeTab,
  EventName,
  EventPayloads,
  ImportResult,
  Item,
  ItemFilter,
  MicrophoneStatus,
  ParentType,
  Recording,
  Reminder,
  ReminderAgenda,
  SearchHit,
  Settings,
  TargetType,
  Task,
} from "./types";

// Every command call goes through here so that a synchronous throw (no Tauri runtime,
/// a bad argument) behaves exactly like a rejected promise: callers' `.catch`/`try` paths
/// run asynchronously and can never form a synchronous effect loop.
function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return args === undefined ? tauriInvoke<T>(cmd) : tauriInvoke<T>(cmd, args);
  } catch (e) {
    return Promise.reject(e);
  }
}

export const appInfo = () => invoke<AppInfo>("app_info");
/** Opens an attachment with the default app; the backend checks the path is inside the data folder. */
export const openAttachment = (id: string) => invoke<void>("open_attachment", { id });
/** Reveals the data folder in Finder (works with a custom STOW_DATA_DIR). */
export const revealDataDir = () => invoke<void>("reveal_data_dir");

// Buckets
export const listBuckets = () => invoke<Bucket[]>("list_buckets");
export const createBucket = (name: string, emoji?: string | null) =>
  invoke<Bucket>("create_bucket", emoji == null ? { name } : { name, emoji });
export const updateBucket = (id: string, patch: BucketPatch) =>
  invoke<Bucket>("update_bucket", { id, patch });
export const deleteBucket = (id: string) => invoke<void>("delete_bucket", { id });
export const reorderBuckets = (ids: string[]) => invoke<void>("reorder_buckets", { ids });

// Items
export const listItems = (filter: ItemFilter, includeDone = false) =>
  invoke<Item[]>("list_items", includeDone ? { filter, includeDone } : { filter });
/** Item count for a filter. No count command exists yet, so this is the list length. */
export const countItems = async (filter: ItemFilter) => (await listItems(filter)).length;
export const setItemDone = (id: string, done: boolean) => invoke<Item>("set_item_done", { id, done });
export const getItem = (id: string) => invoke<Item>("get_item", { id });
export const addItem = (url: string, bucketId?: string | null, title?: string | null) => {
  const args: Record<string, unknown> = { url };
  if (bucketId != null) args.bucketId = bucketId;
  if (title != null && title !== "") args.title = title;
  // Older backends return the bare Item; normalise to `{item, created}`.
  return invoke<AddItemResult | Item>("add_item", args).then((r) =>
    r && typeof r === "object" && "item" in r && "created" in r ? r : { item: r as Item, created: true },
  );
};
export const moveItem = (id: string, bucketId: string | null) =>
  invoke<Item>("move_item", { id, bucketId });
export const updateItemNotes = (id: string, notesMd: string) =>
  invoke<Item>("update_item_notes", { id, notesMd });
export const deleteItem = (id: string) => invoke<void>("delete_item", { id });
export const refetchItem = (id: string) => invoke<void>("refetch_item", { id });
export const readSnapshot = (id: string) => invoke<string | null>("read_snapshot", { id });

// Tasks
export const listTasks = (parentType: ParentType, parentId: string) =>
  invoke<Task[]>("list_tasks", { parentType, parentId });
export const addTask = (parentType: ParentType, parentId: string, title: string) =>
  invoke<Task>("add_task", { parentType, parentId, title });
export const toggleTask = (id: string) => invoke<Task>("toggle_task", { id });
export const renameTask = (id: string, title: string) => invoke<Task>("rename_task", { id, title });
export const deleteTask = (id: string) => invoke<void>("delete_task", { id });

// Reminders
export const listReminders = (targetType: TargetType, targetId: string) =>
  invoke<Reminder[]>("list_reminders", { targetType, targetId });
export const addReminder = (
  targetType: TargetType,
  targetId: string,
  fireAt: string,
  message?: string | null,
) => {
  const args: Record<string, unknown> = { targetType, targetId, fireAt };
  if (message != null && message !== "") args.message = message;
  return invoke<Reminder>("add_reminder", args);
};
export const snoozeReminder = (id: string, until: string) =>
  invoke<Reminder>("snooze_reminder", { id, until });
export const deleteReminder = (id: string) => invoke<void>("delete_reminder", { id });
export const reminderAgenda = (start: string, end: string) =>
  invoke<ReminderAgenda>("reminder_agenda", { start, end });

// Attachments
export const listAttachments = (parentType: ParentType, parentId: string) =>
  invoke<Attachment[]>("list_attachments", { parentType, parentId });
export const addAttachment = (parentType: ParentType, parentId: string, srcPath: string) =>
  invoke<Attachment>("add_attachment", { parentType, parentId, srcPath });
export const deleteAttachment = (id: string) => invoke<void>("delete_attachment", { id });

// Recordings
export const listRecordings = (parentType: ParentType, parentId: string) =>
  invoke<Recording[]>("list_recordings", { parentType, parentId });
export const isRecording = () => invoke<boolean>("is_recording");
export const startRecording = (parentType: ParentType, parentId: string) =>
  invoke<void>("start_recording", { parentType, parentId });
export const stopRecording = (parentType: ParentType, parentId: string) =>
  invoke<Recording>("stop_recording", { parentType, parentId });
export const deleteRecording = (id: string) => invoke<void>("delete_recording", { id });
export const retranscribe = (id: string) => invoke<void>("retranscribe", { id });

// AI
export const listAiRuns = (parentType: ParentType, parentId: string) =>
  invoke<AiRun[]>("list_ai_runs", { parentType, parentId });
export const runAi = (parentType: ParentType, parentId: string, action: AiAction) =>
  invoke<AiRun>("run_ai", { parentType, parentId, action });
export const setApiKey = (account: AiProvider, key: string) =>
  invoke<void>("set_api_key", { account, key });
export const hasApiKey = (account: AiProvider) => invoke<boolean>("has_api_key", { account });
export const deleteApiKey = (account: AiProvider) => invoke<void>("delete_api_key", { account });

// Settings
export const getSettings = () => invoke<Settings>("get_settings");
export const updateSettings = (patch: Partial<Settings>) =>
  invoke<Settings>("update_settings", { patch });
/** The capture window's view of settings (get_settings/update_settings are main-only). */
export const captureSettings = () => invoke<CaptureSettings>("capture_settings");
export const setCloseTabAfterCapture = (value: boolean) =>
  invoke<CaptureSettings>("set_close_tab_after_capture", { value });

// Search
export const search = (q: string) => invoke<SearchHit[]>("search", { q });

// Chrome
export const captureFrontTab = () => invoke<ChromeTab>("capture_front_tab");
/** Closes Chrome's front tab only if it still shows `expectedUrl` (the URL capture_front_tab returned). */
export const closeFrontTab = (expectedUrl?: string) =>
  invoke<CloseTabResult | null>("close_front_tab", expectedUrl ? { expectedUrl } : {}).then((r) =>
    r && typeof r === "object" && "closed" in r ? r : { closed: true },
  );
export const listChromeTabs = () => invoke<ChromeTab[]>("list_chrome_tabs");
export const importChromeTabs = (tabs: ChromeTab[], bucketId: string | null, close: boolean) =>
  invoke<ImportResult>("import_chrome_tabs", { tabs, bucketId, close });

// Window
export const hideCapture = () => invoke<void>("hide_capture");
export const showMain = (targetType?: TargetType, targetId?: string) =>
  invoke<void>(
    "show_main",
    targetType && targetId ? { targetType, targetId } : {},
  );

// Permissions, pairing, data (v0.1 hardening)
export const permissionStatus = () => invoke<PermissionStatus>("permission_status");
/** Prompts if undetermined; resolves the resulting automation status string (not a PermissionStatus). */
export const requestAutomation = () => invoke<AutomationStatus>("request_automation");
/** Prompts if undetermined; resolves the resulting microphone status string (not a PermissionStatus). */
export const requestMicrophone = () => invoke<MicrophoneStatus>("request_microphone");
export const openPrivacySettings = (pane: PrivacyPane) => invoke<void>("open_privacy_settings", { pane });
export const pairingCode = () => invoke<string>("pairing_code");
export const rotateExtensionToken = () => invoke<string>("rotate_extension_token");
export const respondPair = (id: string, allow: boolean) => invoke<void>("respond_pair", { id, allow });
export const exportLibrary = (format: "markdown" | "json", destDir: string) =>
  invoke<ExportResult>("export_library", { format, destDir });
export const backupDatabase = (destPath: string) => invoke<BackupResult>("backup_database", { destPath });
export const readyToQuit = () => invoke<void>("ready_to_quit");
/** Removes downloaded Whisper models (and partial downloads); resolves the bytes freed. */
export const deleteWhisperModels = () => invoke<DeletedModels>("delete_whisper_models");
/** Wipes the data folder and restarts Stow. `confirm` must be exactly "delete". */
export const resetAllData = (confirm: string) => invoke<void>("reset_all_data", { confirm });
/**
 * NOT in tauri-contract.md yet: pre-download the Whisper model. Callers must treat a
 * rejection as "not available" (the model then downloads on the first transcription).
 */
export const downloadWhisperModel = (model: string) => invoke<void>("download_whisper_model", { model });
/** NOT in the contract yet: whether the configured Whisper model is on disk. Rejects when unavailable. */
export const whisperModelDownloaded = (model: string) => invoke<boolean>("whisper_model_downloaded", { model });

/** Pairing code `port:token`, falling back to the settings fields on older backends. */
export async function pairingCodeOr(s: Settings | null): Promise<string> {
  try {
    return await pairingCode();
  } catch {
    if (!s) throw new Error("Settings not loaded");
    return `${s.extension_port}:${s.extension_token}`;
  }
}

// Events
export function onEvent<N extends EventName>(
  name: N,
  handler: (payload: EventPayloads[N]) => void,
): Promise<UnlistenFn> {
  try {
    return listen<EventPayloads[N]>(name, (e) => handler(e.payload));
  } catch (e) {
    return Promise.reject(e);
  }
}

/**
 * Subscribe for the lifetime of a component: returns a cleanup that also covers
 * the listen promise resolving after unmount. Rejections (no Tauri) are swallowed.
 */
export function subscribe<N extends EventName>(name: N, handler: (payload: EventPayloads[N]) => void): () => void {
  let unlisten: UnlistenFn | null = null;
  let cancelled = false;
  onEvent(name, handler)
    .then((u) => {
      if (cancelled) u();
      else unlisten = u;
    })
    .catch(() => {});
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

