// Item actions shared by the list rows, keyboard shortcuts, the bulk bar and
// the detail header. "Archive" is the UI name for done_at (set_item_done).
import { friendlyText } from "./errors";
import { displayTitle } from "./listview";
import { buckets } from "./stores/buckets.svelte";
import { deletions } from "./stores/deletions.svelte";
import { items } from "./stores/items.svelte";
import { ui } from "./stores/ui.svelte";
import type { Item } from "./types";
import { plural } from "./util";

export function doneLabel(item: Item): string {
  return item.done_at ? "Restore" : "Archive";
}

/** Archive ⇄ restore, whichever applies. */
export function toggleDone(item: Item) {
  if (item.done_at) return restoreItem(item);
  archiveItemLater(item);
}

/** Drop a row from the selection before it disappears (also clears a stale detail pane). */
function unselect(ids: string[]) {
  if (ids.some((id) => id === ui.selectedItemId)) ui.selectItem(null);
  if (ui.selectedIds.size) ui.setSelectedIds(new Set([...ui.selectedIds].filter((id) => !ids.includes(id))));
}

/** Undoable archive: hidden now, set_item_done(true) after 6 s unless "Undo". */
export function archiveItemLater(item: Item) {
  unselect([item.id]);
  deletions.schedule(item.id, displayTitle(item), () => items.setDone(item.id, true), { message: `Archived “${displayTitle(item)}”.` });
}

/** Undoable delete: hidden now, committed after 6 s unless "Undo". */
export function deleteItemLater(item: Item) {
  unselect([item.id]);
  deletions.schedule(item.id, displayTitle(item), () => items.remove(item.id));
}

/** Restore (un-archive) at once, with an Undo that archives again. */
export async function restoreItem(item: Item) {
  const r = await items.setDone(item.id, false);
  if (r) {
    ui.toast.success(`Restored “${displayTitle(item)}”.`, {
      actions: [{ label: "Undo", run: () => void items.setDone(item.id, true) }],
    });
  }
}

/** Run `run` for every item at once (a big batch must finish inside the quit fallback window); returns the failures. */
async function concurrently(list: Item[], run: (it: Item) => Promise<unknown>): Promise<string[]> {
  const results = await Promise.allSettled(list.map((it) => run(it)));
  const failures: string[] = [];
  results.forEach((r, i) => {
    if (r.status === "rejected") failures.push(`${displayTitle(list[i])}: ${friendlyText(r.reason)}`);
  });
  return failures;
}

/** Bulk archive: one undo toast for the whole batch, then set_item_done(true) for all at once. */
export function archiveMany(list: Item[]) {
  if (!list.length) return;
  if (list.length === 1) return archiveItemLater(list[0]);
  unselect(list.map((i) => i.id));
  deletions.scheduleBatch(
    list.map((i) => i.id),
    `Archived ${plural(list.length, "item")}.`,
    () => concurrently(list, (it) => items.setDone(it.id, true)),
  );
}

/** Bulk delete: one undo toast for the whole batch, then delete_item for all at once. */
export function deleteMany(list: Item[]) {
  if (!list.length) return;
  if (list.length === 1) return deleteItemLater(list[0]);
  unselect(list.map((i) => i.id));
  deletions.scheduleBatch(
    list.map((i) => i.id),
    `Deleted ${plural(list.length, "item")}.`,
    () => concurrently(list, (it) => items.remove(it.id)),
  );
}

/** Bulk restore (Archived view): immediate, one summary toast. */
export async function restoreMany(list: Item[]) {
  if (!list.length) return;
  if (list.length === 1) return restoreItem(list[0]);
  unselect(list.map((i) => i.id));
  let ok = 0;
  for (const it of list) if (await items.setDone(it.id, false)) ok++;
  if (ok) ui.toast.success(`Restored ${plural(ok, "item")}.`);
}

/** Bulk move: sequential, one summary toast. */
export async function moveMany(list: Item[], bucketId: string | null) {
  let ok = 0;
  for (const it of list) if (await items.move(it.id, bucketId)) ok++;
  if (ok) ui.toast.success(list.length === 1 ? `Moved to ${buckets.nameOf(bucketId)}` : `Moved ${ok} to ${buckets.nameOf(bucketId)}`);
  ui.clearSelectedIds();
}

export function deleteConfirmCopy(item: Item): string {
  return `Delete “${displayTitle(item)}” and its notes, tasks, files and recordings?`;
}
