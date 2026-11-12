import { friendlyError, type ErrorContext } from "../errors";
import type { KindFilter, SortOrder } from "../listview";
import type { ReminderFiredPayload, TargetType } from "../types";

export type Selection =
  | { kind: "setup" }
  | { kind: "inbox" }
  | { kind: "all" }
  | { kind: "archived" }
  | { kind: "today" }
  | { kind: "settings" }
  | { kind: "bucket"; id: string };

/** Views that show the item list (header, rows, Add link). */
export const LIST_KINDS: ReadonlySet<Selection["kind"]> = new Set(["inbox", "all", "archived", "bucket"]);

export type DetailTab = "notes" | "page" | "tasks" | "reminders" | "files" | "recordings" | "ai";
export type BucketTab = Exclude<DetailTab, "page">;
export type { KindFilter, SortOrder };

export interface ToastAction {
  label: string;
  run: () => void;
  /** Rendered as a quiet (ghost) button, e.g. "Show them" next to a summary. */
  secondary?: boolean;
}

export interface Toast {
  id: number;
  kind: "error" | "info" | "success";
  /** Main line (friendly copy). */
  message: string;
  /** Raw error text for the "Details" disclosure. */
  detail?: string;
  /** How many identical toasts were folded into this one. */
  count: number;
  actions?: ToastAction[];
}

export interface ToastOptions {
  detail?: string;
  actions?: ToastAction[];
  /** ms before auto-dismiss; 0 = sticky. Defaults: 4 s, errors 8 s. */
  duration?: number;
  /** Called when the toast leaves for any reason (timeout, dismiss, action). */
  onClose?: () => void;
}

export interface FiredReminder {
  payload: ReminderFiredPayload;
  at: number;
  /** Whether the window was already focused when it fired (then focus is not a "click"). */
  hadFocus: boolean;
}

let selection = $state<Selection>({ kind: "inbox" });
let selectedItemId = $state<string | null>(null);
let detailTab = $state<DetailTab>("notes");
let bucketTab = $state<BucketTab>("notes");
let toasts = $state<Toast[]>([]);
let lastFiredReminder = $state<FiredReminder | null>(null);
let kindFilter = $state<KindFilter>("all");
let sortOrder = $state<SortOrder>("newest");
let showDone = $state<Record<string, boolean>>({});
let highlighted = $state<Record<string, true>>({});
let loadImages = $state<Record<string, boolean>>({});
let newBucketTick = $state(0);
let addLinkOpen = $state(false);
/** Bumped by every openAddLink() so an already-open form takes focus again (⌘L twice). */
let addLinkFocusTick = $state(0);
/** Multi-selection (⌘-click / ⇧-click / ⌘A) in the current list; cleared when the view changes. */
let selectedIds = $state.raw<ReadonlySet<string>>(new Set());
let errorsSuppressed = false;
let nextToastId = 1;

export const TOAST_MS = 4000;
export const ERROR_TOAST_MS = 8000;
export const MAX_TOASTS = 3;

const timers = new Map<number, ReturnType<typeof setTimeout>>();
const closers = new Map<number, () => void>();
const durations = new Map<number, number>();

function arm(id: number) {
  const prev = timers.get(id);
  if (prev) clearTimeout(prev);
  const ms = durations.get(id) ?? TOAST_MS;
  if (ms > 0) timers.set(id, setTimeout(() => dismissToast(id), ms));
}

function pushToast(kind: Toast["kind"], message: string, opts: ToastOptions = {}): number {
  // Same message already showing (and no actions to keep apart): fold it in.
  if (!opts.actions?.length) {
    const same = toasts.find((t) => t.kind === kind && t.message === message && !t.actions?.length);
    if (same) {
      toasts = toasts.map((t) => (t.id === same.id ? { ...t, count: t.count + 1 } : t));
      arm(same.id);
      return same.id;
    }
  }
  const id = nextToastId++;
  durations.set(id, opts.duration ?? (kind === "error" ? ERROR_TOAST_MS : TOAST_MS));
  if (opts.onClose) closers.set(id, opts.onClose);
  toasts = [...toasts, { id, kind, message, detail: opts.detail, count: 1, actions: opts.actions }];
  while (toasts.length > MAX_TOASTS) dismissToast(toasts[0].id);
  arm(id);
  return id;
}

function dismissToast(id: number) {
  const t = timers.get(id);
  if (t) clearTimeout(t);
  timers.delete(id);
  durations.delete(id);
  const close = closers.get(id);
  closers.delete(id);
  if (toasts.some((x) => x.id === id)) toasts = toasts.filter((x) => x.id !== id);
  close?.();
}

export const toast = {
  /** Any rejection value → friendly copy (raw message goes to "Details"). */
  error(e: unknown, ctx?: ErrorContext, opts: ToastOptions = {}) {
    if (errorsSuppressed) return -1;
    const f = friendlyError(e, ctx);
    return pushToast("error", f.title, { ...opts, detail: opts.detail ?? f.detail });
  },
  info(message: string, opts?: ToastOptions) {
    return pushToast("info", message, opts);
  },
  success(message: string, opts?: ToastOptions) {
    return pushToast("success", message, opts);
  },
  dismiss: dismissToast,
  /** Outside the desktop app every command fails: one banner instead of a toast per call. */
  suppressErrors(on: boolean) {
    errorsSuppressed = on;
  },
  /** Test helper. */
  clear() {
    for (const t of [...toasts]) dismissToast(t.id);
  },
};

export const ui = {
  get selection() {
    return selection;
  },
  get selectedItemId() {
    return selectedItemId;
  },
  get detailTab() {
    return detailTab;
  },
  set detailTab(t: DetailTab) {
    detailTab = t;
  },
  get bucketTab() {
    return bucketTab;
  },
  set bucketTab(t: BucketTab) {
    bucketTab = t;
  },
  get toasts() {
    return toasts;
  },
  get lastFiredReminder() {
    return lastFiredReminder;
  },
  get kindFilter() {
    return kindFilter;
  },
  set kindFilter(k: KindFilter) {
    kindFilter = k;
  },
  get sortOrder() {
    return sortOrder;
  },
  set sortOrder(s: SortOrder) {
    sortOrder = s;
  },

  /** Key of the current list ("inbox", "all", "bucket:<id>") for per-list view state. */
  get listKey(): string {
    return selection.kind === "bucket" ? `bucket:${selection.id}` : selection.kind;
  },
  showDone(key: string): boolean {
    return !!showDone[key];
  },
  setShowDone(key: string, on: boolean) {
    showDone = { ...showDone, [key]: on };
  },

  isHighlighted(id: string): boolean {
    return !!highlighted[id];
  },
  /** Mark a freshly added row for 2 s. */
  highlight(id: string, ms = 2000) {
    highlighted = { ...highlighted, [id]: true };
    setTimeout(() => {
      const { [id]: _gone, ...rest } = highlighted;
      highlighted = rest;
    }, ms);
  },

  /** Per-item, per-session "Load images" toggle for readable copies. */
  loadImages(itemId: string): boolean {
    return !!loadImages[itemId];
  },
  setLoadImages(itemId: string, on: boolean) {
    loadImages = { ...loadImages, [itemId]: on };
  },

  /** Incremented by ⌘N; the sidebar opens its "New bucket" field when it changes. */
  get newBucketTick() {
    return newBucketTick;
  },
  requestNewBucket() {
    if (selection.kind === "settings") selection = { kind: "inbox" };
    newBucketTick++;
  },

  /** The inline "Add link" form under the list header (⌘L). */
  get addLinkOpen() {
    return addLinkOpen;
  },
  get addLinkFocusTick() {
    return addLinkFocusTick;
  },
  openAddLink() {
    if (!LIST_KINDS.has(selection.kind) || selection.kind === "archived") this.select({ kind: "inbox" });
    addLinkOpen = true;
    addLinkFocusTick++;
  },
  closeAddLink() {
    addLinkOpen = false;
  },

  /** Bucket id of the current selection for "add here" actions; null = Inbox. */
  get currentBucketId(): string | null {
    return selection.kind === "bucket" ? selection.id : null;
  },

  get selectedIds(): ReadonlySet<string> {
    return selectedIds;
  },
  isChecked(id: string): boolean {
    return selectedIds.has(id);
  },
  setSelectedIds(ids: ReadonlySet<string>) {
    selectedIds = ids;
  },
  clearSelectedIds() {
    if (selectedIds.size) selectedIds = new Set();
  },

  select(sel: Selection) {
    const same =
      sel.kind === selection.kind && (sel.kind !== "bucket" || sel.id === (selection as { id?: string }).id);
    selection = sel;
    if (!same) {
      selectedItemId = null;
      addLinkOpen = false;
      this.clearSelectedIds();
    }
  },
  selectItem(id: string | null) {
    selectedItemId = id;
  },
  /** Select a bucket (null = Inbox) and an item inside it in one step. */
  selectBucketAndItem(bucketId: string | null, itemId: string | null) {
    this.select(bucketId ? { kind: "bucket", id: bucketId } : { kind: "inbox" });
    selectedItemId = itemId;
  },
  /** Show an item in the list it lives in: its bucket (or the Inbox), or Archived when it is archived. */
  reveal(item: { id: string; bucket_id: string | null; done_at?: string | null }) {
    if (item.done_at) this.select({ kind: "archived" });
    else this.select(item.bucket_id ? { kind: "bucket", id: item.bucket_id } : { kind: "inbox" });
    selectedItemId = item.id;
  },
  rememberFired(payload: ReminderFiredPayload, hadFocus: boolean) {
    lastFiredReminder = { payload, at: Date.now(), hadFocus };
  },
  clearFired() {
    lastFiredReminder = null;
  },
  toast,
};

export type { TargetType };
