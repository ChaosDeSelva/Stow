import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { deletions, UNDO_MS } from "./deletions.svelte";
import { toast, ui } from "./ui.svelte";

describe("undoable deletes", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    toast.clear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("hides at once and commits only after the undo window", async () => {
    const commit = vi.fn(async () => {});
    deletions.schedule("a", "Article", commit);
    expect(deletions.isHidden("a")).toBe(true);
    expect(ui.toasts[0].message).toBe("Deleted “Article”.");
    expect(ui.toasts[0].actions?.[0].label).toBe("Undo");
    vi.advanceTimersByTime(UNDO_MS - 1);
    expect(commit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(commit).toHaveBeenCalledTimes(1);
    await vi.runAllTimersAsync();
    expect(deletions.isHidden("a")).toBe(false);
  });

  it("Undo cancels the delete and shows the row again", () => {
    const commit = vi.fn();
    deletions.schedule("b", "B", commit);
    ui.toasts[0].actions?.[0].run();
    expect(deletions.isHidden("b")).toBe(false);
    vi.advanceTimersByTime(UNDO_MS * 2);
    expect(commit).not.toHaveBeenCalled();
  });

  it("flushAll commits everything pending immediately (quit / unload)", async () => {
    const c1 = vi.fn();
    const c2 = vi.fn();
    deletions.schedule("c1", "One", c1);
    deletions.schedule("c2", "Two", c2);
    await deletions.flushAll();
    expect(c1).toHaveBeenCalledTimes(1);
    expect(c2).toHaveBeenCalledTimes(1);
    expect(ui.toasts).toHaveLength(0);
    vi.advanceTimersByTime(UNDO_MS * 2);
    expect(c1).toHaveBeenCalledTimes(1);
  });

  it("scheduling the same id twice is a no-op", () => {
    const commit = vi.fn();
    deletions.schedule("d", "D", commit);
    deletions.schedule("d", "D", commit);
    expect(deletions.pendingCount).toBe(1);
    deletions.undo("d");
  });

  it("custom message (reminder Done)", () => {
    deletions.schedule("r", "Call mom", vi.fn(), { message: "Done: “Call mom”." });
    expect(ui.toasts[0].message).toBe("Done: “Call mom”.");
    deletions.undo("r");
  });
});

describe("batches", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    toast.clear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("hides every id, shows one toast and commits once; Undo restores all", () => {
    const commit = vi.fn();
    deletions.scheduleBatch(["x", "y", "z"], "Archived 3 items.", commit);
    expect(deletions.isHidden("x") && deletions.isHidden("y") && deletions.isHidden("z")).toBe(true);
    expect(ui.toasts).toHaveLength(1);
    expect(ui.toasts[0].message).toBe("Archived 3 items.");
    expect(deletions.pendingCount).toBe(1);
    deletions.undo("y");
    expect(deletions.isHidden("x") || deletions.isHidden("z")).toBe(false);
    vi.advanceTimersByTime(UNDO_MS * 2);
    expect(commit).not.toHaveBeenCalled();
  });

  it("commits one batch after the window and skips ids already pending", async () => {
    const commit = vi.fn(async () => {});
    deletions.schedule("p", "P", vi.fn());
    deletions.scheduleBatch(["p", "q"], "Deleted 2 items.", commit);
    expect(deletions.pendingCount).toBe(2);
    vi.advanceTimersByTime(UNDO_MS);
    expect(commit).toHaveBeenCalledTimes(1);
    await vi.runAllTimersAsync();
    expect(deletions.isHidden("q")).toBe(false);
  });
});
