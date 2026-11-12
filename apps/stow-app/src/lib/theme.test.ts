import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyTheme, initTheme, resolvedTheme, THEME_CACHE_KEY } from "./theme";

type Listener = (e: { matches: boolean }) => void;
let listeners: Listener[] = [];
let systemDark = false;

function mockStorage() {
  const m = new Map<string, string>();
  const store = {
    getItem: (k: string) => (m.has(k) ? m.get(k)! : null),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  };
  Object.defineProperty(window, "localStorage", { configurable: true, value: store });
  return store;
}

function mockMatchMedia() {
  listeners = [];
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (q: string) => ({
      media: q,
      get matches() {
        return systemDark;
      },
      addEventListener: (_: string, l: Listener) => listeners.push(l),
      removeEventListener: (_: string, l: Listener) => (listeners = listeners.filter((x) => x !== l)),
    }),
  });
}

describe("theme", () => {
  beforeEach(() => {
    systemDark = false;
    mockMatchMedia();
    mockStorage();
    delete document.documentElement.dataset.theme;
  });
  afterEach(() => applyTheme("light"));

  it("initTheme defaults to light before settings load", () => {
    initTheme();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("initTheme uses the cached resolved value to avoid a flash", () => {
    window.localStorage.setItem(THEME_CACHE_KEY, "dark");
    initTheme();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("applyTheme sets light/dark directly and caches the resolved value", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(resolvedTheme()).toBe("dark");
    expect(window.localStorage.getItem(THEME_CACHE_KEY)).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("applyTheme('system') follows matchMedia and tracks changes", () => {
    systemDark = true;
    applyTheme("system");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(listeners).toHaveLength(1);
    systemDark = false;
    listeners[0]({ matches: false });
    expect(document.documentElement.dataset.theme).toBe("light");
    // switching away from system detaches the listener
    applyTheme("light");
    expect(listeners).toHaveLength(0);
  });

  it("ignores unknown values (keeps light)", () => {
    applyTheme("purple" as unknown as "light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
