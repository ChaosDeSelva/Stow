import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Item } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (p: string) => `asset://${p}`,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../targets", () => ({ invalidateTarget: vi.fn(), resolveTarget: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { invalidateTarget } from "../targets";
import { buckets } from "./buckets.svelte";
import { items } from "./items.svelte";
import { ui } from "./ui.svelte";

const mockInvoke = vi.mocked(invoke);

function makeItem(over: Partial<Item> = {}): Item {
  return {
    id: "01A",
    bucket_id: null,
    url: "https://example.com",
    canonical_url: "https://example.com/",
    title: "Example",
    description: null,
    site_name: null,
    kind: "other",
    favicon_path: null,
    image_path: null,
    notes_md: "",
    snapshot_status: "pending",
    snapshot_error: null,
    snapshot_fetched_at: null,
    position: 0,
    created_at: "2026-11-08T10:00:00Z",
    updated_at: "2026-11-08T10:00:00Z",
    ...over,
  };
}

describe("items store", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    items.reset();
  });

  it("addItem invokes add_item with camelCase args and inserts the item", async () => {
    const created = makeItem({ id: "01B", bucket_id: "bk1", title: null });
    mockInvoke.mockResolvedValueOnce(created);
    items.setFilter({ kind: "bucket", id: "bk1" });

    const got = await items.addItem("https://example.com", "bk1", "Hello");

    expect(mockInvoke).toHaveBeenCalledWith("add_item", {
      url: "https://example.com",
      bucketId: "bk1",
      title: "Hello",
    });
    expect(got).toEqual({ item: created, created: true });
    expect(items.list.map((i) => i.id)).toContain("01B");
    expect(items.list[0].id).toBe("01B");
  });

  it("addItem into another bucket does not pollute the current list", async () => {
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "01C", bucket_id: "other" }));
    items.setFilter({ kind: "inbox" });
    await items.addItem("https://example.com", "other");
    expect(items.list.find((i) => i.id === "01C")).toBeUndefined();
  });

  it("addItem into the current list replaces an existing row with the same id (dedup)", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "01D", title: "old" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "01D", title: "new" }));
    await items.addItem("https://example.com", null);
    expect(items.list).toHaveLength(1);
    expect(items.list[0].title).toBe("new");
  });

  it("applyUpdated replaces by id and leaves others untouched", () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "a", title: "A" }), makeItem({ id: "b", title: "B" })]);
    items.applyUpdated(makeItem({ id: "b", title: "B2", snapshot_status: "done" }));
    expect(items.list.map((i) => i.title)).toEqual(["A", "B2"]);
    expect(items.list[1].snapshot_status).toBe("done");
  });

  it("applyUpdated removes an item that no longer matches the current filter", () => {
    items.setFilter({ kind: "bucket", id: "bk1" });
    items.setList([makeItem({ id: "a", bucket_id: "bk1" })]);
    items.applyUpdated(makeItem({ id: "a", bucket_id: "bk2" }));
    expect(items.list).toHaveLength(0);
  });

  it("applyUpdated ignores unknown ids that do not match the filter", () => {
    items.setFilter({ kind: "inbox" });
    items.setList([]);
    items.applyUpdated(makeItem({ id: "z", bucket_id: "bk9" }));
    expect(items.list).toHaveLength(0);
  });
});

describe("items store: item-added / item-updated events", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    items.reset();
  });

  it("onAdded fetches the item and inserts it when it matches the filter", async () => {
    items.setFilter({ kind: "bucket", id: "bk1" });
    items.setList([makeItem({ id: "old", bucket_id: "bk1" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "new", bucket_id: "bk1" }));
    await items.onAdded("new");
    expect(mockInvoke).toHaveBeenCalledWith("get_item", { id: "new" });
    expect(items.list.map((i) => i.id)).toEqual(["new", "old"]);
  });

  it("onAdded ignores items for another bucket but still bumps the Inbox count for Inbox items", async () => {
    items.setFilter({ kind: "bucket", id: "bk1" });
    items.setList([]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "x", bucket_id: null }));
    mockInvoke.mockResolvedValueOnce([makeItem({ id: "x", bucket_id: null })]); // list_items inbox count
    await items.onAdded("x");
    expect(items.list).toHaveLength(0);
    expect(items.inboxCount).toBe(1);
  });

  it("onAdded de-duplicates by id", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "dup", title: "old" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "dup", title: "fresh" }));
    mockInvoke.mockResolvedValueOnce([makeItem({ id: "dup" })]);
    await items.onAdded("dup");
    expect(items.list).toHaveLength(1);
    expect(items.list[0].title).toBe("fresh");
  });

  it("refresh (item-updated) inserts a previously-unknown item that matches the filter", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "unseen", bucket_id: null, title: "Landed" }));
    await items.refresh("unseen");
    expect(items.list.map((i) => i.id)).toEqual(["unseen"]);
  });

  it("refresh ignores an unknown item that does not match the filter", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "elsewhere", bucket_id: "bk9" }));
    await items.refresh("elsewhere");
    expect(items.list).toHaveLength(0);
  });
});

describe("items store: bug-sweep behaviours", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    vi.mocked(invalidateTarget).mockReset();
    items.reset();
    ui.selectItem(null);
  });

  it("remove clears the selection when the removed item was selected", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "sel" }), makeItem({ id: "other" })]);
    ui.selectItem("sel");
    mockInvoke.mockResolvedValueOnce(undefined); // delete_item
    mockInvoke.mockResolvedValueOnce([makeItem({ id: "other" })]); // inbox count
    await items.remove("sel");
    expect(ui.selectedItemId).toBeNull();
    expect(invalidateTarget).toHaveBeenCalledWith("item", "sel");
  });

  it("remove keeps the selection when a different item was removed", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "sel" }), makeItem({ id: "other" })]);
    ui.selectItem("sel");
    mockInvoke.mockResolvedValueOnce(undefined);
    mockInvoke.mockResolvedValueOnce([]);
    await items.remove("other");
    expect(ui.selectedItemId).toBe("sel");
  });

  it("ensure remembers not-found ids and does not re-fetch them", async () => {
    mockInvoke.mockRejectedValueOnce("not found");
    expect(await items.ensure("ghost")).toBeUndefined();
    expect(await items.ensure("ghost")).toBeUndefined();
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(items.isMissing("ghost")).toBe(true);
  });

  it("move invalidates the target cache for that item", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "m" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "m", bucket_id: "bk1" }));
    mockInvoke.mockResolvedValueOnce([]);
    await items.move("m", "bk1");
    expect(invalidateTarget).toHaveBeenCalledWith("item", "m");
  });

  it("onAdded reloads buckets when the item's bucket is unknown locally", async () => {
    items.setFilter({ kind: "inbox" });
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "n", bucket_id: "brand-new" })); // get_item
    mockInvoke.mockResolvedValueOnce([]); // list_buckets
    await items.onAdded("n");
    expect(mockInvoke).toHaveBeenCalledWith("list_buckets");
    expect(buckets.loaded).toBe(true);
  });

  it("refresh refreshes the inbox count when the item's bucket changed", async () => {
    items.setFilter({ kind: "bucket", id: "bk1" });
    items.setList([makeItem({ id: "r", bucket_id: "bk1" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "r", bucket_id: null })); // get_item: moved to inbox
    mockInvoke.mockResolvedValueOnce([makeItem({ id: "r", bucket_id: null })]); // inbox count
    await items.refresh("r");
    expect(items.list).toHaveLength(0);
    expect(items.inboxCount).toBe(1);
  });

  it("updateNotes rethrows after toasting so editors can show 'Save failed'", async () => {
    mockInvoke.mockRejectedValueOnce("disk full");
    await expect(items.updateNotes("x", "md")).rejects.toBeTruthy();
    expect(ui.toasts.some((t) => t.kind === "error")).toBe(true);
  });
});

describe("items store: v0.1 hardening", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    items.reset();
  });

  it("addItem passes through {item, created} and reports duplicates", async () => {
    const it = makeItem({ id: "dup" });
    mockInvoke.mockResolvedValueOnce({ item: it, created: false });
    mockInvoke.mockResolvedValue([]);
    const r = await items.addItem("https://example.com", null);
    expect(r).toEqual({ item: it, created: false });
    expect(ui.isHighlighted("dup")).toBe(false);
  });

  it("load asks for done items too (the view filters them)", async () => {
    mockInvoke.mockResolvedValueOnce([makeItem({ id: "a" }), makeItem({ id: "b", done_at: "2026-11-08T11:00:00Z" })]);
    await items.load({ kind: "inbox" });
    expect(mockInvoke).toHaveBeenCalledWith("list_items", { filter: { kind: "inbox" }, includeDone: true });
    expect(items.list).toHaveLength(2);
  });

  it("load failure sets loadError instead of looking empty", async () => {
    mockInvoke.mockRejectedValueOnce({ code: "db", message: "database error: locked" });
    await items.load({ kind: "inbox" });
    expect(items.loadError).toBeTruthy();
    expect(items.loading).toBe(false);
  });

  it("refreshCounts counts open items per bucket from one list_items call", async () => {
    items.setFilter({ kind: "inbox" });
    mockInvoke.mockResolvedValueOnce([
      makeItem({ id: "1", bucket_id: null }),
      makeItem({ id: "2", bucket_id: "b1" }),
      makeItem({ id: "3", bucket_id: "b1" }),
      makeItem({ id: "4", bucket_id: "b1", done_at: "2026-11-08T11:00:00Z" }),
    ]);
    await items.refreshCounts();
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(items.counts).toEqual({ inbox: 1, all: 3, b1: 2 });
    expect(items.inboxCount).toBe(1);
  });

  it("concurrent refreshCounts calls share the in-flight fetch", async () => {
    items.setFilter({ kind: "inbox" });
    let resolve: (v: unknown) => void = () => {};
    mockInvoke.mockImplementationOnce(() => new Promise((r) => (resolve = r)));
    mockInvoke.mockResolvedValueOnce([]);
    const a = items.refreshCounts();
    const b = items.refreshCounts();
    resolve([makeItem()]);
    await Promise.all([a, b]);
    // one fetch + one follow-up because a second request arrived meanwhile
    expect(mockInvoke).toHaveBeenCalledTimes(2);
  });

  it("ensure: a transient failure is retryable, not 'missing'", async () => {
    mockInvoke.mockRejectedValueOnce({ code: "db", message: "database error: busy" });
    expect(await items.ensure("x")).toBeUndefined();
    expect(items.isMissing("x")).toBe(false);
    expect(items.hasFailed("x")).toBe(true);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "x" }));
    expect((await items.retry("x"))?.id).toBe("x");
    expect(items.hasFailed("x")).toBe(false);
  });

  it("ensure: a structured not_found is remembered", async () => {
    mockInvoke.mockRejectedValueOnce({ code: "not_found", message: "not found" });
    await items.ensure("gone");
    expect(items.isMissing("gone")).toBe(true);
  });

  it("setDone calls set_item_done and updates the row", async () => {
    items.setFilter({ kind: "inbox" });
    items.setList([makeItem({ id: "d" })]);
    mockInvoke.mockResolvedValueOnce(makeItem({ id: "d", done_at: "2026-11-08T12:00:00Z" }));
    mockInvoke.mockResolvedValue([]);
    await items.setDone("d", true);
    expect(mockInvoke).toHaveBeenCalledWith("set_item_done", { id: "d", done: true });
    expect(items.list[0].done_at).toBe("2026-11-08T12:00:00Z");
  });

  it("setList with many rows stays fast (batched known map)", () => {
    const many = Array.from({ length: 5000 }, (_, i) => makeItem({ id: `id${i}` }));
    const t = performance.now();
    items.setList(many);
    expect(performance.now() - t).toBeLessThan(500);
    expect(items.get("id4999")?.id).toBe("id4999");
  });
});
