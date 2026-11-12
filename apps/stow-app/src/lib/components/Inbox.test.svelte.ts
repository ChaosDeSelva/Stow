// Mounted flows for the main window: the Add link form (single + batch, ⌘L),
// row Archive / Restore buttons with their undo toasts, and the "Get set up" view.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn(), ask: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(), readText: vi.fn(async () => "see https://clip.example/x") }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(async () => false), requestPermission: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: vi.fn(async () => false), enable: vi.fn(), disable: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import MainLayout from "./MainLayout.svelte";
import { buckets } from "../stores/buckets.svelte";
import { deletions } from "../stores/deletions.svelte";
import { items } from "../stores/items.svelte";
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

const BUCKET = { id: "b1", name: "Watch later", emoji: "📺", archived_at: null, position: 0, notes_md: "", created_at: "", updated_at: "" };

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
  onboarding_steps: { extension: true },
} as Settings;

/** Backend stub: `list` is the item table; add_item / set_item_done are recorded by the mock itself. */
function route(list: Item[], overrides: Record<string, (args: Record<string, unknown>) => unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    const a = (args ?? {}) as Record<string, unknown>;
    if (overrides[cmd]) return overrides[cmd](a);
    switch (cmd) {
      case "list_buckets":
        return [BUCKET];
      case "list_items": {
        const f = a.filter as { kind: string; id?: string };
        return list.filter((i) => (f.kind === "all" ? true : f.kind === "inbox" ? i.bucket_id == null : i.bucket_id === f.id));
      }
      case "get_settings":
        return SETTINGS;
      case "update_settings":
        return { ...SETTINGS, ...(a.patch as object) };
      case "set_item_done": {
        const it = list.find((i) => i.id === a.id);
        return it ? { ...it, done_at: a.done ? "2026-11-08T12:00:00Z" : null } : null;
      }
      case "add_item":
        return { item: { ...baseItem, id: `new-${String(a.url).length}`, url: String(a.url), bucket_id: (a.bucketId as string) ?? null }, created: true };
      default:
        return [];
    }
  });
}

const buttonNamed = (root: ParentNode, text: string) =>
  Array.from(root.querySelectorAll("button")).find((b) => b.textContent?.trim() === text) as HTMLButtonElement | undefined;
const byLabel = (root: ParentNode, label: string) => root.querySelector(`[aria-label="${label}"]`) as HTMLButtonElement | null;
const calls = (cmd: string) => mockInvoke.mock.calls.filter((c) => c[0] === cmd).map((c) => c[1] as Record<string, unknown>);

function typeInto(el: HTMLTextAreaElement | HTMLInputElement, value: string) {
  el.value = value;
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

let target: HTMLElement;
beforeEach(async () => {
  // Commit undo windows left by the previous test against its own backend stub.
  await deletions.flushAll();
  mockInvoke.mockReset();
  toast.clear();
  items.reset();
  system.reset();
  settings.set({ ...SETTINGS });
  ui.select({ kind: "inbox" });
  ui.closeAddLink();
  ui.clearSelectedIds();
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

describe("Add link", () => {
  it("header button opens the form, pre-filled from the clipboard; Save calls add_item and toasts 'Saved to Inbox'", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    expect(target.textContent).toContain("Inbox is where new links land before you sort them into a bucket.");
    expect(target.textContent).toContain("Unsorted links");
    const add = buttonNamed(target.querySelector(".list-header")!, "Add link")!;
    expect(add.classList.contains("primary")).toBe(true);
    expect(add.title).toContain("⌘L");
    add.click();
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(field).toBeTruthy();
    expect(field.value).toBe("https://clip.example/x");
    expect(target.textContent).toContain("From clipboard");
    typeInto(field, "https://a.example/x");
    await settle();
    expect(target.textContent).toContain("1 link");
    typeInto(target.querySelector("#addlink-title") as HTMLInputElement, "Hello");
    buttonNamed(target.querySelector(".addlink")!, "Save")!.click();
    await settle();
    expect(calls("add_item")).toEqual([{ url: "https://a.example/x", title: "Hello" }]);
    expect(ui.toasts.map((t) => t.message)).toContain("Saved to Inbox");
    expect(target.querySelector("#addlink-url")).toBeNull(); // closed after a clean save
    expect(items.list.some((i) => i.url === "https://a.example/x")).toBe(true);
    unmount(app);
  });

  it("tells how many pasted lines were skipped because they were not web addresses", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    buttonNamed(target.querySelector(".list-header")!, "Add link")!.click();
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(target.textContent).not.toContain("Ignored");
    typeInto(field, "https://a.example/x\nsome note I typed\nexample.com\nhttps://b.example/y");
    await settle();
    expect(target.textContent).toContain("2 links");
    expect(target.textContent).toContain("Ignored 2 lines that weren't web addresses");
    typeInto(field, "https://a.example/x\nnot-a-link");
    await settle();
    expect(target.textContent).toContain("Ignored 1 line that wasn't a web address");
    typeInto(field, "https://a.example/x, https://b.example/y");
    await settle();
    expect(target.textContent).not.toContain("Ignored");
    unmount(app);
  });

  it("⌘L opens the form from anywhere in the main window; Esc closes it", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    expect(target.querySelector("#addlink-url")).toBeNull();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "l", metaKey: true, bubbles: true }));
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(field).toBeTruthy();
    expect(document.activeElement).toBe(field);
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settle();
    expect(target.querySelector("#addlink-url")).toBeNull();
    unmount(app);
  });

  it("many URLs: one add_item per link into the picked bucket, title hidden, one summary toast", async () => {
    route([]);
    await buckets.load();
    const app = mount(MainLayout, { target });
    await settle();
    ui.select({ kind: "bucket", id: "b1" });
    await settle();
    expect(target.textContent).toContain("No links in 📺 Watch later yet.");
    buttonNamed(target.querySelector(".list-header")!, "Add link")!.click();
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    typeInto(field, "https://a.com, https://b.com\nhttps://c.com");
    await settle();
    expect(target.textContent).toContain("3 links");
    expect(target.querySelector("#addlink-title")).toBeNull();
    expect(field.rows).toBe(2);
    buttonNamed(target.querySelector(".addlink")!, "Save 3")!.click();
    await settle();
    expect(calls("add_item")).toEqual([
      { url: "https://a.com", bucketId: "b1" },
      { url: "https://b.com", bucketId: "b1" },
      { url: "https://c.com", bucketId: "b1" },
    ]);
    expect(ui.toasts).toHaveLength(1);
    expect(ui.toasts[0].message).toBe("Saved 3 to 📺 Watch later");
    unmount(app);
  });

  it("summarises duplicates and failures", async () => {
    route([], {
      add_item: (a) => {
        if (String(a.url).includes("dup")) return { item: { ...baseItem, id: "d", url: String(a.url) }, created: false };
        if (String(a.url).includes("bad")) return Promise.reject({ code: "http", message: "boom" });
        return { item: { ...baseItem, id: "n", url: String(a.url) }, created: true };
      },
    });
    const app = mount(MainLayout, { target });
    await settle();
    ui.openAddLink();
    await settle();
    typeInto(target.querySelector("#addlink-url") as HTMLTextAreaElement, "https://ok.com https://dup.com https://bad.com");
    await settle();
    buttonNamed(target.querySelector(".addlink")!, "Save 3")!.click();
    await settle();
    expect(ui.toasts).toHaveLength(1);
    expect(ui.toasts[0].kind).toBe("error");
    expect(ui.toasts[0].message).toBe("Saved 1 to Inbox · 1 already in Stow · Couldn't save 1 (see Details)");
    expect(ui.toasts[0].detail).toContain("https://bad.com: Couldn't reach the site.");
    // The failed link stays in the field for another try.
    expect((target.querySelector("#addlink-url") as HTMLTextAreaElement).value).toBe("https://bad.com");
    unmount(app);
  });
});

describe("Archive / Restore", () => {
  it("row Archive button hides the row, shows 'Archived “…”' with Undo, then calls set_item_done(id, true)", async () => {
    route([baseItem]);
    const app = mount(MainLayout, { target });
    await settle();
    expect(target.querySelector(`[data-id="${baseItem.id}"]`)).toBeTruthy();
    byLabel(target, "Archive A post")!.click();
    await settle();
    expect(target.querySelector(`[data-id="${baseItem.id}"]`)).toBeNull();
    expect(ui.toasts[0].message).toBe("Archived “A post”.");
    expect(ui.toasts[0].actions?.[0].label).toBe("Undo");
    expect(calls("set_item_done")).toEqual([]);
    await deletions.flushAll();
    expect(calls("set_item_done")).toEqual([{ id: baseItem.id, done: true }]);
    unmount(app);
  });

  it("Archived view lists done items with Restore → set_item_done(id, false)", async () => {
    const done = { ...baseItem, id: "done1", title: "Old post", done_at: "2026-11-02T00:00:00Z" };
    route([baseItem, done]);
    const app = mount(MainLayout, { target });
    await settle();
    buttonNamed(target, "Archived")!.click();
    await settle();
    expect(target.querySelector(`[data-id="done1"]`)).toBeTruthy();
    expect(target.querySelector(`[data-id="${baseItem.id}"]`)).toBeNull();
    expect(target.textContent).toContain("Archived · 1");
    byLabel(target, "Restore Old post")!.click();
    await settle();
    expect(calls("set_item_done")).toEqual([{ id: "done1", done: false }]);
    expect(ui.toasts[0].message).toBe("Restored “Old post”.");
    unmount(app);
  });

  it("⌘A selects every row and the bulk bar archives them with one undo toast", async () => {
    const two = { ...baseItem, id: "two", title: "Second" };
    route([baseItem, two]);
    const app = mount(MainLayout, { target });
    await settle();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", metaKey: true, bubbles: true }));
    await settle();
    expect(target.textContent).toContain("2 selected");
    buttonNamed(target.querySelector(".bulkbar")!, "Archive")!.click();
    await settle();
    expect(ui.toasts).toHaveLength(1);
    expect(ui.toasts[0].message).toBe("Archived 2 items.");
    expect(deletions.isHidden(baseItem.id) && deletions.isHidden("two")).toBe(true);
    ui.toasts[0].actions![0].run();
    await settle();
    expect(deletions.isHidden(baseItem.id)).toBe(false);
    expect(target.querySelectorAll("[data-id]").length).toBe(2);
    unmount(app);
  });
});

describe("Get set up", () => {
  it("sidebar entry above Inbox with N/6; the view holds the checklist and mentions Add link", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    const nav = Array.from(target.querySelectorAll("nav .navitem")).map((b) => b.textContent?.trim() ?? "");
    expect(nav[0]).toContain("Get set up");
    expect(nav[0]).toContain("1/6");
    expect(nav[1]).toContain("Inbox");
    expect(target.textContent).not.toContain("Welcome to Stow");
    (target.querySelector("nav .navitem") as HTMLButtonElement).click();
    await settle();
    expect(ui.selection.kind).toBe("setup");
    expect(target.textContent).toContain("Welcome to Stow");
    expect(target.textContent).toContain("…or click Add link");
    expect(target.textContent).toContain("1 of 6 done");
    buttonNamed(target, "Dismiss checklist")!.click();
    await settle();
    expect(calls("update_settings")[0]).toEqual({ patch: { onboarding_dismissed: true } });
    expect(ui.selection.kind).toBe("inbox");
    expect(target.textContent).not.toContain("Get set up");
    unmount(app);
  });
});

describe("QA pass: list state, keys and focus", () => {
  const A = { ...baseItem, id: "a1", title: "Alpha", created_at: "2026-11-03T10:00:00Z" };
  const B = { ...baseItem, id: "b2", title: "Bravo", created_at: "2026-11-02T10:00:00Z" };
  const C = { ...baseItem, id: "c3", title: "Charlie", created_at: "2026-11-01T10:00:00Z" };
  const rowIds = () => Array.from(target.querySelectorAll<HTMLElement>("[role=listbox] [data-id]")).map((r) => r.dataset.id);
  const row = (id: string) => target.querySelector<HTMLElement>(`[data-id="${id}"]`)!;
  const key = (el: EventTarget, k: string, mods: KeyboardEventInit = {}) =>
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...mods }));

  it("archived row leaves at once; Undo puts it back in the same position", async () => {
    route([A, B, C]);
    const app = mount(MainLayout, { target });
    await settle();
    expect(rowIds()).toEqual(["a1", "b2", "c3"]);
    byLabel(target, "Archive Bravo")!.click();
    await settle();
    expect(rowIds()).toEqual(["a1", "c3"]);
    expect(target.textContent).toContain("Inbox · 2");
    ui.toasts[0].actions![0].run();
    await settle();
    expect(rowIds()).toEqual(["a1", "b2", "c3"]);
    expect(calls("set_item_done")).toEqual([]);
    unmount(app);
  });

  it("deleted row leaves at once; Undo re-inserts it in place", async () => {
    route([A, B, C]);
    const app = mount(MainLayout, { target });
    await settle();
    byLabel(target, "Delete Alpha")!.click();
    await settle();
    expect(rowIds()).toEqual(["b2", "c3"]);
    ui.toasts[0].actions![0].run();
    await settle();
    expect(rowIds()).toEqual(["a1", "b2", "c3"]);
    unmount(app);
  });

  it("after the undo window the Archived view shows the archived row", async () => {
    const table = [A, B];
    route(table, {
      set_item_done: (a) => {
        const i = table.findIndex((x) => x.id === a.id);
        table[i] = { ...table[i], done_at: a.done ? "2026-11-08T12:00:00Z" : null };
        return table[i];
      },
    });
    const app = mount(MainLayout, { target });
    await settle();
    byLabel(target, "Archive Alpha")!.click();
    await settle();
    await deletions.flushAll();
    await settle();
    expect(rowIds()).toEqual(["b2"]);
    ui.select({ kind: "archived" });
    await settle();
    expect(rowIds()).toEqual(["a1"]);
    unmount(app);
  });

  it("A on the focused row archives it and keeps focus on the next row", async () => {
    route([A, B, C]);
    const app = mount(MainLayout, { target });
    await settle();
    row("b2").focus();
    await settle();
    key(row("b2"), "a");
    await settle();
    expect(rowIds()).toEqual(["a1", "c3"]);
    expect(document.activeElement).toBe(row("c3"));
    expect(ui.selectedItemId).toBe("c3");
    unmount(app);
  });

  it("A / D / ⌫ / ⌘A do nothing while typing; ⌘L does nothing in an editable", async () => {
    route([A, B]);
    const app = mount(MainLayout, { target });
    await settle();
    // A text field inside the list pane (bubbles through the listbox's handler).
    const input = document.createElement("input");
    row("a1").appendChild(input);
    input.focus();
    for (const k of ["a", "d", "Backspace", "Delete"]) key(input, k);
    key(input, "a", { metaKey: true });
    await settle();
    expect(ui.toasts.map((t) => t.message)).toEqual([]);
    expect(ui.selectedIds.size).toBe(0);
    key(input, "l", { metaKey: true });
    await settle();
    expect(ui.addLinkOpen).toBe(false);
    input.remove();
    // ⌘A inside the Add link textarea selects its text, not the rows.
    ui.openAddLink();
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(key(field, "a", { metaKey: true })).toBe(true); // not prevented
    await settle();
    expect(ui.selectedIds.size).toBe(0);
    unmount(app);
  });

  it("letter keys inside a row's menu don't archive the row", async () => {
    route([A]);
    const app = mount(MainLayout, { target });
    await settle();
    row("a1").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 }));
    await settle();
    const menu = target.querySelector("[role=menu]") as HTMLElement;
    expect(menu).toBeTruthy();
    key(menu, "d");
    await settle();
    expect(rowIds()).toEqual(["a1"]);
    expect(ui.toasts).toHaveLength(0);
    unmount(app);
  });

  it("Esc precedence: Add link form, then menu, then the multi-selection", async () => {
    route([A, B]);
    const app = mount(MainLayout, { target });
    await settle();
    ui.setSelectedIds(new Set(["a1", "b2"]));
    ui.openAddLink();
    await settle();
    // Esc from the list row: the form closes first, the selection stays.
    row("a1").focus();
    key(row("a1"), "Escape");
    await settle();
    expect(ui.addLinkOpen).toBe(false);
    expect(ui.selectedIds.size).toBe(2);
    // Esc inside an open menu closes only the menu.
    row("a1").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 }));
    await settle();
    key(target.querySelector("[role=menu]")!, "Escape");
    await settle();
    expect(target.querySelector("[role=menu]")).toBeNull();
    expect(ui.selectedIds.size).toBe(2);
    // Then Esc clears the selection.
    key(row("a1"), "Escape");
    await settle();
    expect(ui.selectedIds.size).toBe(0);
    unmount(app);
  });

  it("Esc inside the form closes it and returns focus to the header's Add link button", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    const add = target.querySelector<HTMLButtonElement>("[data-addlink-toggle]")!;
    add.focus();
    add.click();
    await settle();
    const field = target.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(document.activeElement).toBe(field);
    expect(field.placeholder).toBe("Paste one or more links — commas, spaces or new lines are fine");
    // ⌘L again while open (focus moved away): back into the field.
    add.focus();
    key(window, "l", { metaKey: true });
    await settle();
    expect(document.activeElement).toBe(field);
    key(field, "Escape");
    await settle();
    expect(target.querySelector("#addlink-url")).toBeNull();
    expect(document.activeElement).toBe(add);
    unmount(app);
  });

  it("⌘-click a second row keeps the open one; rows expose aria-selected; live region says N selected", async () => {
    route([A, B, C]);
    const app = mount(MainLayout, { target });
    await settle();
    row("a1").click();
    row("c3").dispatchEvent(new MouseEvent("click", { bubbles: true, metaKey: true }));
    await settle();
    expect([...ui.selectedIds].sort()).toEqual(["a1", "c3"]);
    expect(row("a1").getAttribute("aria-selected")).toBe("true");
    expect(row("b2").getAttribute("aria-selected")).toBe("false");
    expect(row("c3").getAttribute("aria-selected")).toBe("true");
    expect(target.querySelector(".sr-only[role=status][aria-live=polite]")?.textContent).toBe("2 selected");
    // Bulk Delete: both leave, one Undo brings both back in place.
    buttonNamed(target.querySelector(".bulkbar")!, "Delete")!.click();
    await settle();
    expect(rowIds()).toEqual(["b2"]);
    ui.toasts[0].actions![0].run();
    await settle();
    expect(rowIds()).toEqual(["a1", "b2", "c3"]);
    unmount(app);
  });

  it("Archived view: the bulk bar offers Restore and Delete only; M does not open Move", async () => {
    const d1 = { ...A, done_at: "2026-11-05T00:00:00Z" };
    const d2 = { ...B, done_at: "2026-11-05T00:00:00Z" };
    route([d1, d2, C]);
    const app = mount(MainLayout, { target });
    await settle();
    ui.select({ kind: "archived" });
    await settle();
    key(window, "a", { metaKey: true });
    await settle();
    const labels = Array.from(target.querySelectorAll(".bulkbar button")).map((b) => b.textContent?.trim());
    expect(labels).toEqual(["Restore", "Delete", "Clear"]);
    row("a1").focus();
    key(row("a1"), "m");
    await settle();
    expect(target.querySelector("[aria-label='Move to bucket']")).toBeNull();
    unmount(app);
  });

  it("Move for a batch from several buckets ticks no bucket", async () => {
    const inB = { ...B, bucket_id: "b1" };
    route([A, inB]);
    await buckets.load();
    const app = mount(MainLayout, { target });
    await settle();
    ui.select({ kind: "all" });
    await settle();
    key(window, "a", { metaKey: true });
    await settle();
    buttonNamed(target.querySelector(".bulkbar")!, "Move to…")!.click();
    await settle();
    const pop = target.querySelector("[aria-label='Move to bucket']")!;
    expect(pop.textContent).toContain("Move 2 to… (from several buckets)");
    (pop.querySelector("input") as HTMLInputElement).focus();
    await settle();
    expect(pop.querySelectorAll("[role=option] [aria-label='Current'], [role=option] .tick").length).toBe(0);
    unmount(app);
  });

  it("batch with duplicates: 'Show them' (secondary) selects the first duplicate in its own list", async () => {
    const existing = { ...baseItem, id: "dupe1", url: "https://dup.com/", bucket_id: "b1" };
    route([existing], {
      add_item: (a) =>
        String(a.url).includes("dup")
          ? { item: existing, created: false }
          : { item: { ...baseItem, id: "n1", url: String(a.url) }, created: true },
    });
    await buckets.load();
    const app = mount(MainLayout, { target });
    await settle();
    ui.openAddLink();
    await settle();
    typeInto(target.querySelector("#addlink-url") as HTMLTextAreaElement, "https://ok.com https://dup.com/");
    await settle();
    buttonNamed(target.querySelector(".addlink")!, "Save 2")!.click();
    await settle();
    expect(ui.toasts[0].message).toBe("Saved 1 to Inbox · 1 already in Stow");
    const action = ui.toasts[0].actions![0];
    expect(action.label).toBe("Show them");
    expect(action.secondary).toBe(true);
    const btn = buttonNamed(document.body, "Show them");
    if (btn) expect(btn.classList.contains("ghost")).toBe(true);
    action.run();
    await settle();
    expect(ui.selection).toEqual({ kind: "bucket", id: "b1" });
    expect(ui.selectedItemId).toBe("dupe1");
    unmount(app);
  });

  it("single duplicate names the bucket it is really in, with 'Show it'", async () => {
    const existing = { ...baseItem, id: "dupe1", url: "https://dup.com/", bucket_id: "b1" };
    route([existing], { add_item: () => ({ item: existing, created: false }) });
    await buckets.load();
    const app = mount(MainLayout, { target });
    await settle();
    ui.openAddLink();
    await settle();
    typeInto(target.querySelector("#addlink-url") as HTMLTextAreaElement, "https://dup.com/");
    await settle();
    buttonNamed(target.querySelector(".addlink")!, "Save")!.click();
    await settle();
    expect(ui.toasts[0].message).toBe("Already in 📺 Watch later");
    expect(ui.toasts[0].actions![0].label).toBe("Show it");
    unmount(app);
  });

  it("the detail pane's Open original is secondary: one primary button in the window", async () => {
    route([A]);
    const app = mount(MainLayout, { target });
    await settle();
    row("a1").click();
    await settle();
    const open = buttonNamed(target.querySelector(".detail")!, "Open original")!;
    expect(open.classList.contains("primary")).toBe(false);
    expect(target.querySelectorAll("button.primary").length).toBe(1);
    unmount(app);
  });
});

describe("QA pass: Get set up", () => {
  it("sidebar badge and checklist footer count the same steps (persisted + live)", async () => {
    route([baseItem]);
    settings.set({ ...SETTINGS, onboarding_steps: { extension: true, whisper: true } });
    const app = mount(MainLayout, { target });
    await settle();
    await items.refreshCounts();
    await settle();
    const badge = target.querySelector("nav .navitem .count")!.textContent;
    expect(badge).toBe("3/6"); // extension + whisper persisted, captured live (1 item)
    ui.select({ kind: "setup" });
    await settle();
    expect(target.textContent).toContain("3 of 6 done");
    unmount(app);
  });

  it("the entry stays visible while its view is open even when every step is done", async () => {
    route([baseItem]);
    settings.set({
      ...SETTINGS,
      onboarding_steps: { captured: true, automation: true, notifications: true, extension: true, ai_key: true, whisper: true },
    });
    const app = mount(MainLayout, { target });
    await settle();
    expect(target.querySelector("nav")!.textContent).not.toContain("Get set up");
    ui.select({ kind: "setup" });
    await settle();
    expect(target.querySelector("nav")!.textContent).toContain("Get set up");
    expect(target.textContent).toContain("6 of 6 done");
    unmount(app);
  });

  it("Settings → Show setup checklist turns it back on and opens the view", async () => {
    route([], {
      get_settings: () => ({ ...SETTINGS, onboarding_dismissed: true }),
      update_settings: (a) => ({ ...SETTINGS, onboarding_dismissed: true, ...(a.patch as object) }),
    });
    settings.set({ ...SETTINGS, onboarding_dismissed: true });
    const app = mount(MainLayout, { target });
    await settle();
    ui.select({ kind: "settings" });
    await settle();
    const box = Array.from(target.querySelectorAll("label.check")).find((l) => l.textContent?.includes("Show setup checklist"))!
      .querySelector("input") as HTMLInputElement;
    expect(box.checked).toBe(false);
    box.click();
    await settle();
    expect(calls("update_settings")).toContainEqual({ patch: { onboarding_dismissed: false } });
    expect(ui.selection.kind).toBe("setup");
    expect(target.textContent).toContain("Welcome to Stow");
    unmount(app);
  });
});

describe("Add link form layout", () => {
  it("the URL field is sans-serif (no .mono) and the hint / count / chip sit in one nowrap row", async () => {
    route([]);
    const app = mount(MainLayout, { target });
    await settle();
    ui.openAddLink();
    await settle();
    const form = target.querySelector(".addlink")!;
    const field = form.querySelector("#addlink-url") as HTMLTextAreaElement;
    expect(field.classList.contains("mono")).toBe(false);
    expect(form.querySelector(".mono")).toBeNull();
    // No computed-font check: component CSS isn't loaded under Vitest, so jsdom (31+) reports
    // its own user-agent default for <textarea> (monospace). The .mono checks above are the
    // contract; app.css (`font: inherit`) and the component style set the sans face.
    expect(field.getAttribute("placeholder")).toContain("Paste one or more links");
    // Row 2: count, clipboard chip and hint are siblings of one row (never stacked).
    const meta = form.querySelector(".meta")!;
    expect(meta.querySelector(".count")!.textContent).toBe("1 link");
    const chip = meta.querySelector(".chip.clip")!;
    expect(chip.textContent).toBe("From clipboard");
    expect(chip.classList.contains("accent")).toBe(true);
    expect(meta.querySelector(".hint")!.textContent).toBe("⏎ save · ⇧⏎ new line · esc cancel");
    // Row 4: the picker is its own full-width row with the long placeholder.
    const picker = form.querySelector(".picker input[role=combobox]") as HTMLInputElement;
    expect(picker.getAttribute("placeholder")).toBe("Type to pick a bucket…");
    expect(picker.closest(".buttons")).toBeNull();
    // Row 5: Cancel / Save in their own row, after the picker.
    const buttons = form.querySelector(".buttons")!;
    expect(Array.from(buttons.querySelectorAll("button")).map((b) => b.textContent?.trim())).toEqual(["Cancel", "Save"]);
    expect(buttons.previousElementSibling).toBe(form.querySelector(".picker"));
    unmount(app);
  });

  it("bucket picker options render [icon] [label] [check] as siblings; the current row's check is last", async () => {
    route([]);
    await buckets.load();
    const app = mount(MainLayout, { target });
    await settle();
    ui.openAddLink();
    await settle();
    const picker = target.querySelector(".addlink .picker input[role=combobox]") as HTMLInputElement;
    picker.focus();
    picker.dispatchEvent(new FocusEvent("focus"));
    await settle();
    const options = Array.from(target.querySelectorAll(".addlink [role=option]"));
    expect(options.map((o) => o.textContent?.trim())).toEqual(["Inbox", "📺Watch later"]);
    const inbox = options[0];
    const name = inbox.querySelector(".name")!;
    const tick = inbox.querySelector(".tick")!;
    expect(tick).toBeTruthy();
    expect(name.parentElement).toBe(inbox);
    expect(tick.parentElement).toBe(inbox);
    expect(name.compareDocumentPosition(tick) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(inbox.lastElementChild).toBe(tick);
    expect(name.classList.contains("truncate")).toBe(true);
    expect(tick.querySelector("[aria-label='Current']")).toBeTruthy();
    // The unselected bucket row has the same shape minus the check.
    expect(options[1].querySelector(".name.truncate")).toBeTruthy();
    expect(options[1].querySelector(".tick")).toBeNull();
    unmount(app);
  });
});
