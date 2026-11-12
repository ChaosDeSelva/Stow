// Global-shortcut helpers: key-capture → Tauri accelerator string, and
// accelerator → macOS glyphs (⌃⌥⇧⌘ order, as macOS menus show them).

export const DEFAULT_HOTKEY = "Ctrl+Cmd+S";

export interface KeyLike {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

export type CaptureResult =
  | { kind: "accelerator"; value: string }
  | { kind: "cancel" }
  | { kind: "reset" }
  /** Only modifiers so far, or a key without any modifier. */
  | { kind: "incomplete"; reason: "modifier-only" | "needs-modifier" };

const NAMED_CODES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
};

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn", "OS"]);

/** Physical key name for the accelerator (layout independent via `code`). */
export function keyName(code: string): string | null {
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1];
  m = /^Digit(\d)$/.exec(code);
  if (m) return m[1];
  m = /^(F\d{1,2})$/.exec(code);
  if (m) return m[1];
  return NAMED_CODES[code] ?? null;
}

/** Interpret one keydown while "Press keys…" is active. */
export function captureKey(e: KeyLike): CaptureResult {
  const anyMod = e.metaKey || e.ctrlKey || e.altKey || e.shiftKey;
  if (e.key === "Escape" && !anyMod) return { kind: "cancel" };
  if ((e.key === "Backspace" || e.key === "Delete") && !anyMod) return { kind: "reset" };
  if (MODIFIER_KEYS.has(e.key)) return { kind: "incomplete", reason: "modifier-only" };
  const k = keyName(e.code);
  if (!k) return { kind: "incomplete", reason: "modifier-only" };
  // Shift alone is not enough for a global shortcut (it would eat capital letters).
  if (!(e.metaKey || e.ctrlKey || e.altKey)) return { kind: "incomplete", reason: "needs-modifier" };
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Cmd");
  parts.push(k);
  return { kind: "accelerator", value: parts.join("+") };
}

const GLYPH_ORDER = ["⌃", "⌥", "⇧", "⌘"];
const MOD_GLYPH: Record<string, string> = {
  ctrl: "⌃",
  control: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
  cmd: "⌘",
  command: "⌘",
  super: "⌘",
  meta: "⌘",
  cmdorctrl: "⌘",
  commandorcontrol: "⌘",
};
const KEY_GLYPH: Record<string, string> = {
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  enter: "↩",
  tab: "⇥",
  space: "Space",
  backspace: "⌫",
};

/** "Ctrl+Cmd+S" → "⌃⌘S"; unknown strings come back unchanged. */
export function hotkeyGlyphs(acc: string | null | undefined): string {
  if (!acc) return "";
  const parts = acc.split("+").map((p) => p.trim()).filter(Boolean);
  if (!parts.length) return acc;
  const mods = new Set<string>();
  let key = "";
  for (const p of parts) {
    const g = MOD_GLYPH[p.toLowerCase()];
    if (g) mods.add(g);
    else key = KEY_GLYPH[p.toLowerCase()] ?? (p.length === 1 ? p.toUpperCase() : p);
  }
  return GLYPH_ORDER.filter((g) => mods.has(g)).join("") + key;
}
