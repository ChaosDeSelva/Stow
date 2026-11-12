// Undoable deletes and archives: the rows disappear at once, an "Undo" toast
// stays for 6 s, and only then is the command sent. A batch (bulk archive /
// delete) shares one toast and one Undo. Pending work is flushed on
// app-will-quit and on window unload so nothing is silently kept.
import { toast } from "./ui.svelte";

export const UNDO_MS = 6000;

interface Batch {
  ids: string[];
  commit: () => unknown;
  toastId: number;
}

let hidden = $state.raw<Record<string, true>>({});
/** Each pending batch, keyed by every id it hides (so `undo(id)` finds it). */
const pending = new Map<string, Batch>();

function unhide(ids: string[]) {
  const next = { ...hidden };
  let changed = false;
  for (const id of ids) {
    if (next[id]) {
      delete next[id];
      changed = true;
    }
  }
  if (changed) hidden = next;
}

function forget(b: Batch) {
  for (const id of b.ids) if (pending.get(id) === b) pending.delete(id);
}

async function run(b: Batch) {
  try {
    await b.commit();
  } finally {
    unhide(b.ids);
  }
}

export const deletions = {
  /** True while a delete/archive for this id is waiting out its undo window. */
  isHidden(id: string): boolean {
    return !!hidden[id];
  },
  /** Number of pending batches. */
  get pendingCount() {
    return new Set(pending.values()).size;
  },
  /** `message` overrides the default "Deleted “label”." line. */
  schedule(id: string, label: string, commit: () => unknown, opts: { ms?: number; message?: string } = {}) {
    this.scheduleBatch([id], opts.message ?? `Deleted “${label}”.`, commit, opts);
  },
  /** Hide `ids` now; run `commit` once after the undo window unless undone. Ids already pending are skipped. */
  scheduleBatch(ids: string[], message: string, commit: () => unknown, opts: { ms?: number } = {}) {
    const fresh = ids.filter((id) => !pending.has(id));
    if (!fresh.length) return;
    const ms = opts.ms ?? UNDO_MS;
    const add: Record<string, true> = {};
    for (const id of fresh) add[id] = true;
    hidden = { ...hidden, ...add };
    const batch: Batch = { ids: fresh, commit, toastId: -1 };
    for (const id of fresh) pending.set(id, batch);
    batch.toastId = toast.info(message, {
      duration: ms,
      actions: [{ label: "Undo", run: () => deletions.undo(fresh[0]) }],
      onClose: () => {
        // Timeout (or the toast was pushed out): commit unless undone meanwhile.
        if (pending.get(fresh[0]) !== batch) return;
        forget(batch);
        void run(batch);
      },
    });
  },
  /** Undo the whole batch that contains `id`. */
  undo(id: string) {
    const b = pending.get(id);
    if (!b) return;
    forget(b);
    unhide(b.ids);
    toast.dismiss(b.toastId);
  },
  /** Send every pending delete now (quit / unload). */
  async flushAll() {
    const all = [...new Set(pending.values())];
    pending.clear();
    await Promise.allSettled(
      all.map((b) => {
        toast.dismiss(b.toastId);
        return run(b);
      }),
    );
  },
};
