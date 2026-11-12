// Appearance: settings.theme is the source of truth ('light' | 'dark' | 'system').
// The resolved value lands on <html data-theme="light|dark">; app.css styles dark
// tokens only under [data-theme="dark"], so light is the default and there is
// never a flash of the OS theme.

export type ThemeSetting = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

/** Only a cache of the last resolved value, to avoid a flash before get_settings resolves. */
export const THEME_CACHE_KEY = "stow.theme.resolved";

let mq: MediaQueryList | null = null;
let onChange: ((e: { matches: boolean }) => void) | null = null;

function set(resolved: ResolvedTheme) {
  document.documentElement.dataset.theme = resolved;
  try {
    window.localStorage?.setItem(THEME_CACHE_KEY, resolved);
  } catch {
    /* storage unavailable */
  }
}

function detach() {
  if (mq && onChange) mq.removeEventListener("change", onChange);
  mq = null;
  onChange = null;
}

export function resolvedTheme(): ResolvedTheme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

/** Called before mount: cached value or light. */
export function initTheme() {
  let cached: string | null = null;
  try {
    cached = window.localStorage?.getItem(THEME_CACHE_KEY) ?? null;
  } catch {
    /* ignore */
  }
  document.documentElement.dataset.theme = cached === "dark" ? "dark" : "light";
}

/** Apply a settings value; 'system' follows prefers-color-scheme and stays in sync. */
export function applyTheme(theme: ThemeSetting) {
  detach();
  if (theme === "dark" || theme === "light") {
    set(theme);
    return;
  }
  if (theme !== "system" || typeof window.matchMedia !== "function") {
    set("light");
    return;
  }
  mq = window.matchMedia("(prefers-color-scheme: dark)");
  set(mq.matches ? "dark" : "light");
  onChange = (e) => set(e.matches ? "dark" : "light");
  mq.addEventListener("change", onChange);
}
