import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import { invoke } from "@tauri-apps/api/core";
import * as api from "./api";

const mockInvoke = vi.mocked(invoke);

describe("api wrappers (v0.1 contract, old-backend tolerant)", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("addItem normalises a bare Item from older backends", async () => {
    const item = { id: "1", url: "https://a" };
    mockInvoke.mockResolvedValueOnce(item);
    expect(await api.addItem("https://a")).toEqual({ item, created: true });
  });

  it("addItem passes {item, created} through", async () => {
    const r = { item: { id: "1" }, created: false };
    mockInvoke.mockResolvedValueOnce(r);
    expect(await api.addItem("https://a", "b1", "T")).toEqual(r);
    expect(mockInvoke).toHaveBeenCalledWith("add_item", { url: "https://a", bucketId: "b1", title: "T" });
  });

  it("closeFrontTab normalises void to {closed: true}", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    expect(await api.closeFrontTab("https://a")).toEqual({ closed: true });
    mockInvoke.mockResolvedValueOnce({ closed: false });
    expect(await api.closeFrontTab("https://a")).toEqual({ closed: false });
  });

  it("listItems only sends includeDone when asked", async () => {
    mockInvoke.mockResolvedValue([]);
    await api.listItems({ kind: "inbox" });
    expect(mockInvoke).toHaveBeenLastCalledWith("list_items", { filter: { kind: "inbox" } });
    await api.listItems({ kind: "inbox" }, true);
    expect(mockInvoke).toHaveBeenLastCalledWith("list_items", { filter: { kind: "inbox" }, includeDone: true });
  });

  it("a synchronous invoke throw becomes a rejection", async () => {
    mockInvoke.mockImplementationOnce(() => {
      throw new TypeError("no runtime");
    });
    await expect(api.getSettings()).rejects.toThrow("no runtime");
  });

  it("pairingCodeOr falls back to port:token", async () => {
    mockInvoke.mockRejectedValueOnce("unknown command");
    const s = { extension_port: 4848, extension_token: "tok" } as Parameters<typeof api.pairingCodeOr>[0];
    expect(await api.pairingCodeOr(s)).toBe("4848:tok");
  });

  it("subscribe swallows listen failures and cleans up", () => {
    const off = api.subscribe("item-added", () => {});
    expect(typeof off).toBe("function");
    off();
  });
});
