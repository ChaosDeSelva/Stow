import * as api from "../api";
import { invalidateAllTargets, invalidateTarget } from "../targets";
import type { Bucket, BucketPatch } from "../types";
import { toast } from "./ui.svelte";

let list = $state.raw<Bucket[]>([]);
let quietFailed = false;
let loaded = $state(false);

export const buckets = {
  get list() {
    return list;
  },
  get loaded() {
    return loaded;
  },
  byId(id: string | null | undefined): Bucket | undefined {
    return id ? list.find((b) => b.id === id) : undefined;
  },
  /** Display name for a bucket id; null/unknown → "Inbox". */
  nameOf(id: string | null | undefined): string {
    const b = this.byId(id);
    return b ? label(b) : "Inbox";
  },
  /** `quiet` (background refreshes): toast only the first failure. */
  async load(opts: { quiet?: boolean } = {}) {
    try {
      const l = await api.listBuckets();
      list = Array.isArray(l) ? l : [];
      loaded = true;
      quietFailed = false;
    } catch (e) {
      if (!opts.quiet || !quietFailed) toast.error(e);
      if (opts.quiet) quietFailed = true;
    }
  },
  async create(name: string, emoji?: string | null): Promise<Bucket | null> {
    try {
      const b = await api.createBucket(name.trim(), emoji);
      list = [...list.filter((x) => x.id !== b.id), b].sort((a, z) => a.position - z.position);
      return b;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
  async update(id: string, patch: BucketPatch): Promise<Bucket | null> {
    try {
      const b = await api.updateBucket(id, patch);
      list = list.map((x) => (x.id === id ? b : x));
      invalidateTarget("bucket", id);
      return b;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
  /** Notes save that rejects after toasting, so the editor can show "Save failed". */
  async saveNotes(id: string, notesMd: string): Promise<Bucket> {
    try {
      const b = await api.updateBucket(id, { notes_md: notesMd });
      list = list.map((x) => (x.id === id ? b : x));
      return b;
    } catch (e) {
      toast.error(e);
      throw e;
    }
  },
  async remove(id: string): Promise<boolean> {
    try {
      await api.deleteBucket(id);
      list = list.filter((x) => x.id !== id);
      invalidateTarget("bucket", id);
      invalidateAllTargets(); // items that lived here now resolve to the Inbox
      return true;
    } catch (e) {
      toast.error(e);
      return false;
    }
  },
  /** Move a bucket one place up (-1) or down (+1) among the visible buckets. */
  async move(id: string, delta: -1 | 1) {
    const ids = list.filter((b) => !b.archived_at).map((b) => b.id);
    const i = ids.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    await this.reorder(ids);
  },
  async reorder(ids: string[]) {
    const order = new Map(ids.map((id, i) => [id, i]));
    list = [...list].sort((a, z) => (order.get(a.id) ?? 1e9) - (order.get(z.id) ?? 1e9));
    try {
      await api.reorderBuckets(ids);
    } catch (e) {
      toast.error(e);
      await this.load();
    }
  },
};

export function label(b: Bucket): string {
  return b.emoji ? `${b.emoji} ${b.name}` : b.name;
}
