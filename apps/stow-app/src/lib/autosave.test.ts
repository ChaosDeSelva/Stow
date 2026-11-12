import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAutosave, flushAll, registerAutosave } from "./autosave";

describe("createAutosave", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("debounces and saves the latest value once", () => {
    const save = vi.fn();
    const a = createAutosave(save, 600);
    a.schedule("a");
    a.schedule("ab");
    a.schedule("abc");
    vi.advanceTimersByTime(599);
    expect(save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith("abc");
  });

  it("flush saves the pending value immediately, exactly once", () => {
    const save = vi.fn();
    const a = createAutosave(save, 600);
    a.schedule("draft");
    a.schedule("draft 2");
    a.flush();
    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith("draft 2");
    vi.advanceTimersByTime(1000);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("flush with nothing pending does not call save", () => {
    const save = vi.fn();
    const a = createAutosave(save, 600);
    a.flush();
    expect(save).not.toHaveBeenCalled();
  });

  it("cancel drops the pending value", () => {
    const save = vi.fn();
    const a = createAutosave(save, 600);
    a.schedule("x");
    a.cancel();
    vi.advanceTimersByTime(1000);
    a.flush();
    expect(save).not.toHaveBeenCalled();
  });

  it("does not re-save an unchanged value after a flush", () => {
    const save = vi.fn();
    const a = createAutosave(save, 600);
    a.schedule("same");
    a.flush();
    a.schedule("same");
    vi.advanceTimersByTime(1000);
    expect(save).toHaveBeenCalledTimes(1);
  });
});

describe("autosave registry (flushAll)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("flushAll flushes every registered autosave once and skips unregistered ones", () => {
    const a = vi.fn();
    const b = vi.fn();
    const c = vi.fn();
    const sa = createAutosave(a, 600);
    const sb = createAutosave(b, 600);
    const sc = createAutosave(c, 600);
    const offA = registerAutosave(sa);
    registerAutosave(sb);
    const offC = registerAutosave(sc);
    sa.schedule("A");
    sb.schedule("B");
    sc.schedule("C");
    offC(); // unmounted editor: it flushed itself on destroy in real life
    flushAll();
    expect(a).toHaveBeenCalledWith("A");
    expect(b).toHaveBeenCalledWith("B");
    expect(c).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
    offA();
    sa.schedule("A2");
    flushAll();
    expect(a).toHaveBeenCalledTimes(1);
  });

  it("flush returns the save's promise so a caller can await the write", async () => {
    let done!: () => void;
    const save = vi.fn(() => new Promise<void>((r) => (done = r)));
    const a = createAutosave(save, 600);
    a.schedule("draft");
    const p = a.flush();
    expect(p).toBeInstanceOf(Promise);
    let settled = false;
    void Promise.resolve(p).then(() => (settled = true));
    await Promise.resolve();
    expect(settled).toBe(false);
    done();
    await p;
    expect(settled).toBe(true);
    expect(a.flush()).toBeUndefined();
  });

  it("flushAll returns every pending save's promise; allSettled waits for all of them", async () => {
    const resolvers: (() => void)[] = [];
    const slow = vi.fn(() => new Promise<void>((r) => resolvers.push(r)));
    const sync = vi.fn();
    const sa = createAutosave(slow, 600);
    const sb = createAutosave(slow, 600);
    const sc = createAutosave(sync, 600);
    const offs = [registerAutosave(sa), registerAutosave(sb), registerAutosave(sc)];
    sa.schedule("A");
    sb.schedule("B");
    sc.schedule("C");
    const promises = flushAll();
    expect(promises).toHaveLength(2);
    expect(slow).toHaveBeenCalledTimes(2);
    expect(sync).toHaveBeenCalledWith("C");
    let settled = false;
    void Promise.allSettled(promises).then(() => (settled = true));
    await Promise.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);
    resolvers[0]();
    await Promise.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);
    resolvers[1]();
    await Promise.allSettled(promises);
    expect(settled).toBe(true);
    offs.forEach((off) => off());
  });

  it("flushAll with nothing pending is a no-op", () => {
    const a = vi.fn();
    const off = registerAutosave(createAutosave(a, 600));
    expect(flushAll()).toEqual([]);
    expect(a).not.toHaveBeenCalled();
    off();
  });
});
