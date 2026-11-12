<script lang="ts">
  // The "Get set up" view (own sidebar entry) until dismissed or every step is done.
  import * as api from "../api";
  import { copyText } from "../clipboard";
  import { hotkeyGlyphs, DEFAULT_HOTKEY } from "../hotkey";
  import { markStep } from "../onboarding";
    import { settings } from "../stores/settings.svelte";
import { setup } from "../stores/setup.svelte";
  import { system } from "../stores/system.svelte";
  import { toast, ui } from "../stores/ui.svelte";
  import type { OnboardingStep } from "../types";
  import Icon from "./Icon.svelte";

  const s = $derived(settings.value);
  const done = (step: OnboardingStep) => setup.isDone(step);
  const hotkey = $derived(hotkeyGlyphs(s?.hotkey || DEFAULT_HOTKEY));
  let downloading = $state(false);

  function mark(step: OnboardingStep) {
    const patch = markStep(settings.value?.onboarding_steps, step);
    if (patch) void settings.update(patch, { quiet: true });
  }

  async function allowAutomation() {
    const r = await system.askAutomation();
    if (r === "granted") mark("automation");
    else if (r === "chrome_not_running") toast.info("Open Google Chrome first, then try again.");
    else if (r == null) toast.info("Stow will ask the first time you save a tab from Chrome.");
  }
  async function allowNotifications() {
    const r = await system.askNotifications();
    if (r) mark("notifications");
  }
  async function setLogin(on: boolean) {
    const r = await system.setLoginItem(on);
    if (r == null) toast.info("Couldn't change the login item. You can add Stow in System Settings → General → Login Items.");
    else void settings.update({ open_at_login: r }, { quiet: true });
  }
  async function copyPairing() {
    try {
      await copyText(await api.pairingCodeOr(settings.value));
      toast.success("Pairing code copied");
      mark("extension");
    } catch (e) {
      toast.error(e);
    }
  }
  async function downloadModel() {
    downloading = true;
    try {
      await api.downloadWhisperModel(s?.whisper_model ?? "base.en");
      mark("whisper");
      toast.success("Transcription model downloaded");
    } catch {
      toast.info("The model downloads automatically the first time you transcribe a recording.");
    } finally {
      downloading = false;
    }
  }
  function dismiss() {
    void settings.update({ onboarding_dismissed: true });
    ui.select({ kind: "inbox" });
  }
</script>

{#snippet dot(isDone: boolean)}
  <span class="status-dot" class:ok={isDone} aria-hidden="true"></span>
  <span class="sr-only">{isDone ? "Done:" : "To do:"}</span>
{/snippet}

<div class="scroll">
  <section class="setup" aria-labelledby="setup-title">
    <h2 id="setup-title">Welcome to Stow</h2>
    <p class="sub">Get tabs out of Chrome and into buckets you'll actually come back to. Everything stays on this Mac.</p>
    <ol>
      <li>
        {@render dot(done("captured"))}
        <div class="text">
          <div class="t">Save your first tab</div>
          <div class="d">In Chrome, press <kbd>{hotkey}</kbd>. Pick a bucket, hit ⏎, and the tab closes. …or click Add link (<kbd>⌘L</kbd>) and paste a web address.</div>
        </div>
        <div class="b">
          <button type="button" class="small" title="Add one or more links (⌘L)" onclick={() => ui.openAddLink()}><Icon name="plus" size={11} /> Add link</button>
          <button type="button" class="ghost small" onclick={() => ui.select({ kind: "settings" })}>Change shortcut</button>
        </div>
      </li>
      <li>
        {@render dot(done("automation"))}
        <div class="text">
          <div class="t">Let Stow see your Chrome tab</div>
          <div class="d">macOS asks once whether Stow may talk to Google Chrome. Stow only reads the current tab's address and title.</div>
        </div>
        {#if !done("automation")}
          <div class="b">
            {#if system.automation === "denied"}
              <button type="button" class="small" onclick={() => api.openPrivacySettings("automation").catch((e) => toast.error(e))}>Open Privacy Settings</button>
            {:else}
              <button type="button" class="small" onclick={allowAutomation}>Allow access</button>
            {/if}
          </div>
        {/if}
      </li>
      <li>
        {@render dot(done("notifications"))}
        <div class="text">
          <div class="t">Turn on reminders</div>
          <div class="d">Get a nudge when it's time to watch or read something. Reminders only fire while Stow is running.</div>
          <label class="check small">
            <input type="checkbox" checked={!!(system.loginItem ?? s?.open_at_login)} onchange={(e) => setLogin((e.target as HTMLInputElement).checked)} />
            Open Stow at login (recommended)
          </label>
        </div>
        {#if !done("notifications")}
          <div class="b"><button type="button" class="small" onclick={allowNotifications}>Allow notifications</button></div>
        {/if}
      </li>
      <li>
        {@render dot(done("extension"))}
        <div class="text">
          <div class="t">Add the Chrome button (optional)</div>
          <div class="d">One-click saving from the toolbar and right-click menu.</div>
        </div>
        <div class="b">
          <button type="button" class="small" onclick={() => ui.select({ kind: "settings" })}>Get the extension</button>
          <button type="button" class="ghost small" onclick={copyPairing}>Copy pairing code</button>
        </div>
      </li>
      <li>
        {@render dot(done("ai_key"))}
        <div class="text">
          <div class="t">Add an AI key (optional)</div>
          <div class="d">Summaries and task suggestions with your own Anthropic or OpenAI key, kept in the macOS Keychain. Nothing is sent unless you click an AI action.</div>
        </div>
        {#if !done("ai_key")}<div class="b"><button type="button" class="small" onclick={() => ui.select({ kind: "settings" })}>Add key</button></div>{/if}
      </li>
      <li>
        {@render dot(done("whisper"))}
        <div class="text">
          <div class="t">Voice notes (optional)</div>
          <div class="d">Record thoughts and get a transcript made on this Mac. Needs a one-time 142 MB download.</div>
        </div>
        {#if !done("whisper")}<div class="b"><button type="button" class="small" disabled={downloading} onclick={downloadModel}>{downloading ? "Downloading…" : "Download now"}</button></div>{/if}
      </li>
    </ol>
    <div class="footer small">
      <span class="muted" role="status">{setup.done} of {setup.total} done</span>
      <span class="spacer"></span>
      <button type="button" class="ghost small" onclick={dismiss}>Dismiss checklist</button>
    </div>
  </section>
</div>

<style>
  .scroll {
    height: 100%;
    min-height: 0;
    overflow: auto;
    padding: 20px 24px 24px;
    background: var(--surface-card);
  }
  .setup {
    max-width: 560px;
    margin: 0 auto;
    padding: 16px 20px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-doc);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .sub {
    margin: 0;
    color: var(--body);
  }
  ol {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--hairline-soft);
  }
  li > :global(.status-dot) {
    margin-top: 6px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .t {
    color: var(--ink);
    font-weight: 600;
    line-height: 1.4;
  }
  .d {
    font-size: 12.5px;
    line-height: 1.4;
    color: var(--body);
  }
  .check {
    margin-top: 2px;
    color: var(--body);
  }
  .b {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 1px;
  }
  .footer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--hairline-soft);
  }
</style>
