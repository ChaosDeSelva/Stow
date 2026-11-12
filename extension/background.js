// Stow extension: POSTs the active tab to the local Stow app.
import { endpoint, failureReason, isHttpUrl, isValidPort, isValidToken } from "./lib.js";

const DEFAULTS = { port: 0, token: "", bucketId: "", closeTab: false };

/** Settings live in storage.local; the pairing token must never sync to other machines. */
export async function settings() {
  const local = await chrome.storage.local.get(DEFAULTS);
  if (!local.token) {
    // One-time migration from an older build that used storage.sync.
    const legacy = await chrome.storage.sync.get(DEFAULTS).catch(() => ({}));
    if (legacy && legacy.token) {
      await chrome.storage.local.set(legacy);
      await chrome.storage.sync.remove(Object.keys(DEFAULTS)).catch(() => {});
      return { ...DEFAULTS, ...legacy };
    }
  }
  return { ...DEFAULTS, ...local };
}

async function badge(tabId, text, color, title) {
  try {
    await chrome.action.setBadgeBackgroundColor({ tabId, color });
    await chrome.action.setBadgeText({ tabId, text });
    if (title) await chrome.action.setTitle({ tabId, title });
    setTimeout(() => {
      chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
      chrome.action.setTitle({ tabId, title: "Stow this page" }).catch(() => {});
    }, 4000);
  } catch (_) {}
}

async function stowTab(tab, { noClose = false } = {}) {
  const s = await settings();
  if (!isValidPort(s.port) || !isValidToken(s.token)) {
    await badge(tab.id, "⚙", "#b17816", "Stow isn't paired yet — open the extension options");
    chrome.runtime.openOptionsPage();
    return;
  }
  if (!isHttpUrl(tab.url)) {
    await badge(tab.id, "✕", "#cd4239", "Chrome pages can't be stowed");
    return;
  }
  try {
    const res = await fetch(endpoint(s.port, "/capture"), {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${s.token}` },
      body: JSON.stringify({ url: tab.url, title: tab.title || null, bucket_id: s.bucketId || null }),
    });
    if (!res.ok) {
      await badge(tab.id, "✕", "#cd4239", failureReason(res.status));
      if (res.status === 401 || res.status === 403) chrome.runtime.openOptionsPage();
      return;
    }
    const body = await res.json().catch(() => ({}));
    await badge(tab.id, "✓", "#2c8c66", body.created === false ? "Already in Stow" : "Stowed");
    if (s.closeTab && !noClose) {
      setTimeout(() => chrome.tabs.remove(tab.id).catch(() => {}), 400);
    }
  } catch (e) {
    console.warn("stow failed", e);
    await badge(tab.id, "✕", "#cd4239", failureReason(e));
  }
}

chrome.action.onClicked.addListener((tab) => stowTab(tab));

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "stow-page", title: "Stow this page", contexts: ["page", "action"] });
  chrome.contextMenus.create({ id: "stow-link", title: "Stow this link", contexts: ["link"] });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "stow-page" && tab) return stowTab(tab);
  if (info.menuItemId === "stow-link" && info.linkUrl && tab) {
    // A stowed link is not the open page, so never close the tab it was clicked on.
    return stowTab({ id: tab.id, url: info.linkUrl, title: null }, { noClose: true });
  }
});
