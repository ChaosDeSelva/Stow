// Effect-loop guards: list components load once per parent, whether invoke throws,
// rejects or resolves, and never keep calling the backend.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(), readText: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(async () => false), requestPermission: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: vi.fn(async () => false), enable: vi.fn(), disable: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import AiPanel from "./AiPanel.svelte";
import TaskList from "./TaskList.svelte";
import { toast, ui } from "../stores/ui.svelte";
import type { Item } from "../types";

const mockInvoke = vi.mocked(invoke);

async function settle() {
  for (let i = 0; i < 6; i++) {
    flushSync();
    await new Promise((r) => setTimeout(r, 5));
  }
}

const modes = {
  throwing: () =>
    mockInvoke.mockImplementation(() => {
      throw new TypeError("no runtime");
    }),
  rejecting: () => mockInvoke.mockImplementation(async () => Promise.reject({ code: "db", message: "database error" })),
  resolving: () =>
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "has_api_key") return true;
      if (cmd === "get_settings") return { ai_provider: "anthropic", ai_model: "claude-opus-5-5", ai_consent_given: true };
      if (cmd === "read_snapshot") return "some page text";
      return [];
    }),
};

const item: Item = {
  id: "01HZZZZZZZZZZZZZZZZZZZZZZZ",
  bucket_id: null,
  url: "https://www.youtube.com/watch?v=1",
  canonical_url: "https://www.youtube.com/watch?v=1",
  title: "youtube.com",
  description: null,
  site_name: "YouTube",
  kind: "video",
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
};

describe.each(Object.keys(modes) as (keyof typeof modes)[])("no effect loops (%s invoke)", (mode) => {
  let target: HTMLElement;
  beforeEach(() => {
    mockInvoke.mockReset();
    modes[mode]();
    toast.clear();
    target = document.createElement("div");
    document.body.appendChild(target);
  });
  afterEach(() => target.remove());

  it("TaskList loads once", async () => {
    const app = mount(TaskList, { target, props: { parentType: "item", parentId: `t-${mode}` } });
    await settle();
    const n = mockInvoke.mock.calls.length;
    expect(n).toBe(1);
    await settle();
    expect(mockInvoke.mock.calls.length).toBe(n);
    expect(ui.toasts.length).toBeLessThanOrEqual(1);
    unmount(app);
  });

  it("AiPanel makes a bounded number of calls", async () => {
    const app = mount(AiPanel, {
      target,
      props: { parentType: "item", parentId: `a-${mode}`, item: { ...item, id: `a-${mode}` }, appendToNotes: () => {} },
    });
    await settle();
    const n = mockInvoke.mock.calls.length;
    expect(n).toBeLessThanOrEqual(6); // list_ai_runs, list_recordings, get_settings, has_api_key×2 (+read_snapshot)
    await settle();
    expect(mockInvoke.mock.calls.length).toBe(n);
    expect(ui.toasts.length).toBeLessThanOrEqual(3);
    unmount(app);
  });
});

describe("AiPanel rules", () => {
  let target: HTMLElement;
  beforeEach(() => {
    mockInvoke.mockReset();
    modes.resolving();
    target = document.createElement("div");
    document.body.appendChild(target);
  });
  afterEach(() => target.remove());

  it("videos can't summarise the page, empty notes can't be summarised, title==host offers Fix title", async () => {
    const app = mount(AiPanel, { target, props: { parentType: "item", parentId: item.id, item, appendToNotes: () => {} } });
    await settle();
    const button = (label: string) => Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === label);
    expect(button("Summarize page")?.disabled).toBe(true);
    expect(target.textContent).toContain("Video pages have no readable text yet.");
    expect(button("Summarize notes")?.disabled).toBe(true);
    expect(button("Fix title & description")).toBeTruthy();
    expect(button("Extract metadata")).toBeUndefined();
    expect(target.textContent).toContain("Anthropic");
    expect(target.textContent).toContain("Nothing generated yet.");
    unmount(app);
  });
});
