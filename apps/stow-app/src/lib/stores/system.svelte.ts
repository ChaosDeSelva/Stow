// Live system status shared by Settings, onboarding and the AI panel: API keys,
// macOS permissions and launch-at-login. Every probe degrades to "unknown"
// (null) when the command or plugin is unavailable; nothing here toasts.
import * as autostart from "@tauri-apps/plugin-autostart";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import * as api from "../api";
import type { AiProvider, AutomationStatus, MicrophoneStatus } from "../types";

let hasKey = $state<Record<AiProvider, boolean | null>>({ anthropic: null, openai: null });
let automation = $state<AutomationStatus | null>(null);
let microphone = $state<MicrophoneStatus | null>(null);
let notifications = $state<boolean | null>(null);
let loginItem = $state<boolean | null>(null);

/** Call a possibly-missing plugin/command; resolves null instead of rejecting or throwing. */
async function probe<T>(fn: () => Promise<T>): Promise<T | null> {
  try {
    return await fn();
  } catch {
    return null;
  }
}

export const system = {
  get hasKey() {
    return hasKey;
  },
  get anyKey(): boolean | null {
    if (hasKey.anthropic || hasKey.openai) return true;
    if (hasKey.anthropic === null || hasKey.openai === null) return null;
    return false;
  },
  get automation() {
    return automation;
  },
  get microphone() {
    return microphone;
  },
  get notifications() {
    return notifications;
  },
  get loginItem() {
    return loginItem;
  },

  /** Assigns only this provider's slot after the await (no lost update between concurrent refreshes). */
  async refreshKey(p: AiProvider) {
    const v = await probe(() => api.hasApiKey(p));
    hasKey[p] = v ?? false;
    return hasKey[p];
  },
  refreshKeys() {
    return Promise.all([this.refreshKey("anthropic"), this.refreshKey("openai")]);
  },

  async refreshPermissions() {
    const [st, n] = await Promise.all([probe(() => api.permissionStatus()), probe(() => isPermissionGranted())]);
    automation = st?.automation ?? null;
    microphone = st?.microphone ?? null;
    notifications = n;
  },
  /** request_automation resolves the new automation status string (not a permission_status object). */
  async askAutomation(): Promise<AutomationStatus | null> {
    const r = await probe(() => api.requestAutomation());
    if (typeof r === "string") automation = r;
    return typeof r === "string" ? r : null;
  },
  /** request_microphone resolves the new microphone status string. */
  async askMicrophone(): Promise<MicrophoneStatus | null> {
    const r = await probe(() => api.requestMicrophone());
    if (typeof r === "string") microphone = r;
    return typeof r === "string" ? r : null;
  },
  async askNotifications(): Promise<boolean | null> {
    const r = await probe(() => requestPermission());
    notifications = r == null ? notifications : r === "granted";
    return notifications;
  },

  async refreshLoginItem() {
    loginItem = await probe(() => autostart.isEnabled());
    return loginItem;
  },
  /** Enable/disable launch at login; resolves the resulting state (null = plugin missing). */
  async setLoginItem(on: boolean): Promise<boolean | null> {
    const ok = await probe(() => (on ? autostart.enable() : autostart.disable()).then(() => true));
    if (ok) loginItem = on;
    return ok ? on : null;
  },

  /** Test helper. */
  reset() {
    hasKey = { anthropic: null, openai: null };
    automation = microphone = null;
    notifications = loginItem = null;
  },
};

/** "Allowed / Not allowed / Not asked yet" for a permission status. */
export function permissionLabel(s: string | boolean | null | undefined): { text: string; tone: "ok" | "bad" | "unknown" } {
  if (s === true || s === "granted") return { text: "Allowed", tone: "ok" };
  if (s === false || s === "denied" || s === "restricted") return { text: "Not allowed", tone: "bad" };
  if (s === "chrome_not_running") return { text: "Chrome isn't running", tone: "unknown" };
  return { text: "Not asked yet", tone: "unknown" };
}
