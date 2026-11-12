import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}`, isTauri: () => false }));

import { fileSrc, hostOf, humanSize, importSummary, splitKey } from "./util";

describe("util", () => {
  it("importSummary copy", () => {
    expect(importSummary(3, 2)).toBe("Saved 3 tabs. 2 were already in Stow or weren't web pages.");
    expect(importSummary(1, 1)).toBe("Saved 1 tab. 1 was already in Stow or wasn't a web page.");
    expect(importSummary(4, 0)).toBe("Saved 4 tabs.");
  });
  it("fileSrc is null outside Tauri (no convertFileSrc crash)", () => {
    expect(fileSrc("/a.png")).toBeNull();
    expect(fileSrc(null)).toBeNull();
  });
  it("hostOf / splitKey / humanSize", () => {
    expect(hostOf("https://www.example.com/x")).toBe("example.com");
    expect(hostOf("nope")).toBe("");
    expect(splitKey("item:01H:x")).toEqual(["item", "01H:x"]);
    expect(humanSize(1536)).toBe("1.5 KB");
  });
});
