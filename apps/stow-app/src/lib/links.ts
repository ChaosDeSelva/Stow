import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "./stores/ui.svelte";

/**
 * Document-level click delegate: links inside rendered Markdown (`.md`) open in
 * the system browser instead of navigating the webview.
 */
export function openExternalLinks(e: MouseEvent) {
  const a = (e.target as HTMLElement | null)?.closest?.("a");
  if (!a || !a.closest(".md")) return;
  const href = a.getAttribute("href") ?? "";
  e.preventDefault();
  if (/^https?:\/\//i.test(href)) openUrl(href).catch((err) => toast.error(err));
}

export function openInBrowser(url: string) {
  openUrl(url).catch((err) => toast.error(err));
}
