import { describe, expect, it } from "vitest";
import { dayBounds, formPresets, formatRelative, fromLocalInputValue, nextMonday, thisWeekend, toLocalInputValue, presets } from "./time";

describe("dayBounds", () => {
  it("covers the full local day as RFC3339 UTC", () => {
    const date = new Date(2026, 10, 8, 15, 30, 0); // local Nov 8 2026 15:30
    const { start, end } = dayBounds(date);
    const s = new Date(start);
    const e = new Date(end);
    expect(start.endsWith("Z")).toBe(true);
    expect(end.endsWith("Z")).toBe(true);
    expect(s.getFullYear()).toBe(2026);
    expect(s.getMonth()).toBe(10);
    expect(s.getDate()).toBe(8);
    expect(s.getHours()).toBe(0);
    expect(s.getMinutes()).toBe(0);
    expect(s.getSeconds()).toBe(0);
    // end is the start of the next local day
    expect(e.getTime() - s.getTime()).toBeGreaterThanOrEqual(23 * 3600 * 1000);
    expect(e.getTime() - s.getTime()).toBeLessThanOrEqual(25 * 3600 * 1000);
    expect(e.getHours()).toBe(0);
    expect(e.getDate()).toBe(9);
    // the given moment is inside the bounds
    expect(date.getTime()).toBeGreaterThanOrEqual(s.getTime());
    expect(date.getTime()).toBeLessThan(e.getTime());
  });
});

describe("datetime-local conversion", () => {
  it("round-trips through the input value format", () => {
    const iso = new Date(2026, 0, 5, 9, 7, 0).toISOString();
    const v = toLocalInputValue(iso);
    expect(v).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);
    expect(v).toBe("2026-01-05T09:07");
    expect(fromLocalInputValue(v)).toBe(iso);
  });

  it("returns null for an empty input value", () => {
    expect(fromLocalInputValue("")).toBeNull();
  });
});

describe("formatRelative", () => {
  it("describes past and future moments", () => {
    const now = new Date(2026, 10, 8, 12, 0, 0);
    expect(formatRelative(new Date(now.getTime() - 5 * 60_000).toISOString(), now)).toBe("5 min ago");
    expect(formatRelative(new Date(now.getTime() + 2 * 3600_000).toISOString(), now)).toBe("in 2 h");
    expect(formatRelative(new Date(now.getTime() + 20_000).toISOString(), now)).toBe("now");
  });
});

describe("presets", () => {
  it("produces future RFC3339 UTC moments", () => {
    const now = new Date(2026, 10, 8, 12, 0, 0);
    const list = presets(now);
    expect(list.map((p) => p.label.replace(/ .*/, ""))).toEqual(["+1h", "Tonight", "Tomorrow", "Next"]);
    for (const p of list) {
      expect(new Date(p.fireAt).getTime()).toBeGreaterThan(now.getTime());
      expect(p.fireAt.endsWith("Z")).toBe(true);
    }
    const tomorrow = new Date(list[2].fireAt);
    expect(tomorrow.getDate()).toBe(9);
    expect(tomorrow.getHours()).toBe(9);
  });

  it("skips 'tonight' when it is already past 19:00", () => {
    const late = new Date(2026, 10, 8, 21, 0, 0);
    expect(presets(late).map((p) => p.label)).not.toContain("Tonight");
    expect(presets(late)).toHaveLength(3);
  });
});

describe("form presets", () => {
  it("next week is 7 days out at 9:00", () => {
    const now = new Date(2026, 10, 8, 12, 0, 0);
    const nw = new Date(presets(now).find((p) => p.label === "Next week")!.fireAt);
    expect(nw.getDate()).toBe(15);
    expect(nw.getHours()).toBe(9);
  });

  it("next Monday is never today", () => {
    const monday = new Date(2026, 10, 9, 8, 0, 0); // Mon Nov 9 2026
    const nm = nextMonday(monday);
    expect(nm.getDay()).toBe(1);
    expect(nm.getDate()).toBe(16);
    expect(nextMonday(new Date(2026, 10, 8, 12)).getDate()).toBe(9); // from Sunday
  });

  it("this weekend is the coming Saturday 10:00, or Sunday when Saturday has passed", () => {
    const wed = new Date(2026, 10, 11, 12, 0, 0);
    const w = thisWeekend(wed);
    expect(w.getDay()).toBe(6);
    expect(w.getHours()).toBe(10);
    const satNoon = new Date(2026, 10, 14, 12, 0, 0);
    expect(thisWeekend(satNoon).getDay()).toBe(0);
  });

  it("form presets add This weekend and Next Monday", () => {
    const labels = formPresets(new Date(2026, 10, 8, 12)).map((p) => p.label);
    expect(labels).toContain("This weekend");
    expect(labels).toContain("Next Monday");
  });
});
