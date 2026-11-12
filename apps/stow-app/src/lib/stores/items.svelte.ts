import * as api from "../api";
import { errorCode } from "../errors";
import { invalidateTarget } from "../targets";
import type { AddItemResult, Item, ItemFilter } from "../types";
import { buckets } from "./buckets.svelte";
import { toast, ui } from "./ui.svelte";

let filter = $state.raw<ItemFilter>({ kind: "inbox" });
/** The current list INCLUDING done items; views filter them (so "Show done (N)" is instant). */
let list = $state.raw<Item[]>([]);
let loading = $state(false);
let loadError = $state<unknown>(null);
/** Open (not done) items per bucket id; "inbox" for the Inbox, "all" for everything. */
let counts = $state.raw<Record<string, number>>({});
/** Every item we have seen, so the detail pane can show one that is not in the current list. */
let known = $state.raw<Record<string, Item>>({});
/** Ids that `get_item` reported not_found; never re-fetched so effects don't loop/toast. */
let missing = $state.raw<Record<string, true>>({});
/** Ids whose fetch failed transiently (shown as "Couldn't load — Retry"). */
let failed = $state.raw<Record<string, true>>({});
let loadSeq = 0;

export function matches(item: Item, f: ItemFilter): boolean {
  switch (f.kind) {
    case "all":
      return true;
    case "inbox":
      return item.bucket_id == null;
    case "bucket":
      return item.bucket_id === f.id;
  }
}

export function isDone(item: Item): boolean {
  return !!item.done_at;
}

/** Open-item counts keyed by bucket id, plus "inbox" and "all". */
export function countOpen(all: Item[]): Record<string, number> {
  const out: Record<string, number> = { inbox: 0, all: 0 };
  for (const i of all) {
    if (i.done_at) continue;
    out.all++;
    const k = i.bucket_id ?? "inbox";
    out[k] = (out[k] ?? 0) + 1;
  }
  return out;
}

function without<T>(rec: Record<string, T>, id: string): Record<string, T> {
  if (!(id in rec)) return rec;
  const { [id]: _gone, ...rest } = rec;
  return rest;
}

function rememberAll(l: Item[]) {
  if (!l.length) return;
  known = { ...known, ...Object.fromEntries(l.map((i) => [i.id, i])) };
  for (const i of l) {
    missing = without(missing, i.id);
    failed = without(failed, i.id);
  }
}

function forget(id: string) {
  known = without(known, id);
  list = list.filter((i) => i.id !== id);
  if (ui.selectedItemId === id) ui.selectItem(null);
  invalidateTarget("item", id);
}

/** A bucket id we have never seen means another window created it: refresh buckets. */
function ensureBucketKnown(bucketId: string | null) {
  if (bucketId && buckets.loaded && !buckets.byId(bucketId)) void buckets.load({ quiet: true });
  else if (bucketId && !buckets.loaded) void buckets.load({ quiet: true });
}

let countsInflight: Promise<void> | null = null;
let countsDirty = false;

export const items = {
  get filter() {
    return filter;
  },
  get list() {
    return list;
  },
  get loading() {
    return loading;
  },
  get loadError() {
    return loadError;
  },
  get counts() {
    return counts;
  },
  get inboxCount() {
    return counts.inbox ?? 0;
  },
  get(id: string | null | undefined): Item | undefined {
    return id ? known[id] : undefined;
  },
  isMissing(id: string): boolean {
    return !!missing[id];
  },
  hasFailed(id: string): boolean {
    return !!failed[id];
  },

  setFilter(f: ItemFilter) {
    filter = f;
  },
  setList(l: Item[]) {
    list = l;
    rememberAll(l);
  },
  reset() {
    filter = { kind: "inbox" };
    list = [];
    known = {};
    missing = {};
    failed = {};
    counts = {};
    loading = false;
    loadError = null;
  },

  async load(f: ItemFilter = filter) {
    filter = f;
    const seq = ++loadSeq;
    loading = true;
    loadError = null;
    try {
      const l = await api.listItems(f, true);
      if (seq !== loadSeq) return; // a newer load superseded this one
      this.setList(Array.isArray(l) ? l : []);
      if (f.kind === "all") counts = countOpen(list);
    } catch (e) {
      if (seq === loadSeq) {
        loadError = e;
        list = [];
      }
    } finally {
      if (seq === loadSeq) loading = false;
    }
  },

  /** Sidebar counts. Concurrent calls share one fetch (plus one follow-up if asked again meanwhile). */
  refreshCounts(): Promise<void> {
    if (filter.kind === "all" && !loading && !loadError) {
      counts = countOpen(list);
      return Promise.resolve();
    }
    if (countsInflight) {
      countsDirty = true;
      return countsInflight;
    }
    countsInflight = (async () => {
      try {
        do {
          countsDirty = false;
          const all = await api.listItems({ kind: "all" });
          counts = countOpen(Array.isArray(all) ? all : []);
        } while (countsDirty);
      } catch {
        /* counts are cosmetic */
      } finally {
        countsInflight = null;
      }
    })();
    return countsInflight;
  },
  /** Back-compat name. */
  refreshInboxCount() {
    return this.refreshCounts();
  },

  /** Make sure `known` has this item. Only not_found is remembered as missing; other failures can be retried. */
  async ensure(id: string): Promise<Item | undefined> {
    if (known[id]) return known[id];
    if (missing[id] || failed[id]) return undefined;
    try {
      const it = await api.getItem(id);
      rememberAll([it]);
      return it;
    } catch (e) {
      if (errorCode(e) === "not_found") missing = { ...missing, [id]: true };
      else failed = { ...failed, [id]: true };
      return undefined;
    }
  },
  retry(id: string) {
    failed = without(failed, id);
    return this.ensure(id);
  },

  /** `quiet`: rethrow instead of toasting (batch callers summarise failures themselves). */
  async addItem(url: string, bucketId: string | null, title?: string | null, opts: { quiet?: boolean } = {}): Promise<AddItemResult | null> {
    try {
      const r = await api.addItem(url, bucketId, title);
      this.applyUpdated(r.item);
      ensureBucketKnown(r.item.bucket_id);
      if (r.created) ui.highlight(r.item.id);
      void this.refreshCounts();
      return r;
    } catch (e) {
      if (opts.quiet) throw e;
      toast.error(e);
      return null;
    }
  },

  /** Replace an item by id, insert it if it newly matches the filter, or drop it if it no longer matches. */
  applyUpdated(item: Item) {
    rememberAll([item]);
    const inList = list.some((i) => i.id === item.id);
    if (inList) {
      if (matches(item, filter)) list = list.map((i) => (i.id === item.id ? item : i));
      else list = list.filter((i) => i.id !== item.id);
    } else if (matches(item, filter)) {
      list = [item, ...list];
    }
  },

  /**
   * `item-added` handler: a row was inserted by any path (panel, paste,
   * extension, import). Fetch it, insert if it matches the filter (dedup by
   * id) and keep the counts current. Resolves the item (or null).
   */
  async onAdded(id: string): Promise<Item | null> {
    try {
      const it = await api.getItem(id);
      const isNew = !list.some((i) => i.id === id);
      this.applyUpdated(it);
      if (isNew) ui.highlight(id);
      ensureBucketKnown(it.bucket_id);
      await this.refreshCounts();
      return it;
    } catch {
      return null; // deleted again before we could fetch it
    }
  },

  /** `item-updated` handler: refetch the row and apply it (inserts unknown rows that match). */
  async refresh(id: string) {
    const before = known[id];
    try {
      const it = await api.getItem(id);
      this.applyUpdated(it);
      ensureBucketKnown(it.bucket_id);
      if (!before || before.bucket_id !== it.bucket_id || !!before.done_at !== !!it.done_at) {
        invalidateTarget("item", id);
        await this.refreshCounts();
      }
    } catch (e) {
      if (errorCode(e) === "not_found") forget(id);
    }
  },

  async move(id: string, bucketId: string | null): Promise<Item | null> {
    try {
      const it = await api.moveItem(id, bucketId);
      this.applyUpdated(it);
      invalidateTarget("item", id);
      void this.refreshCounts();
      return it;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },

  async setDone(id: string, done: boolean): Promise<Item | null> {
    try {
      const it = await api.setItemDone(id, done);
      this.applyUpdated(it);
      void this.refreshCounts();
      return it;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },

  /** Rejects after toasting so editors can show "Save failed". */
  async updateNotes(id: string, notesMd: string): Promise<Item> {
    try {
      const it = await api.updateItemNotes(id, notesMd);
      this.applyUpdated(it);
      return it;
    } catch (e) {
      toast.error(e);
      throw e;
    }
  },

  async remove(id: string): Promise<boolean> {
    try {
      await api.deleteItem(id);
      forget(id);
      void this.refreshCounts();
      return true;
    } catch (e) {
      toast.error(e);
      return false;
    }
  },

  async refetch(id: string) {
    try {
      await api.refetchItem(id);
      const cur = known[id];
      if (cur) this.applyUpdated({ ...cur, snapshot_status: "pending", snapshot_error: null });
    } catch (e) {
      toast.error(e);
    }
  },
};
