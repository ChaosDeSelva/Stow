// Resolve a reminder/notification target to something the UI can show and
// navigate to. Titles come from a session cache; navigation asks for `fresh`
// so a moved item or renamed bucket is never followed to a stale place.
import * as api from "./api";
import type { TargetType } from "./types";

export interface ResolvedTarget {
  title: string;
  /** Bucket to select (null = Inbox). */
  bucketId: string | null;
  /** Item to select, if the target is or belongs to an item. */
  itemId: string | null;
}

const cache = new Map<string, Promise<ResolvedTarget | null>>();
const key = (t: TargetType, id: string) => `${t}:${id}`;

export function invalidateTarget(targetType: TargetType, targetId: string) {
  cache.delete(key(targetType, targetId));
}

export function invalidateAllTargets() {
  cache.clear();
}

export function resolveTarget(
  targetType: TargetType,
  targetId: string,
  opts: { fresh?: boolean } = {},
): Promise<ResolvedTarget | null> {
  const k = key(targetType, targetId);
  let p = opts.fresh ? undefined : cache.get(k);
  if (!p) {
    p = resolve(targetType, targetId).catch(() => null);
    cache.set(k, p);
    void p.then((v) => {
      if (!v && cache.get(k) === p) cache.delete(k);
    });
  }
  return p;
}

async function resolve(targetType: TargetType, targetId: string): Promise<ResolvedTarget | null> {
  if (targetType === "item") {
    const it = await api.getItem(targetId);
    return { title: it.title ?? it.url, bucketId: it.bucket_id, itemId: it.id };
  }
  if (targetType === "bucket") {
    const b = (await api.listBuckets()).find((x) => x.id === targetId);
    return b ? { title: b.emoji ? `${b.emoji} ${b.name}` : b.name, bucketId: b.id, itemId: null } : null;
  }
  // Task: the contract has no get_task, so scan bucket task lists, then item task lists.
  const bucketsList = await api.listBuckets();
  for (const b of bucketsList) {
    const t = (await api.listTasks("bucket", b.id)).find((x) => x.id === targetId);
    if (t) return { title: t.title, bucketId: b.id, itemId: null };
  }
  const allItems = await api.listItems({ kind: "all" });
  for (const it of allItems) {
    const t = (await api.listTasks("item", it.id)).find((x) => x.id === targetId);
    if (t) return { title: t.title, bucketId: it.bucket_id, itemId: it.id };
  }
  return null;
}
