import type { ParentType, TargetType } from "../types";
import { toast } from "./ui.svelte";

type Owner = ParentType | TargetType;

/**
 * A reactive cache of lists keyed by their owner (`parent_type:parent_id` or
 * `target_type:target_id`). Several components can show lists for different
 * owners at once (bucket tasks above item tasks), so nothing is "current".
 */
export function parentKeyed<T extends { id: string }>(
  loader: (ownerType: Owner, ownerId: string) => Promise<T[]>,
) {
  let data = $state.raw<Record<string, T[]>>({});
  let quietFailed = false;
  const inflight = new Map<string, Promise<T[]>>();
  const key = (t: Owner, id: string) => `${t}:${id}`;

  const col = {
    key,
    get(ownerType: Owner, ownerId: string): T[] {
      return data[key(ownerType, ownerId)] ?? [];
    },
    set(ownerType: Owner, ownerId: string, list: T[]) {
      data = { ...data, [key(ownerType, ownerId)]: list };
    },
    /** `quiet` (background reloads): only the first consecutive failure is toasted. */
    async load(ownerType: Owner, ownerId: string, quiet = false): Promise<T[]> {
      const k = key(ownerType, ownerId);
      const existing = inflight.get(k);
      if (existing) return existing;
      const p = loader(ownerType, ownerId)
        .then((l) => {
          data = { ...data, [k]: l };
          quietFailed = false;
          return l;
        })
        .catch((e) => {
          if (!quiet || !quietFailed) toast.error(e);
          if (quiet) quietFailed = true;
          return data[k] ?? [];
        })
        .finally(() => inflight.delete(k));
      inflight.set(k, p);
      return p;
    },
    upsert(ownerType: Owner, ownerId: string, item: T, where: "start" | "end" = "end") {
      const k = key(ownerType, ownerId);
      const cur = data[k] ?? [];
      const idx = cur.findIndex((x) => x.id === item.id);
      const next =
        idx === -1
          ? where === "start"
            ? [item, ...cur]
            : [...cur, item]
          : cur.map((x) => (x.id === item.id ? item : x));
      data = { ...data, [k]: next };
    },
    /** Replace an entry wherever it lives. Returns false when unknown. */
    replace(item: T): boolean {
      for (const k of Object.keys(data)) {
        if (data[k].some((x) => x.id === item.id)) {
          data = { ...data, [k]: data[k].map((x) => (x.id === item.id ? item : x)) };
          return true;
        }
      }
      return false;
    },
    remove(id: string) {
      let changed = false;
      const next: Record<string, T[]> = {};
      for (const k of Object.keys(data)) {
        const l = data[k];
        if (l.some((x) => x.id === id)) {
          changed = true;
          next[k] = l.filter((x) => x.id !== id);
        } else next[k] = l;
      }
      if (changed) data = next;
    },
    ownerOf(id: string): { ownerType: Owner; ownerId: string } | null {
      for (const k of Object.keys(data)) {
        if (data[k].some((x) => x.id === id)) {
          const i = k.indexOf(":");
          return { ownerType: k.slice(0, i) as Owner, ownerId: k.slice(i + 1) };
        }
      }
      return null;
    },
    /** Reload whichever loaded list contains `id` (for `*-updated` events). */
    async reloadContaining(id: string) {
      const o = col.ownerOf(id);
      if (o) await col.load(o.ownerType, o.ownerId, true);
    },
    /** Reload every list currently loaded (used after an owner's own list may have changed). */
    async reloadAll() {
      await Promise.all(
        Object.keys(data).map((k) => {
          const i = k.indexOf(":");
          return col.load(k.slice(0, i) as Owner, k.slice(i + 1), true);
        }),
      );
    },
  };
  return col;
}
