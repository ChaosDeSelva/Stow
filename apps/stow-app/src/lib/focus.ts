// Focus helpers shared by dialogs, menus and popovers.

const FOCUSABLE =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

export function focusables(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => !el.hasAttribute("inert"));
}

/** Keep Tab inside `root`; call from a keydown handler. Returns true when handled. */
export function trapTab(e: KeyboardEvent, root: HTMLElement | null): boolean {
  if (e.key !== "Tab" || !root) return false;
  const f = focusables(root);
  if (!f.length) {
    e.preventDefault();
    root.focus();
    return true;
  }
  const first = f[0];
  const last = f[f.length - 1];
  const active = document.activeElement as HTMLElement | null;
  if (e.shiftKey && (active === first || !root.contains(active))) {
    e.preventDefault();
    last.focus();
    return true;
  }
  if (!e.shiftKey && (active === last || !root.contains(active))) {
    e.preventDefault();
    first.focus();
    return true;
  }
  return false;
}

/** Remember the focused element now; the returned function puts focus back (if it is still in the document). */
export function rememberFocus(): () => void {
  const prev = document.activeElement as HTMLElement | null;
  return () => {
    if (prev && document.contains(prev) && typeof prev.focus === "function") prev.focus();
  };
}

/** Index after an arrow/Home/End key in a list of `n` (wrapping), or null for other keys. */
export function rovingIndex(key: string, i: number, n: number, orientation: "vertical" | "horizontal" = "vertical"): number | null {
  if (n <= 0) return null;
  const next = orientation === "vertical" ? "ArrowDown" : "ArrowRight";
  const prev = orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
  if (key === next) return (i + 1 + n) % n;
  if (key === prev) return (i - 1 + n) % n;
  if (key === "Home") return 0;
  if (key === "End") return n - 1;
  return null;
}
