// Pure multi-select helpers for the item list: ⌘-click toggles, ⇧-click
// extends from the anchor, ⌘A takes every visible row.

export function toggleId(selected: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(selected);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

/**
 * Ids between `anchor` and `target` in `rowIds` order (inclusive), added to
 * `selected`. An anchor that is not in the rows selects only the target.
 */
export function rangeIds(rowIds: readonly string[], anchor: string | null, target: string, selected: ReadonlySet<string> = new Set()): Set<string> {
  const a = anchor ? rowIds.indexOf(anchor) : -1;
  const t = rowIds.indexOf(target);
  const next = new Set(selected);
  if (t < 0) return next;
  if (a < 0) {
    next.add(target);
    return next;
  }
  for (let i = Math.min(a, t); i <= Math.max(a, t); i++) next.add(rowIds[i]);
  return next;
}

export function selectAll(rowIds: readonly string[]): Set<string> {
  return new Set(rowIds);
}
