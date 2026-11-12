import { describe, expect, it } from "vitest";
import { rangeIds, selectAll, toggleId } from "./selection";

const rows = ["a", "b", "c", "d", "e"];

describe("multi-select helpers", () => {
  it("toggleId adds then removes without mutating the input", () => {
    const s0 = new Set<string>();
    const s1 = toggleId(s0, "b");
    expect([...s1]).toEqual(["b"]);
    expect(s0.size).toBe(0);
    expect([...toggleId(s1, "b")]).toEqual([]);
  });

  it("rangeIds selects the inclusive span in either direction and keeps earlier picks", () => {
    expect([...rangeIds(rows, "b", "d")].sort()).toEqual(["b", "c", "d"]);
    expect([...rangeIds(rows, "d", "b")].sort()).toEqual(["b", "c", "d"]);
    expect([...rangeIds(rows, "e", "e", new Set(["a"]))].sort()).toEqual(["a", "e"]);
  });

  it("rangeIds without a usable anchor selects just the target; unknown target is a no-op", () => {
    expect([...rangeIds(rows, null, "c")]).toEqual(["c"]);
    expect([...rangeIds(rows, "zz", "c")]).toEqual(["c"]);
    expect([...rangeIds(rows, "a", "zz")]).toEqual([]);
  });

  it("selectAll takes every visible row", () => {
    expect([...selectAll(rows)]).toEqual(rows);
    expect(selectAll([]).size).toBe(0);
  });
});
