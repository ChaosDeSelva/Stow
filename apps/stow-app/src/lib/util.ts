// Small shared helpers.
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

/** "www.example.com" → "example.com"; "" for unparsable URLs. */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** "item:01H…" → ["item", "01H…"]. */
export function splitKey(key: string): [string, string] {
  const i = key.indexOf(":");
  return i < 0 ? [key, ""] : [key.slice(0, i), key.slice(i + 1)];
}

/** True inside the Tauri webview (false in a plain browser / tests without a runtime). */
export function inTauri(): boolean {
  try {
    return isTauri();
  } catch {
    return false;
  }
}

/** asset:// URL for a local file, or null outside Tauri / for empty paths. */
export function fileSrc(path: string | null | undefined): string | null {
  if (!path || !inTauri()) return null;
  try {
    return convertFileSrc(path);
  } catch {
    return null;
  }
}

/** Read a localStorage flag without throwing. */
export function lsGet(key: string): string | null {
  try {
    return window.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}
export function lsSet(key: string, value: string | null) {
  try {
    if (value == null) window.localStorage?.removeItem(key);
    else window.localStorage?.setItem(key, value);
  } catch {
    /* storage unavailable */
  }
}

export function isEditable(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return (el as HTMLElement).isContentEditable;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** "Saved 3 tabs. 2 were already in Stow or weren't web pages." */
export function importSummary(imported: number, skipped: number): string {
  const a = `Saved ${imported} tab${imported === 1 ? "" : "s"}.`;
  if (!skipped) return a;
  return skipped === 1
    ? `${a} 1 was already in Stow or wasn't a web page.`
    : `${a} ${skipped} were already in Stow or weren't web pages.`;
}
