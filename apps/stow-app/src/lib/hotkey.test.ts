import { describe, expect, it } from "vitest";
import { captureKey, hotkeyGlyphs, keyName } from "./hotkey";

const key = (k: string, code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key: k,
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("captureKey", () => {
  it("builds an accelerator in ⌃⌥⇧⌘ order", () => {
    expect(captureKey(key("s", "KeyS", { ctrlKey: true, metaKey: true }))).toEqual({ kind: "accelerator", value: "Ctrl+Cmd+S" });
    expect(captureKey(key("1", "Digit1", { altKey: true, shiftKey: true }))).toEqual({ kind: "accelerator", value: "Alt+Shift+1" });
    expect(captureKey(key(" ", "Space", { metaKey: true }))).toEqual({ kind: "accelerator", value: "Cmd+Space" });
  });

  it("requires at least one of ⌃⌥⌘", () => {
    expect(captureKey(key("s", "KeyS"))).toEqual({ kind: "incomplete", reason: "needs-modifier" });
    expect(captureKey(key("S", "KeyS", { shiftKey: true }))).toEqual({ kind: "incomplete", reason: "needs-modifier" });
  });

  it("ignores lone modifiers", () => {
    expect(captureKey(key("Meta", "MetaLeft", { metaKey: true })).kind).toBe("incomplete");
  });

  it("Esc cancels and ⌫ resets", () => {
    expect(captureKey(key("Escape", "Escape"))).toEqual({ kind: "cancel" });
    expect(captureKey(key("Backspace", "Backspace"))).toEqual({ kind: "reset" });
  });

  it("uses the physical key, not the layout character", () => {
    expect(captureKey(key("ß", "KeyS", { altKey: true, metaKey: true }))).toEqual({ kind: "accelerator", value: "Alt+Cmd+S" });
    expect(keyName("BracketLeft")).toBe("[");
    expect(keyName("IntlRo")).toBeNull();
  });
});

describe("hotkeyGlyphs", () => {
  it.each([
    ["Ctrl+Cmd+S", "⌃⌘S"],
    ["CmdOrCtrl+Shift+S", "⇧⌘S"],
    ["Cmd+Alt+Ctrl+Shift+K", "⌃⌥⇧⌘K"],
    ["Alt+Space", "⌥Space"],
    ["Cmd+Up", "⌘↑"],
  ])("%s → %s", (acc, glyphs) => {
    expect(hotkeyGlyphs(acc)).toBe(glyphs);
  });
});
