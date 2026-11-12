const URL_TOKEN = /https?:\/\/[^\s<>"']+/i;
const TRAILING = /[.,;:!?'"’”]+$/;

/**
 * First http(s) URL in free text, or null. Trailing sentence punctuation is
 * stripped; a closing `)` is kept only when the token has a matching `(`.
 * Non-http schemes (chrome://, javascript:, file://) never match.
 */
export function extractFirstUrl(text: string): string | null {
  if (!text) return null;
  const m = URL_TOKEN.exec(text.trim());
  if (!m) return null;
  let url = m[0];
  for (;;) {
    const before = url;
    url = url.replace(TRAILING, "");
    while (url.endsWith(")") && count(url, "(") < count(url, ")")) url = url.slice(0, -1);
    while (url.endsWith("]") && count(url, "[") < count(url, "]")) url = url.slice(0, -1);
    if (url === before) break;
  }
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    if (!parsed.hostname) return null;
  } catch {
    return null;
  }
  return url;
}

function count(s: string, ch: string): number {
  let n = 0;
  for (const c of s) if (c === ch) n++;
  return n;
}

/**
 * Every http(s) URL in free text, split on commas, whitespace and newlines.
 * Junk tokens are dropped, duplicates removed, order preserved.
 */
export function extractUrls(text: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const token of tokens(text)) {
    const u = extractFirstUrl(token);
    if (u && !seen.has(u)) {
      seen.add(u);
      out.push(u);
    }
  }
  return out;
}

/** Non-empty lines of `text` holding no http(s) URL (what the Add link form skips). */
export function countIgnoredLines(text: string): number {
  let n = 0;
  for (const line of (text ?? "").split(/\r?\n/)) if (line.trim() && !extractUrls(line).length) n++;
  return n;
}

function tokens(text: string): string[] {
  return (text ?? "").split(/[\s,]+/).filter(Boolean);
}
