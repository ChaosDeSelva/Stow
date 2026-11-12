import { beforeEach, describe, expect, it, vi } from "vitest";

const writeText = vi.fn();
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: (t: string) => writeText(t) }));

import { copyText } from "./clipboard";

describe("copyText", () => {
  beforeEach(() => writeText.mockReset());

  it("uses the Tauri clipboard plugin first", async () => {
    writeText.mockResolvedValueOnce(undefined);
    const navWrite = vi.fn();
    Object.defineProperty(navigator, "clipboard", { value: { writeText: navWrite }, configurable: true });
    await expect(copyText("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
    expect(navWrite).not.toHaveBeenCalled();
  });

  it("falls back to navigator.clipboard when the plugin fails", async () => {
    writeText.mockRejectedValueOnce(new Error("not allowed"));
    const navWrite = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText: navWrite }, configurable: true });
    await expect(copyText("hello")).resolves.toBe(true);
    expect(navWrite).toHaveBeenCalledWith("hello");
  });

  it("rejects when both paths fail", async () => {
    writeText.mockRejectedValueOnce(new Error("plugin"));
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn(async () => { throw new Error("denied"); }) },
      configurable: true,
    });
    await expect(copyText("x")).rejects.toThrow();
  });
});
