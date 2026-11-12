import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import HotkeyRecorder from "./HotkeyRecorder.svelte";

function press(el: Element, init: KeyboardEventInit) {
  el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  flushSync();
}

describe("HotkeyRecorder", () => {
  let target: HTMLElement;
  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
  });
  afterEach(() => target.remove());

  function btn() {
    return Array.from(target.querySelectorAll("button")).find((b) => /Record shortcut|Press keys/.test(b.textContent ?? ""))!;
  }

  it("records a combination with a modifier and applies it", async () => {
    const onApply = vi.fn(async () => null);
    const app = mount(HotkeyRecorder, { target, props: { value: "Ctrl+Cmd+S", onApply } });
    flushSync();
    expect(target.querySelector("kbd")?.textContent).toBe("⌃⌘S");
    btn().click();
    flushSync();
    expect(btn().textContent).toContain("Press keys…");
    press(btn(), { key: "k", code: "KeyK" });
    expect(target.textContent).toContain("Add ⌃, ⌥ or ⌘");
    expect(onApply).not.toHaveBeenCalled();
    press(btn(), { key: "k", code: "KeyK", altKey: true, metaKey: true });
    expect(onApply).toHaveBeenCalledWith("Alt+Cmd+K");
    unmount(app);
  });

  it("Esc cancels, ⌫ resets to ⌃⌘S", () => {
    const onApply = vi.fn(async () => null);
    const app = mount(HotkeyRecorder, { target, props: { value: "Alt+Space", onApply } });
    flushSync();
    btn().click();
    flushSync();
    press(btn(), { key: "Escape", code: "Escape" });
    expect(btn().textContent).toContain("Record shortcut");
    expect(onApply).not.toHaveBeenCalled();
    btn().click();
    flushSync();
    press(btn(), { key: "Backspace", code: "Backspace" });
    expect(onApply).toHaveBeenCalledWith("Ctrl+Cmd+S");
    unmount(app);
  });

  it("a rejected apply shows the error and keeps the previous shortcut", async () => {
    const onApply = vi.fn(async () => "That shortcut can't be used. Try another combination.");
    const app = mount(HotkeyRecorder, { target, props: { value: "Ctrl+Cmd+S", onApply, registered: true } });
    flushSync();
    btn().click();
    flushSync();
    press(btn(), { key: "k", code: "KeyK", altKey: true, metaKey: true });
    await Promise.resolve();
    await Promise.resolve();
    flushSync();
    expect(onApply).toHaveBeenCalledWith("Alt+Cmd+K");
    expect(target.querySelector("kbd")?.textContent).toBe("⌃⌘S");
    expect(target.textContent).toContain("That shortcut can't be used. Try another combination. Still using ⌃⌘S.");
    expect(target.textContent).not.toContain("Works everywhere");
    // Recording again clears the stale error.
    btn().click();
    flushSync();
    expect(target.textContent).not.toContain("Still using");
    unmount(app);
  });

  it("shows the conflict warning when the shortcut didn't register", () => {
    const app = mount(HotkeyRecorder, { target, props: { value: "Cmd+Space", onApply: vi.fn(), registered: false } });
    flushSync();
    expect(target.textContent).toContain("Couldn't use ⌘Space — another app already uses it. Pick a different shortcut.");
    unmount(app);
  });
});
