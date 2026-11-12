<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "./lib/api";
  import { subscribe } from "./lib/api";
  import { flushAll } from "./lib/autosave";
  import CapturePanel from "./lib/components/CapturePanel.svelte";
  import Icon from "./lib/components/Icon.svelte";
  import MainLayout from "./lib/components/MainLayout.svelte";
  import Modal from "./lib/components/Modal.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import { openExternalLinks } from "./lib/links";
  import { markStep } from "./lib/onboarding";
  import { aiRuns } from "./lib/stores/aiRuns.svelte";
  import { buckets } from "./lib/stores/buckets.svelte";
  import { deletions } from "./lib/stores/deletions.svelte";
  import { items } from "./lib/stores/items.svelte";
  import { recordings } from "./lib/stores/recordings.svelte";
  import { reminders } from "./lib/stores/reminders.svelte";
  import { settings } from "./lib/stores/settings.svelte";
import { setup } from "./lib/stores/setup.svelte";
  import { system } from "./lib/stores/system.svelte";
  import { ui } from "./lib/stores/ui.svelte";
  import { resolveTarget } from "./lib/targets";
  import type { OnboardingStep, PairRequestPayload, TargetType } from "./lib/types";
  import { extractFirstUrl } from "./lib/url";
  import { inTauri, isEditable, lsGet, lsSet } from "./lib/util";

  /** Set once the main window has opened: the first launch starts on "Get set up", later ones on the Inbox. */
  const LS_LAUNCHED = "stow.launched";

  let hash = $state(window.location.hash);
  const isCapture = $derived(hash === "#/capture");
  const desktop = inTauri();
  let pairQueue = $state<PairRequestPayload[]>([]);
  let wasHidden = false;

  if (!desktop) ui.toast.suppressErrors(true);

  async function navigateTo(targetType: TargetType, targetId: string) {
    const t = await resolveTarget(targetType, targetId, { fresh: true });
    if (!t) {
      ui.toast.error({ code: "not_found", message: "target not found" }, { what: targetType === "task" ? "task" : targetType });
      return;
    }
    ui.selectBucketAndItem(t.bucketId, t.itemId);
    if (t.itemId && targetType === "task") ui.detailTab = "tasks";
  }

  function mark(step: OnboardingStep) {
    const patch = markStep(settings.value?.onboarding_steps, step);
    if (patch && settings.value) void settings.update(patch, { quiet: true });
  }

  async function onPaste(e: ClipboardEvent) {
    if (isEditable(document.activeElement) || isEditable(e.target as Element | null)) return;
    const text = e.clipboardData?.getData("text/plain") ?? "";
    const url = extractFirstUrl(text);
    if (!url) return;
    e.preventDefault();
    const r = await items.addItem(url, ui.currentBucketId);
    if (!r) return;
    if (r.created) {
      ui.toast.success(`Saved to ${buckets.nameOf(r.item.bucket_id)}`);
      mark("captured");
    } else ui.toast.info(`Already in ${buckets.nameOf(r.item.bucket_id)}`);
  }

  function onFocus() {
    const f = ui.lastFiredReminder;
    if (!f) return;
    if (Date.now() - f.at < 60_000 && !f.hadFocus) {
      ui.clearFired();
      void navigateTo(f.payload.target_type, f.payload.target_id);
    } else if (Date.now() - f.at >= 60_000) ui.clearFired();
  }

  /** Window shown again after being closed to the menu bar: explain once. */
  function onVisibility() {
    if (document.visibilityState === "hidden") {
      wasHidden = true;
      return;
    }
    const s = settings.value;
    if (!wasHidden || !s || s.close_to_tray_explained || s.quit_on_close) return;
    wasHidden = false;
    void settings.update({ close_to_tray_explained: true }, { quiet: true });
    ui.toast.info(
      "Stow is still running in the menu bar so reminders can fire. Quit with ⌘Q or from the menu bar icon.",
      {
        duration: 0,
        actions: [
          { label: "Got it", run: () => {} },
          { label: "Quit when I close the window", run: () => void settings.update({ quit_on_close: true }) },
        ],
      },
    );
  }

  async function onWillQuit() {
    // Wait for the notes writes and the pending deletes to land before telling
    // Rust to exit; it quits on its own after the fallback delay regardless.
    await Promise.allSettled([...flushAll(), deletions.flushAll()]);
    api.readyToQuit().catch(() => {});
  }

  function onUnload() {
    flushAll();
    void deletions.flushAll();
  }

  async function answerPair(allow: boolean) {
    const req = pairQueue[0];
    if (!req) return;
    pairQueue = pairQueue.slice(1);
    try {
      await api.respondPair(req.id, allow);
      if (allow) {
        ui.toast.success("Chrome extension connected");
        mark("extension");
      }
    } catch (e) {
      ui.toast.error(e);
    }
  }

  onMount(() => {
    const onHash = () => (hash = window.location.hash);
    window.addEventListener("hashchange", onHash);
    // Both windows: keep the bucket list in sync when ANY window changes buckets.
    const offBuckets = subscribe("buckets-changed", () => void buckets.load({ quiet: true }));
    if (isCapture) {
      return () => {
        window.removeEventListener("hashchange", onHash);
        offBuckets();
      };
    }

    // Main window: one-shot loads (onMount, never $effect: no load loops).
    void buckets.load();
    void settings.load().then((s) => {
      // Only while the user hasn't navigated yet, and never onto a checklist with nothing left to do.
      const untouched = ui.selection.kind === "inbox" && !ui.selectedItemId && !ui.addLinkOpen;
      if (s && !s.onboarding_dismissed && lsGet(LS_LAUNCHED) !== "1" && untouched && !setup.complete) ui.select({ kind: "setup" });
      if (s) lsSet(LS_LAUNCHED, "1");
    });
    void items.refreshCounts();
    void reminders.loadAgenda(new Date(), true);
    void system.refreshKeys();
    void system.refreshPermissions();

    const offs = [
      subscribe("item-added", ({ id }) => {
        void items.onAdded(id).then((it) => {
          if (it) mark("captured");
        });
      }),
      subscribe("item-updated", ({ id }) => void items.refresh(id)),
      // The process exits ~800 ms later unless we call ready_to_quit sooner.
      subscribe("app-will-quit", () => void onWillQuit()),
      subscribe("recording-updated", ({ id }) => void recordings.reloadContaining(id)),
      // Tray Stop (or any other path) flips the record button and the global bar too.
      subscribe("recording-state", ({ recording }) => recordings.setRecording(recording)),
      subscribe("ai-run-updated", ({ id }) => void aiRuns.reloadContaining(id)),
      subscribe("reminder-fired", (p) => {
        // Notification clicks navigate from Rust (`navigate`); a focus gained soon AFTER
        // the reminder fired is still treated as the click for the cases it misses (see onFocus).
        ui.rememberFired(p, document.hasFocus());
        ui.toast.info(`Reminder: ${p.title}`, {
          duration: 10_000,
          actions: [{ label: "Open", run: () => void navigateTo(p.target_type, p.target_id) }],
        });
        void reminders.reloadAll();
      }),
      subscribe("navigate", (p) => void navigateTo(p.target_type, p.target_id)),
      // Tray "Settings…" (Rust shows the window itself).
      subscribe("open-settings", () => ui.select({ kind: "settings" })),
      subscribe("pair-request", (p) => (pairQueue = [...pairQueue, p])),
      subscribe("hotkey-status", (p) => {
        const s = settings.value;
        if (s) settings.set({ ...s, hotkey: p.hotkey, hotkey_registered: p.registered, hotkey_error: p.error });
      }),
      subscribe("model-download-progress", (p) => {
        if (p.total != null && p.received >= p.total) mark("whisper");
      }),
    ];
    document.addEventListener("paste", onPaste);
    document.addEventListener("click", openExternalLinks);
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", onFocus);
    window.addEventListener("beforeunload", onUnload);
    return () => {
      window.removeEventListener("hashchange", onHash);
      offBuckets();
      for (const off of offs) off();
      document.removeEventListener("paste", onPaste);
      document.removeEventListener("click", openExternalLinks);
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("beforeunload", onUnload);
    };
  });
</script>

{#if !desktop}
  <div class="banner red nodesktop" role="alert">
    <Icon name="triangle-exclamation" class="banner-icon" /><span>Open Stow from the desktop app. This page can't reach your library from a browser.</span>
  </div>
{/if}

{#if isCapture}
  <CapturePanel />
{:else}
  <MainLayout />
{/if}

{#if pairQueue.length}
  <Modal title="Allow the Chrome extension to save tabs to Stow?" role="alertdialog" onClose={() => answerPair(false)}>
    <p class="muted">“{pairQueue[0].name || "Stow for Chrome"}” is asking to connect. Only allow it if you just installed or reset the extension.</p>
    {#snippet actions()}
      <button type="button" onclick={() => answerPair(false)}>Deny</button>
      <button type="button" class="primary" onclick={() => answerPair(true)}>Allow</button>
    {/snippet}
  </Modal>
{/if}
<Toasts />

<style>
  .nodesktop {
    position: fixed;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 1100;
    border: 1px solid var(--hairline);
  }
  p {
    margin: 0;
  }
</style>
