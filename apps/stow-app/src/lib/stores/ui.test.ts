import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ERROR_TOAST_MS, MAX_TOASTS, TOAST_MS, toast, ui } from "./ui.svelte";

describe("toasts", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    toast.clear();
    toast.suppressErrors(false);
  });
  afterEach(() => {
    toast.clear();
    vi.useRealTimers();
  });

  it("error toasts use friendly copy and keep the raw message as detail", () => {
    toast.error({ code: "db", message: "database error: locked" });
    expect(ui.toasts[0].message).toMatch(/couldn't save that change/);
    expect(ui.toasts[0].detail).toBe("database error: locked");
  });

  it("identical messages fold into one toast with a counter", () => {
    toast.error("chrome_not_running");
    toast.error("chrome_not_running");
    toast.error("chrome_not_running");
    expect(ui.toasts).toHaveLength(1);
    expect(ui.toasts[0].count).toBe(3);
  });

  it("at most three are visible; the oldest goes first", () => {
    for (let i = 0; i < 5; i++) toast.info(`m${i}`);
    expect(ui.toasts).toHaveLength(MAX_TOASTS);
    expect(ui.toasts.map((t) => t.message)).toEqual(["m2", "m3", "m4"]);
  });

  it("errors auto-dismiss after 8 s, others after 4 s", () => {
    toast.error("boom");
    toast.info("hi");
    vi.advanceTimersByTime(TOAST_MS);
    expect(ui.toasts.map((t) => t.message)).toEqual(["boom"]);
    vi.advanceTimersByTime(ERROR_TOAST_MS - TOAST_MS);
    expect(ui.toasts).toHaveLength(0);
  });

  it("duration 0 is sticky and onClose fires on dismiss", () => {
    const onClose = vi.fn();
    const id = toast.info("sticky", { duration: 0, onClose });
    vi.advanceTimersByTime(60_000);
    expect(ui.toasts).toHaveLength(1);
    toast.dismiss(id);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("suppressErrors drops error toasts (browser preview)", () => {
    toast.suppressErrors(true);
    toast.error("x");
    expect(ui.toasts).toHaveLength(0);
  });

  it("highlight marks a row for 2 s", () => {
    ui.highlight("row1");
    expect(ui.isHighlighted("row1")).toBe(true);
    vi.advanceTimersByTime(2000);
    expect(ui.isHighlighted("row1")).toBe(false);
  });
});

describe("navigation helpers", () => {
  it("reveal opens an item in its bucket, the Inbox, or Archived when it is archived", () => {
    ui.reveal({ id: "x", bucket_id: "b1", done_at: null });
    expect(ui.selection).toEqual({ kind: "bucket", id: "b1" });
    expect(ui.selectedItemId).toBe("x");
    ui.reveal({ id: "y", bucket_id: "b1", done_at: "2026-11-01T00:00:00Z" });
    expect(ui.selection).toEqual({ kind: "archived" });
    expect(ui.selectedItemId).toBe("y");
    ui.reveal({ id: "z", bucket_id: null });
    expect(ui.selection).toEqual({ kind: "inbox" });
  });

  it("jumping to another list clears the multi-selection and closes Add link", () => {
    ui.select({ kind: "inbox" });
    ui.setSelectedIds(new Set(["a", "b"]));
    ui.openAddLink();
    ui.selectBucketAndItem("b9", "i1");
    expect(ui.selectedIds.size).toBe(0);
    expect(ui.addLinkOpen).toBe(false);
    expect(ui.selectedItemId).toBe("i1");
  });

  it("every openAddLink bumps the focus tick (⌘L on an open form refocuses it)", () => {
    const t = ui.addLinkFocusTick;
    ui.openAddLink();
    ui.openAddLink();
    expect(ui.addLinkFocusTick).toBe(t + 2);
  });
});
