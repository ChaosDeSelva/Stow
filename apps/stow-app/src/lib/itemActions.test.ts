import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn(), revealItemInDir: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { archiveMany, deleteMany } from "./itemActions";
import { deletions } from "./stores/deletions.svelte";
import { items } from "./stores/items.svelte";
import { toast } from "./stores/ui.svelte";
import type { Item } from "./types";

const mockInvoke = vi.mocked(invoke);

const item = (id: string): Item =>
  ({
    id,
    bucket_id: null,
    url: `https://ex.com/${id}`,
    canonical_url: `https://ex.com/${id}`,
    title: id,
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
    created_at: "",
    updated_at: "",
  }) as Item;

const calls = (cmd: string) => mockInvoke.mock.calls.filter((c) => c[0] === cmd).map((c) => c[1] as Record<string, unknown>);

describe("bulk archive / delete commits", () => {
  beforeEach(async () => {
    await deletions.flushAll();
    mockInvoke.mockReset();
    toast.clear();
    items.reset();
  });
  afterEach(async () => {
    await deletions.flushAll();
  });

  it("deleteMany sends every delete_item at once instead of one after another", async () => {
    const resolvers: (() => void)[] = [];
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "delete_item") return new Promise<void>((r) => resolvers.push(r));
      return Promise.resolve([]);
    });
    deleteMany([item("a"), item("b"), item("c")]);
    expect(calls("delete_item")).toEqual([]);
    const flushed = deletions.flushAll();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    // All three are in flight before any of them has answered.
    expect(calls("delete_item")).toEqual([{ id: "a" }, { id: "b" }, { id: "c" }]);
    expect(resolvers).toHaveLength(3);
    resolvers.forEach((r) => r());
    await flushed;
    expect(deletions.isHidden("a") || deletions.isHidden("b") || deletions.isHidden("c")).toBe(false);
  });

  it("archiveMany sends every set_item_done at once and one failure does not stop the others", async () => {
    const resolvers: ((v?: unknown) => void)[] = [];
    const rejecters: ((e: unknown) => void)[] = [];
    mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "set_item_done") {
        return new Promise((res, rej) => {
          resolvers.push(res);
          rejecters.push(rej);
        });
      }
      return Promise.resolve([]);
    });
    archiveMany([item("x"), item("y")]);
    const flushed = deletions.flushAll();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect(calls("set_item_done")).toEqual([
      { id: "x", done: true },
      { id: "y", done: true },
    ]);
    rejecters[0](new Error("boom"));
    resolvers[1]({ ...item("y"), done_at: "2026-11-08T12:00:00Z" });
    await flushed;
    expect(deletions.isHidden("x") || deletions.isHidden("y")).toBe(false);
  });
});
