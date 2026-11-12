// Pure helpers for the middle column: kind pills, sort menu, done filtering.
import type { Item } from "./types";
import { hostOf } from "./util";

export type KindFilter = "all" | "video" | "article";
export type SortOrder = "newest" | "oldest" | "title" | "site";

export const SORT_LABELS: Record<SortOrder, string> = {
  newest: "Newest",
  oldest: "Oldest",
  title: "Title",
  site: "Site",
};

export function displayTitle(i: Item): string {
  return i.title?.trim() || hostOf(i.url) || i.url;
}

export function siteOf(i: Item): string {
  return i.site_name || hostOf(i.url);
}

export interface ViewOptions {
  kind: KindFilter;
  sort: SortOrder;
  /** Include archived (done) rows after the open ones. */
  showDone: boolean;
  /** Only archived rows (the Archived view). */
  archivedOnly?: boolean;
  /** Rows hidden by a pending (undoable) delete or archive. */
  hidden?: (id: string) => boolean;
}

function base(list: Item[], o: ViewOptions): Item[] {
  return list.filter((i) => !(o.hidden?.(i.id) ?? false) && (o.kind === "all" || i.kind === o.kind));
}

/** Rows to show, in order; done rows only when `showDone` (and then after open ones). */
export function visibleItems(list: Item[], o: ViewOptions): Item[] {
  const rows = base(list, o).filter((i) => (o.archivedOnly ? !!i.done_at : o.showDone || !i.done_at));
  const cmp = (a: Item, b: Item): number => {
    switch (o.sort) {
      case "oldest":
        return a.created_at.localeCompare(b.created_at);
      case "title":
        return displayTitle(a).localeCompare(displayTitle(b), undefined, { sensitivity: "base" });
      case "site":
        return siteOf(a).localeCompare(siteOf(b)) || b.created_at.localeCompare(a.created_at);
      default:
        return b.created_at.localeCompare(a.created_at);
    }
  };
  return [...rows].sort((a, b) => Number(!!a.done_at) - Number(!!b.done_at) || cmp(a, b));
}

/** Archived (done) rows matching the kind filter (for "Show archived (N)" and the Archived view). */
export function doneCount(list: Item[], o: Omit<ViewOptions, "sort" | "showDone">): number {
  return base(list, { ...o, sort: "newest", showDone: true }).filter((i) => !!i.done_at).length;
}

/** Open rows matching the kind filter (header "Inbox · 14"). */
export function openCount(list: Item[], o: Omit<ViewOptions, "sort" | "showDone">): number {
  return base(list, { ...o, sort: "newest", showDone: true }).filter((i) => !i.done_at).length;
}
