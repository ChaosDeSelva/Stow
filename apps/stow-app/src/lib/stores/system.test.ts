import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(async () => true),
  requestPermission: vi.fn(async () => "granted"),
}));
vi.mock("@tauri-apps/plugin-autostart", () => ({
  isEnabled: vi.fn(async () => false),
  enable: vi.fn(async () => {}),
  disable: vi.fn(async () => {}),
}));

import { invoke } from "@tauri-apps/api/core";
import { permissionLabel, system } from "./system.svelte";

const mockInvoke = vi.mocked(invoke);

describe("system status", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    system.reset();
  });

  it("concurrent key refreshes don't overwrite each other", async () => {
    let releaseA: (v: boolean) => void = () => {};
    mockInvoke.mockImplementation((_cmd, args) => {
      const a = (args as { account: string }).account;
      return a === "anthropic" ? new Promise((r) => (releaseA = r as (v: boolean) => void)) : Promise.resolve(true);
    });
    const pa = system.refreshKey("anthropic");
    await system.refreshKey("openai");
    releaseA(false);
    await pa;
    expect(system.hasKey).toEqual({ anthropic: false, openai: true });
  });

  it("missing permission_status degrades to unknown, notifications still read", async () => {
    mockInvoke.mockRejectedValue("unknown command permission_status");
    await system.refreshPermissions();
    expect(system.automation).toBeNull();
    expect(system.notifications).toBe(true);
  });

  it("askAutomation stores the prompted result", async () => {
    mockInvoke.mockResolvedValueOnce("granted");
    expect(await system.askAutomation()).toBe("granted");
    expect(system.automation).toBe("granted");
  });

  it("askMicrophone stores the status string; a non-string result is ignored", async () => {
    mockInvoke.mockResolvedValueOnce("denied");
    expect(await system.askMicrophone()).toBe("denied");
    expect(system.microphone).toBe("denied");
    // An old backend returning the whole permission_status object must not poison the state.
    mockInvoke.mockResolvedValueOnce({ automation: "granted", microphone: "granted" });
    expect(await system.askMicrophone()).toBeNull();
    expect(system.microphone).toBe("denied");
  });

  it("setLoginItem mirrors the plugin", async () => {
    expect(await system.setLoginItem(true)).toBe(true);
    expect(system.loginItem).toBe(true);
  });

  it("permissionLabel", () => {
    expect(permissionLabel("granted").text).toBe("Allowed");
    expect(permissionLabel("denied").text).toBe("Not allowed");
    expect(permissionLabel("not_determined").text).toBe("Not asked yet");
    expect(permissionLabel(null).text).toBe("Not asked yet");
    expect(permissionLabel(true).tone).toBe("ok");
  });
});
