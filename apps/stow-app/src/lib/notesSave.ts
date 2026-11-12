// Notes saves are keyed by the record the editor was bound to (`item:<id>` /
// `bucket:<id>`), never by the current selection.
import { buckets } from "./stores/buckets.svelte";
import { items } from "./stores/items.svelte";
import { splitKey } from "./util";

export function saveNotesByKey(recordKey: string, md: string): Promise<unknown> | undefined {
  const [kind, id] = splitKey(recordKey);
  if (kind === "item" && id) return items.updateNotes(id, md);
  if (kind === "bucket" && id) return buckets.saveNotes(id, md);
  return undefined;
}

/** Append Markdown to a record's notes (AI output, transcripts). */
export function appendNotes(kind: "item" | "bucket", id: string, md: string) {
  const cur = (kind === "item" ? items.get(id)?.notes_md : buckets.byId(id)?.notes_md) ?? "";
  const next = cur + (cur.trim() ? "\n\n" : "") + md.trim() + "\n";
  saveNotesByKey(`${kind}:${id}`, next)?.catch(() => {});
}
