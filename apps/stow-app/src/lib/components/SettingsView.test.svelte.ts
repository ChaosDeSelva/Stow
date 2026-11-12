import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";

// `invoke` THROWS synchronously by default (as it does when the Tauri runtime is absent).
// A one-shot load must not turn into an effect loop that spams toasts and freezes the renderer.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => {
    throw new TypeError("Cannot read properties of undefined (reading 'invoke')");
  }),
  convertFileSrc: (p: string) => p,
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(), readText: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(async () => false), requestPermission: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: vi.fn(async () => false), enable: vi.fn(), disable: vi.fn() }));

import SettingsView from "./SettingsView.svelte";
import { invoke } from "@tauri-apps/api/core";
import { settings } from "../stores/settings.svelte";
import { toast, ui } from "../stores/ui.svelte";
import type { Settings } from "../types";

const mockInvoke = vi.mocked(invoke);

const SETTINGS: Settings = {
  theme: "light",
  close_tab_after_capture: true,
  hotkey: "Ctrl+Cmd+S",
  ai_provider: "anthropic",
  ai_model: "claude-opus-5-5",
  whisper_model: "base.en",
  data_dir: "/Users/me/Library/Application Support/Stow",
  extension_port: 4848,
  extension_token: "tok",
  hotkey_registered: false,
  hotkey_error: "already in use",
};

async function settle() {
  for (let i = 0; i < 5; i++) {
    flushSync();
    await new Promise((r) => setTimeout(r, 5));
  }
  flushSync();
}

describe("SettingsView", () => {
  let target: HTMLElement;
  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    toast.clear();
    settings.set(null);
    mockInvoke.mockClear();
  });
  afterEach(() => target.remove());

  it("loads once and does not loop when every command throws", async () => {
    const app = mount(SettingsView, { target });
    await settle();
    const calls = mockInvoke.mock.calls.length;
    // get_settings + has_api_key×2 + permission_status + app_info
    expect(calls).toBeLessThanOrEqual(6);
    await settle();
    expect(mockInvoke.mock.calls.length).toBe(calls);
    expect(ui.toasts.length).toBeLessThanOrEqual(3);
    unmount(app);
  });

  it("with resolving commands: a fixed set of one-shot calls, then nothing more", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "get_settings":
          return SETTINGS;
        case "has_api_key":
          return false;
        case "permission_status":
          return { automation: "granted", microphone: "not_determined" };
        case "app_info":
          return { name: "Stow", version: "0.1.0", data_dir: "/x" };
        case "pairing_code":
          return "4848:tok";
        case "whisper_model_downloaded":
          throw "unknown command";
        default:
          return null;
      }
    });
    const app = mount(SettingsView, { target });
    await settle();
    const names = mockInvoke.mock.calls.map((c) => c[0]).sort();
    // whisper_model_downloaded once per known model (the danger zone's "(N MB)").
    expect(names).toEqual(
      ["app_info", "get_settings", "has_api_key", "has_api_key", "pairing_code", "permission_status", "whisper_model_downloaded", "whisper_model_downloaded", "whisper_model_downloaded"].sort(),
    );
    await settle();
    expect(mockInvoke.mock.calls.length).toBe(names.length);

    // Sections in the documented order.
    const headings = Array.from(target.querySelectorAll("section > h2")).map((h) => h.textContent?.trim());
    expect(headings).toEqual(["General", "Capture", "Permissions", "Chrome extension", "AI", "Transcription", "Data"]);
    // Hotkey conflict warning copy.
    expect(target.textContent).toContain("Couldn't use ⌃⌘S — another app already uses it. Pick a different shortcut.");
    // Pairing code field and status rows.
    expect((target.querySelector("#pairing") as HTMLInputElement).value).toBe("4848:tok");
    expect(target.textContent).toContain("Allowed");
    expect(target.textContent).toContain("Not asked yet");
    // Whisper status hidden when the app can't tell.
    expect(Array.from(target.querySelectorAll(".ok-text")).some((e) => e.textContent?.trim() === "Downloaded")).toBe(false);
    // At most one primary button.
    expect(target.querySelectorAll("button.primary").length).toBeLessThanOrEqual(1);
    // Models unknown: the delete button has no size and the About footer falls back to the version.
    expect(target.textContent).toContain("Delete downloaded models");
    expect(target.textContent).not.toMatch(/Delete downloaded models \(\d+ MB\)/);
    expect(target.querySelector("footer.about")?.textContent).toContain("Stow 0.1.0");
    unmount(app);
  });

  function resolving(extra: Record<string, (args: Record<string, unknown>) => unknown> = {}) {
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      const a = (args ?? {}) as Record<string, unknown>;
      if (extra[cmd]) return extra[cmd](a);
      switch (cmd) {
        case "get_settings":
          return SETTINGS;
        case "has_api_key":
          return false;
        case "permission_status":
          return { automation: "granted", microphone: "granted" };
        case "app_info":
          return {
            name: "Stow",
            version: "0.1.0",
            data_dir: "/x",
            build: { version: "0.1.0", commit: "abcdef0123456789", built_at: "2026-11-08T09:30:00Z" },
          };
        case "pairing_code":
          return "4848:tok";
        case "whisper_model_downloaded":
          return a.model === "base.en" || a.model === "tiny.en";
        default:
          return null;
      }
    });
  }
  const button = (text: string) => Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement | undefined;

  it("About shows version, commit and build time from app_info().build", async () => {
    resolving();
    const app = mount(SettingsView, { target });
    await settle();
    const about = target.querySelector("footer.about")?.textContent ?? "";
    expect(about).toContain("Stow 0.1.0");
    expect(about).toContain("abcdef012");
    expect(about).toContain("built ");
    expect(about).toContain("2026");
    unmount(app);
  });

  it("danger zone: 'Delete downloaded models (N MB)' calls delete_whisper_models and reports the bytes freed", async () => {
    const calls: string[] = [];
    resolving({
      delete_whisper_models: () => {
        calls.push("delete_whisper_models");
        return { deleted_bytes: 227_000_000 };
      },
    });
    const app = mount(SettingsView, { target });
    await settle();
    // tiny.en (75) + base.en (142) on disk.
    const del = button("Delete downloaded models (217 MB)");
    expect(del).toBeTruthy();
    del!.click();
    await settle();
    expect(calls).toEqual([]); // not before confirming
    (Array.from(target.querySelectorAll("button")).find((b) => b.textContent?.trim() === "Delete") as HTMLButtonElement).click(); // ConfirmInline's confirm
    await settle();
    expect(calls).toEqual(["delete_whisper_models"]);
    expect(ui.toasts.some((t) => t.message.includes("216 MB freed"))).toBe(true);
    expect(button("No downloaded models")?.disabled).toBe(true);
    unmount(app);
  });

  it("danger zone: 'Delete all Stow data…' needs the typed word delete, then calls reset_all_data and blocks the UI", async () => {
    const resets: unknown[] = [];
    resolving({
      reset_all_data: (a) => {
        resets.push(a);
        return null;
      },
    });
    const app = mount(SettingsView, { target });
    await settle();
    button("Delete all Stow data…")!.click();
    await settle();
    const input = target.querySelector<HTMLInputElement>("input[aria-label='Type delete to confirm']")!;
    const go = () => button("Delete everything")!;
    expect(go().disabled).toBe(true);
    input.value = "Delete me";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(go().disabled).toBe(true);
    go().click();
    await settle();
    expect(resets).toEqual([]);
    expect(target.querySelector(".resetting")).toBeNull();

    input.value = " delete ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(go().disabled).toBe(false);
    go().click();
    await settle();
    expect(resets).toEqual([{ confirm: "delete" }]);
    expect(mockInvoke.mock.calls.filter((c) => c[0] === "reset_all_data").length).toBe(1);
    const overlay = target.querySelector(".resetting");
    expect(overlay?.getAttribute("role")).toBe("alertdialog");
    expect(overlay?.textContent).toContain("Resetting… Stow will restart.");
    unmount(app);
  });

  it("danger zone: a rejected reset_all_data drops the overlay and toasts", async () => {
    resolving({
      reset_all_data: () => Promise.reject({ code: "io", message: "could not clear the data folder: busy" }),
    });
    const app = mount(SettingsView, { target });
    await settle();
    button("Delete all Stow data…")!.click();
    await settle();
    const input = target.querySelector<HTMLInputElement>("input[aria-label='Type delete to confirm']")!;
    input.value = "delete";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    button("Delete everything")!.click();
    await settle();
    expect(target.querySelector(".resetting")).toBeNull();
    expect(ui.toasts.some((t) => t.kind === "error")).toBe(true);
    unmount(app);
  });
});
