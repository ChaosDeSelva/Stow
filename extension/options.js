import { endpoint, isValidPort, isValidToken, parsePairingCode } from "./lib.js";

const $ = (id) => document.getElementById(id);
const DEFAULTS = { port: 0, token: "", bucketId: "", closeTab: false };
const DEFAULT_PORT = 47823;

function status(kind, msg) {
  const el = $("status");
  el.className = kind;
  el.textContent = msg;
}

async function load() {
  return { ...DEFAULTS, ...(await chrome.storage.local.get(DEFAULTS)) };
}

async function loadBuckets(port, token, selected) {
  const sel = $("bucket");
  sel.innerHTML = '<option value="">Inbox</option>';
  if (!isValidPort(port) || !isValidToken(token)) return false;
  try {
    const res = await fetch(endpoint(port, "/buckets"), { headers: { authorization: `Bearer ${token}` } });
    if (!res.ok) throw new Error(res.status === 401 ? "pairing code rejected" : `HTTP ${res.status}`);
    const buckets = await res.json();
    for (const b of buckets) {
      const o = document.createElement("option");
      o.value = b.id;
      o.textContent = `${b.emoji ? b.emoji + " " : ""}${b.name}`;
      if (b.id === selected) o.selected = true;
      sel.appendChild(o);
    }
    status("ok", `✓ Connected to Stow — ${buckets.length} bucket${buckets.length === 1 ? "" : "s"}`);
    return true;
  } catch (e) {
    status("err", `Couldn't reach Stow: ${e.message}. Is the app running?`);
    return false;
  }
}

async function init() {
  const s = await load();
  if (s.port && s.token) $("code").value = `${s.port}:${s.token}`;
  $("closeTab").checked = s.closeTab;
  await loadBuckets(s.port, s.token, s.bucketId);
}

async function persist(partial) {
  const s = { ...(await load()), ...partial };
  await chrome.storage.local.set(s);
  return s;
}

$("save").addEventListener("click", async () => {
  const parsed = parsePairingCode($("code").value);
  if (!parsed) {
    status("err", "That doesn't look like a pairing code. It looks like 47823:3f9a… — copy it from Stow → Settings → Chrome extension.");
    return;
  }
  const s = await persist({ port: parsed.port, token: parsed.token, bucketId: $("bucket").value, closeTab: $("closeTab").checked });
  await loadBuckets(s.port, s.token, s.bucketId);
});

$("auto").addEventListener("click", async () => {
  status("info", "Asking Stow… approve the request in the Stow window.");
  const s = await load();
  const ports = [s.port, DEFAULT_PORT].filter(isValidPort);
  for (const port of ports) {
    try {
      const health = await fetch(endpoint(port, "/health"));
      if (!health.ok) continue;
      const res = await fetch(endpoint(port, "/pair"), {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name: "Chrome extension" }),
      });
      if (res.status === 403) {
        status("err", "Stow declined the pairing request.");
        return;
      }
      if (!res.ok) continue;
      const { token } = await res.json();
      if (!isValidToken(token)) continue;
      const saved = await persist({ port, token });
      $("code").value = `${port}:${token}`;
      await loadBuckets(saved.port, saved.token, saved.bucketId);
      return;
    } catch (_) {
      /* try next port */
    }
  }
  status("err", "Couldn't find Stow on this Mac. Open the app, then try again, or paste the pairing code.");
});

$("bucket").addEventListener("change", () => persist({ bucketId: $("bucket").value }));
$("closeTab").addEventListener("change", () => persist({ closeTab: $("closeTab").checked }));

init();
