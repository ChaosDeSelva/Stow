import { describe, expect, it } from "vitest";
import { doneCount, openCount, visibleItems } from "./listview";
import type { Item } from "./types";

function item(over: Partial<Item>): Item {
  return {
    id: "x",
    bucket_id: null,
    url: "https://example.com",
    canonical_url: "https://example.com/",
    title: "T",
    description: null,
    site_name: null,
    kind: "article",
    favicon_path: null,
    image_path: null,
    notes_md: "",
    snapshot_status: "done",
    snapshot_error: null,
    snapshot_fetched_at: null,
    done_at: null,
    position: 0,
    created_at: "2026-11-08T10:00:00Z",
    updated_at: "2026-11-08T10:00:00Z",
    ...over,
  };
}

const list = [
  item({ id: "a", title: "Banana", kind: "video", created_at: "2026-11-01T00:00:00Z", url: "https://youtube.com/1" }),
  item({ id: "b", title: "apple", kind: "article", created_at: "2026-11-03T00:00:00Z", url: "https://zeta.example/1" }),
  item({ id: "c", title: "Cherry", kind: "article", created_at: "2026-11-02T00:00:00Z", done_at: "2026-11-05T00:00:00Z" }),
  item({ id: "d", title: "Date", kind: "other", created_at: "2026-11-04T00:00:00Z", url: "https://alpha.example/" }),
];
const ids = (l: Item[]) => l.map((i) => i.id);

describe("visibleItems", () => {
  it("newest first, done hidden by default", () => {
    expect(ids(visibleItems(list, { kind: "all", sort: "newest", showDone: false }))).toEqual(["d", "b", "a"]);
  });
  it("oldest, title (case-insensitive) and site sorts", () => {
    expect(ids(visibleItems(list, { kind: "all", sort: "oldest", showDone: false }))).toEqual(["a", "b", "d"]);
    expect(ids(visibleItems(list, { kind: "all", sort: "title", showDone: false }))).toEqual(["b", "a", "d"]);
    expect(ids(visibleItems(list, { kind: "all", sort: "site", showDone: false }))).toEqual(["d", "a", "b"]);
  });
  it("kind pills filter", () => {
    expect(ids(visibleItems(list, { kind: "video", sort: "newest", showDone: false }))).toEqual(["a"]);
    expect(ids(visibleItems(list, { kind: "article", sort: "newest", showDone: true }))).toEqual(["b", "c"]);
  });
  it("Show done puts done rows after open ones", () => {
    expect(ids(visibleItems(list, { kind: "all", sort: "newest", showDone: true }))).toEqual(["d", "b", "a", "c"]);
  });
  it("rows pending an undoable delete are hidden", () => {
    expect(ids(visibleItems(list, { kind: "all", sort: "newest", showDone: false, hidden: (id) => id === "b" }))).toEqual(["d", "a"]);
  });
  it("counts", () => {
    expect(doneCount(list, { kind: "all" })).toBe(1);
    expect(doneCount(list, { kind: "video" })).toBe(0);
    expect(openCount(list, { kind: "all" })).toBe(3);
  });
});

describe("archivedOnly", () => {
  it("shows only archived rows, in the chosen order", () => {
    expect(ids(visibleItems(list, { kind: "all", sort: "newest", showDone: false, archivedOnly: true }))).toEqual(["c"]);
    expect(ids(visibleItems(list, { kind: "video", sort: "newest", showDone: false, archivedOnly: true }))).toEqual([]);
  });
});
