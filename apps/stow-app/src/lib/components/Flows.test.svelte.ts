// Mounted flows: capture panel (saved / duplicate / tab not closed), AI "Add as tasks"
// preview, readable copy images, setup checklist copy, toast details + undo.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

const handlers = new Map<string, (e: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    handlers.set(name, h);
    return () => handlers.delete(name);
  }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(), readText: vi.fn(async () => "see https://clip.example/x") }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(async () => false), requestPermission: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: vi.fn(async () => false), enable: vi.fn(), disable: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import AiPanel from "./AiPanel.svelte";
import CapturePanel from "./CapturePanel.svelte";
import PageView from "./PageView.svelte";
import SetupChecklist from "./SetupChecklist.svelte";
import Toasts from "./Toasts.svelte";
import { deletions } from "../stores/deletions.svelte";
import { settings } from "../stores/settings.svelte";
import { system } from "../stores/system.svelte";
import { toast, ui } from "../stores/ui.svelte";
import type { Item, Settings } from "../types";

const mockInvoke = vi.mocked(invoke);

async function settle(ms = 5) {
  for (let i = 0; i < 6; i++) {
    flushSync();
    await new Promise((r) => setTimeout(r, ms));
  }
}

const baseItem: Item = {
  id: "01HAAAAAAAAAAAAAAAAAAAAAAA",
  bucket_id: null,
  url: "https://blog.example/post",
  canonical_url: "https://blog.example/post",
  title: "A post",
  description: null,
  site_name: null,
  kind: "article",
  favicon_path: null,
  image_path: null,
  notes_md: "",
  snapshot_status: "done",
  snapshot_error: null,
  snapshot_fetched_at: "2026-11-08T10:00:00Z",
  done_at: null,
  position: 0,
  created_at: "2026-11-01T10:00:00Z",
  updated_at: "2026-11-01T10:00:00Z",
};

const SETTINGS = {
  theme: "light",
  close_tab_after_capture: true,
  hotkey: "Ctrl+Cmd+S",
  ai_provider: "anthropic",
  ai_model: "claude-opus-5-5",
  whisper_model: "base.en",
  data_dir: "/x",
  extension_port: 1,
  extension_token: "t",
  ai_consent_given: true,
} as Settings;

let target: HTMLElement;
beforeEach(() => {
  mockInvoke.mockReset();
  handlers.clear();
  toast.clear();
  const mem = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => mem.get(k) ?? null,
    setItem: (k: string, v: string) => void mem.set(k, String(v)),
    removeItem: (k: string) => void mem.delete(k),
    clear: () => mem.clear(),
  });
  target = document.createElement("div");
  document.body.appendChild(target);
});
afterEach(() => target.remove());

describe("CapturePanel", () => {
  /** The capture window's command allowlist, straight from the capability file. */
  const CAPTURE_ALLOWED: string[] = (
    JSON.parse(readFileSync(resolve(process.cwd(), "src-tauri/capabilities/capture.json"), "utf8")) as { permissions: unknown[] }
  ).permissions
    .filter((p): p is string => typeof p === "string" && p.startsWith("allow-"))
    .map((p) => p.slice("allow-".length).replace(/-/g, "_"));

  function route(overrides: Record<string, (args: Record<string, unknown>) => unknown>) {
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      const a = (args ?? {}) as Record<string, unknown>;
      if (overrides[cmd]) return overrides[cmd](a);
      switch (cmd) {
        case "capture_settings":
          return { close_tab_after_capture: true, hotkey: "Ctrl+Cmd+S", theme: "light" };
        case "get_settings":
          throw { code: "internal", message: "get_settings is main-only" };
        case "list_buckets":
          return [{ id: "b1", name: "Watch later", emoji: "📺", archived_at: null, position: 0 }];
        default:
          return null;
      }
    });
  }

  it("Chrome tab → save → 'Saved to' then hides", async () => {
    route({
      capture_front_tab: () => ({ window_index: 1, tab_index: 1, url: "https://a.example/", title: "A" }),
      add_item: () => ({ item: { ...baseItem, bucket_id: null }, created: true }),
      close_front_tab: () => ({ closed: true }),
    });
    const app = mount(CapturePanel, { target });
    await settle();
    handlers.get("capture-open")?.({ payload: {} });
    await settle();
    expect((target.querySelector("#cap-url") as HTMLInputElement).value).toBe("https://a.example/");
    expect(target.textContent).toContain("Type to pick a bucket · ⏎ save · esc cancel");
    (Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Save") as HTMLButtonElement).click();
    await settle();
    expect(target.textContent).toContain("Saved to Inbox");
    // First close ever: the one-time explainer line.
    expect(target.textContent).toContain("Stow closes the tab after saving.");
    await settle(600);
    expect(mockInvoke.mock.calls.some((c) => c[0] === "hide_capture")).toBe(true);
    unmount(app);
  });

  it("duplicate keeps the panel open with Keep only (move_item is not allowed in the capture window)", async () => {
    route({
      capture_front_tab: () => ({ window_index: 1, tab_index: 1, url: "https://a.example/", title: "A" }),
      add_item: () => ({ item: { ...baseItem, bucket_id: "b1" }, created: false }),
    });
    const app = mount(CapturePanel, { target });
    await settle();
    handlers.get("capture-open")?.({ payload: {} });
    await settle();
    (Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Save") as HTMLButtonElement).click();
    await settle();
    expect(target.textContent).toContain("Already saved in 📺 Watch later");
    expect(target.textContent).toContain("Keep in 📺 Watch later");
    expect(target.textContent).not.toContain("Move to");
    expect(mockInvoke.mock.calls.some((c) => c[0] === "hide_capture")).toBe(false);
    expect(CAPTURE_ALLOWED).toContain("capture_settings");
    expect(CAPTURE_ALLOWED).not.toContain("get_settings");
    for (const c of mockInvoke.mock.calls) expect(CAPTURE_ALLOWED).toContain(c[0]);
    unmount(app);
  });

  it("reads capture_settings (never get_settings), applies its theme and saves the checkbox via set_close_tab_after_capture", async () => {
    document.documentElement.dataset.theme = "light";
    const sent: unknown[] = [];
    route({
      capture_settings: () => ({ close_tab_after_capture: true, hotkey: "Ctrl+Cmd+S", theme: "dark" }),
      set_close_tab_after_capture: (a) => {
        sent.push(a);
        return { close_tab_after_capture: a.value, hotkey: "Ctrl+Cmd+S", theme: "dark" };
      },
    });
    const app = mount(CapturePanel, { target });
    await settle();
    const names = mockInvoke.mock.calls.map((c) => c[0]);
    expect(names).toContain("capture_settings");
    expect(names).not.toContain("get_settings");
    expect(names).not.toContain("update_settings");
    expect(document.documentElement.dataset.theme).toBe("dark");
    const box = Array.from(target.querySelectorAll("label")).find((l) => l.textContent?.includes("Close tab in Chrome"))?.querySelector("input") as HTMLInputElement;
    expect(box.checked).toBe(true);
    box.click();
    await settle();
    expect(sent).toEqual([{ value: false }]);
    expect(box.checked).toBe(false);
    expect(mockInvoke.mock.calls.map((c) => c[0])).not.toContain("update_settings");
    for (const c of mockInvoke.mock.calls) expect(CAPTURE_ALLOWED).toContain(c[0]);
    document.documentElement.dataset.theme = "light";
    unmount(app);
  });

  it("tab not closed because the user switched tabs", async () => {
    localStorage.setItem("stow.capture.closeExplained", "1");
    route({
      capture_front_tab: () => ({ window_index: 1, tab_index: 1, url: "https://a.example/", title: "A" }),
      add_item: () => ({ item: baseItem, created: true }),
      close_front_tab: () => ({ closed: false }),
    });
    const app = mount(CapturePanel, { target });
    await settle();
    handlers.get("capture-open")?.({ payload: {} });
    await settle();
    (Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Save") as HTMLButtonElement).click();
    await settle();
    expect(target.textContent).toContain("Saved. The tab wasn't closed because you switched tabs.");
    unmount(app);
  });

  it("Chrome not running: clipboard prefill chip + Chrome-only line", async () => {
    route({
      capture_front_tab: () => Promise.reject({ code: "chrome_not_running", message: "chrome_not_running" }),
    });
    const app = mount(CapturePanel, { target });
    await settle();
    handlers.get("capture-open")?.({ payload: {} });
    await settle();
    expect((target.querySelector("#cap-url") as HTMLInputElement).value).toBe("https://clip.example/x");
    expect(target.textContent).toContain("From clipboard");
    unmount(app);
  });

  it("non-http Chrome page copy", async () => {
    route({ capture_front_tab: () => ({ window_index: 1, tab_index: 1, url: "chrome://newtab/", title: "New Tab" }) });
    const app = mount(CapturePanel, { target });
    await settle();
    handlers.get("capture-open")?.({ payload: {} });
    await settle();
    expect(target.textContent).toContain("This is a Chrome page (chrome://newtab/), which can't be saved. Paste a link instead.");
    unmount(app);
  });
});

describe("AiPanel: Add as tasks preview", () => {
  it("shows checkboxes, adds only checked ones and toasts 'Added N suggested tasks'", async () => {
    settings.set(SETTINGS);
    const added: string[] = [];
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      const a = (args ?? {}) as Record<string, string>;
      if (cmd === "has_api_key") return true;
      if (cmd === "list_ai_runs")
        return [
          {
            id: "r1",
            parent_type: "item",
            parent_id: baseItem.id,
            action: "suggest_tasks",
            provider: "anthropic",
            model: "m",
            input_hash: "",
            output_md: "- Read it\n- Share it\n- Archive it",
            status: "done",
            error: null,
            created_at: "2026-11-08T10:00:00Z",
          },
        ];
      if (cmd === "add_task") {
        added.push(a.title);
        return { id: `t${added.length}`, parent_type: "item", parent_id: baseItem.id, title: a.title, done_at: null, position: 0, created_at: "" };
      }
      if (cmd === "read_snapshot") return "words words";
      return [];
    });
    const app = mount(AiPanel, { target, props: { parentType: "item", parentId: baseItem.id, item: baseItem, appendToNotes: () => {} } });
    await settle();
    (Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.includes("Add as tasks")) as HTMLButtonElement).click();
    await settle();
    const boxes = target.querySelectorAll<HTMLInputElement>("fieldset input[type=checkbox]");
    expect(boxes.length).toBe(3);
    expect(Array.from(boxes).every((b) => b.checked)).toBe(true);
    boxes[1].click();
    await settle();
    const addBtn = Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Add 2 tasks") as HTMLButtonElement;
    expect(addBtn).toBeTruthy();
    addBtn.click();
    await settle();
    expect(added).toEqual(["Read it", "Archive it"]);
    expect(added.every((t) => !t.includes("(AI)"))).toBe(true);
    expect(ui.toasts.some((t) => t.message === "Added 2 suggested tasks")).toBe(true);
    unmount(app);
  });
});

describe("PageView", () => {
  it("renders snapshot images as links until Load images is ticked", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => (cmd === "read_snapshot" ? "Intro\n\n![chart](https://cdn.example/c.png)" : null));
    const app = mount(PageView, { target, props: { item: baseItem } });
    await settle();
    expect(target.querySelector("article img")).toBeNull();
    expect(target.querySelector("article a.img-link")?.getAttribute("href")).toBe("https://cdn.example/c.png");
    const box = Array.from(target.querySelectorAll("label")).find((l) => l.textContent?.includes("Load images"))?.querySelector("input") as HTMLInputElement;
    box.click();
    await settle();
    expect(target.querySelector("article img")?.getAttribute("src")).toBe("https://cdn.example/c.png");
    ui.setLoadImages(baseItem.id, false);
    unmount(app);
  });
});

describe("SetupChecklist", () => {
  it("shows the spec copy and counts done steps", async () => {
    system.reset();
    settings.set({ ...SETTINGS, hotkey: "Ctrl+Cmd+S", onboarding_steps: { extension: true } });
    mockInvoke.mockResolvedValue(null);
    const app = mount(SetupChecklist, { target });
    await settle();
    const text = target.textContent ?? "";
    expect(text).toContain("Welcome to Stow");
    expect(text).toContain("Get tabs out of Chrome and into buckets you'll actually come back to. Everything stays on this Mac.");
    expect(text).toContain("In Chrome, press ⌃⌘S. Pick a bucket, hit ⏎, and the tab closes.");
    expect(text).toContain("Needs a one-time 142 MB download.");
    expect(text).toContain("1 of 6 done");
    unmount(app);
  });
});

describe("Toasts", () => {
  it("errors show a Details disclosure; undo toasts have an Undo button", async () => {
    const app = mount(Toasts, { target });
    toast.error({ code: "io", message: "io error: No space left on device" });
    const commit = vi.fn();
    deletions.schedule("x1", "Thing", commit);
    flushSync();
    expect(target.querySelector("[role=alert]")?.textContent).toContain("out of disk space");
    expect(target.querySelector("details summary")?.textContent).toBe("Details");
    const undo = Array.from(target.querySelectorAll("button")).find((b) => b.textContent === "Undo") as HTMLButtonElement;
    undo.click();
    flushSync();
    expect(deletions.isHidden("x1")).toBe(false);
    expect(commit).not.toHaveBeenCalled();
    unmount(app);
  });
});
