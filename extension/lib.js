// Pure helpers shared by the service worker and the options page. No Chrome APIs here
// so they can be unit-tested with `node --test extension/lib.test.mjs`.

/** Parse "47823:3f9a…" (or legacy separate values) into {port, token} or null. */
export function parsePairingCode(input) {
  const s = String(input || "").trim();
  const i = s.indexOf(":");
  if (i <= 0) return null;
  const port = s.slice(0, i).trim();
  const token = s.slice(i + 1).trim();
  if (!isValidPort(port) || !isValidToken(token)) return null;
  return { port: Number(port), token };
}

export function isValidPort(p) {
  return /^\d{1,5}$/.test(String(p)) && Number(p) >= 1 && Number(p) <= 65535;
}

export function isValidToken(t) {
  return /^[0-9a-f]{32}$/.test(String(t));
}

/** Build the local endpoint URL from a validated port; never interpolate raw strings. */
export function endpoint(port, path) {
  if (!isValidPort(port)) throw new Error("invalid port");
  const u = new URL("http://127.0.0.1/");
  u.port = String(port);
  u.pathname = path;
  return u.toString();
}

export function isHttpUrl(url) {
  return /^https?:\/\//i.test(String(url || ""));
}

/** Human reason for the toolbar title/badge, from an error or HTTP status. */
export function failureReason(statusOrError) {
  if (statusOrError === 401 || statusOrError === 403) return "Pairing code changed — open Stow's extension options";
  if (typeof statusOrError === "number") return `Stow returned HTTP ${statusOrError}`;
  return "Stow isn't running — open the app";
}
