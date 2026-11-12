import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

// Two render profiles (tauri-contract.md "Rendering rules"). Links open via the
// opener plugin (see `openExternalLinks`), never in the webview.

/** Tags no user/AI Markdown may produce: no remote fetches, no forms, no active content. */
export const USER_FORBID_TAGS = [
  "img",
  "form",
  "input",
  "button",
  "iframe",
  "object",
  "embed",
  "video",
  "audio",
  "svg",
  "math",
  "style",
];
export const USER_FORBID_ATTR = ["style", "onerror", "onload"];

function toHtml(md: string): string {
  return marked.parse(md, { async: false }) as string;
}

/** Notes, transcripts and AI output. */
export function renderUserMarkdown(md: string | null | undefined): string {
  if (!md) return "";
  return DOMPurify.sanitize(toHtml(md), {
    USE_PROFILES: { html: true },
    FORBID_TAGS: USER_FORBID_TAGS,
    FORBID_ATTR: USER_FORBID_ATTR,
  });
}

const SNAPSHOT_FORBID_TAGS = USER_FORBID_TAGS.filter((t) => t !== "img");

/**
 * Saved readable copies. Remote images become an "[image]" link (no request is
 * made) unless the per-item "Load images" toggle is on.
 */
export function renderSnapshot(md: string | null | undefined, opts: { loadImages?: boolean } = {}): string {
  if (!md) return "";
  const clean = DOMPurify.sanitize(toHtml(md), {
    USE_PROFILES: { html: true },
    FORBID_TAGS: SNAPSHOT_FORBID_TAGS,
    FORBID_ATTR: [...USER_FORBID_ATTR, "srcset"],
  });
  const box = document.createElement("div");
  box.innerHTML = clean;
  for (const img of Array.from(box.querySelectorAll("img"))) {
    const src = img.getAttribute("src") ?? "";
    const remote = /^https?:\/\//i.test(src);
    if (opts.loadImages && remote) {
      img.setAttribute("loading", "lazy");
      img.setAttribute("referrerpolicy", "no-referrer");
      continue;
    }
    const alt = img.getAttribute("alt")?.trim();
    const label = alt ? `[image: ${alt}]` : "[image]";
    let repl: Node;
    if (remote && !img.closest("a")) {
      const a = document.createElement("a");
      a.href = src;
      a.textContent = label;
      a.className = "img-link";
      repl = a;
    } else repl = document.createTextNode(label);
    img.replaceWith(repl);
  }
  return box.innerHTML;
}

/** Parse "- [ ] text" / "- text" / "1. text" lines into task titles. */
export function parseTaskLines(md: string | null | undefined): string[] {
  if (!md) return [];
  const out: string[] = [];
  for (const raw of md.split(/\r?\n/)) {
    const m = /^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?(.+?)\s*$/.exec(raw);
    if (m && m[1]) out.push(m[1].replace(/\*\*/g, "").trim());
  }
  return out;
}

/** Rough word count of Markdown text (for the AI cost/privacy line). */
export function wordCount(md: string | null | undefined): number {
  if (!md) return 0;
  const m = md.replace(/[#*_`>[\]()!-]/g, " ").match(/\S+/g);
  return m ? m.length : 0;
}
