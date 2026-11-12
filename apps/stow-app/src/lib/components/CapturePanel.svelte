<script lang="ts">
  // The `capture` window: a compact always-on-top, transparent panel (520×320, no
  // decorations) that draws its own rounded card. Its capability allows only add_item,
  // capture_front_tab, close_front_tab, hide_capture, list_buckets, create_bucket,
  // capture_settings, set_close_tab_after_capture, app_info and clipboard READ: no other
  // command may be reached from here or the stores it uses (so no settings store here).
  import { onMount, tick } from "svelte";
  import { readText } from "@tauri-apps/plugin-clipboard-manager";
  import * as api from "../api";
  import { subscribe } from "../api";
  import { errorCode, friendlyText } from "../errors";
  import { buckets } from "../stores/buckets.svelte";
  import { applyTheme } from "../theme";
  import { formatRelative } from "../time";
  import type { Item } from "../types";
  import { extractFirstUrl } from "../url";
  import { lsGet, lsSet } from "../util";
  import BucketPicker from "./BucketPicker.svelte";
  import Icon from "./Icon.svelte";

  const LS_BUCKET = "stow.capture.lastBucket";
  const LS_CLOSE_EXPLAINED = "stow.capture.closeExplained";
  const SAVED_MS = 600;

  let url = $state("");
  let title = $state("");
  let bucketId = $state<string | null>(lsGet(LS_BUCKET) || null);
  let closeTab = $state(false);
  let error = $state<string | null>(null);
  /** Extra line under the error ("Stow works with Google Chrome"). */
  let hint = $state<string | null>(null);
  /** The URL Chrome's front tab showed when captured; closing is only attempted for that tab. */
  let chromeUrl = $state<string | null>(null);
  let reading = $state(false);
  let fromClipboard = $state(false);
  let saving = $state(false);
  /** Success line shown briefly before the window hides. */
  let saved = $state<{ bucket: string; note?: string } | null>(null);
  /** add_item found the URL already saved. */
  let duplicate = $state<Item | null>(null);
  let urlInput = $state<HTMLInputElement | null>(null);
  let picker = $state<ReturnType<typeof BucketPicker> | null>(null);

  function rememberBucket(id: string | null) {
    lsSet(LS_BUCKET, id);
  }

  /** The panel's narrow settings view; also keeps this window's theme in step with Settings. */
  async function reloadSettings() {
    try {
      const s = await api.captureSettings();
      closeTab = s.close_tab_after_capture;
      applyTheme(s.theme ?? "light");
    } catch {
      /* no backend (browser preview): keep the current values */
    }
  }

  onMount(() => {
    void buckets.load({ quiet: true });
    void reloadSettings();
    // Do NOT capture on mount: the hidden capture window mounts at app launch and
    // asking Chrome then would trigger the macOS Automation prompt unprompted.
    return subscribe("capture-open", () => {
      resetForm();
      void buckets.load({ quiet: true });
      void reloadSettings();
      void capture();
    });
  });

  // Drop a remembered bucket that no longer exists.
  $effect(() => {
    if (bucketId && buckets.loaded && !buckets.byId(bucketId)) bucketId = null;
  });

  function resetForm() {
    url = "";
    title = "";
    error = null;
    hint = null;
    chromeUrl = null;
    fromClipboard = false;
    saving = false;
    saved = null;
    duplicate = null;
  }

  async function focusUrl() {
    await tick();
    urlInput?.focus();
    urlInput?.select();
  }
  async function focusPicker() {
    await tick();
    picker?.focus();
  }

  async function capture() {
    reading = true;
    try {
      const tab = await api.captureFrontTab();
      if (/^https?:\/\//i.test(tab.url)) {
        url = tab.url;
        title = tab.title;
        chromeUrl = tab.url;
        void focusPicker();
      } else {
        error = `This is a Chrome page (${tab.url}), which can't be saved. Paste a link instead.`;
        void focusUrl();
      }
    } catch (e) {
      const code = errorCode(e);
      error = `${friendlyText(e)} Paste a link instead.`;
      if (code === "chrome_not_running") {
        hint = "Stow works with Google Chrome.";
        await prefillFromClipboard();
      }
      void focusUrl();
    } finally {
      reading = false;
    }
  }

  /** Chrome isn't running: offer a URL from the clipboard (plugin permission may be missing). */
  async function prefillFromClipboard() {
    try {
      const u = extractFirstUrl(await readText());
      if (u && !url) {
        url = u;
        fromClipboard = true;
        error = null;
      }
    } catch {
      /* no clipboard access: leave the field empty */
    }
  }

  async function finish(ms: number) {
    await new Promise((r) => setTimeout(r, ms));
    await api.hideCapture().catch(() => {});
    resetForm();
  }

  async function save() {
    if (saving || duplicate) return;
    const u = extractFirstUrl(url);
    if (!u) {
      error = "Enter a web address starting with http:// or https://";
      void focusUrl();
      return;
    }
    saving = true;
    error = null;
    try {
      const r = await api.addItem(u, bucketId, title.trim() || null);
      rememberBucket(bucketId);
      if (!r.created) {
        duplicate = r.item;
        saving = false;
        return;
      }
      let note: string | undefined;
      let ms = SAVED_MS;
      if (closeTab && chromeUrl) {
        // Only close the tab that was captured; Rust re-checks it still shows this URL.
        try {
          const c = await api.closeFrontTab(chromeUrl);
          if (!c.closed) {
            note = "Saved. The tab wasn't closed because you switched tabs.";
            ms = 1800;
          } else if (lsGet(LS_CLOSE_EXPLAINED) !== "1") {
            lsSet(LS_CLOSE_EXPLAINED, "1");
            note = "Stow closes the tab after saving. Untick 'Close tab in Chrome' to keep it.";
            ms = 2600;
          }
        } catch (e) {
          note = `Saved, but the tab wasn't closed: ${friendlyText(e)}`;
          ms = 2200;
        }
      }
      saved = { bucket: buckets.nameOf(r.item.bucket_id), note };
      await finish(ms);
    } catch (e) {
      error = friendlyText(e);
    } finally {
      saving = false;
    }
  }

  async function cancel() {
    resetForm();
    await api.hideCapture().catch(() => {});
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      void cancel();
    } else if (e.key === "Enter" && !e.isComposing) {
      // Enter on a focused button activates that button, not Save.
      if ((document.activeElement as HTMLElement | null)?.tagName === "BUTTON") return;
      e.preventDefault();
      if (duplicate) void cancel();
      else void save();
    }
  }

  function onUrlPaste(e: ClipboardEvent) {
    const text = e.clipboardData?.getData("text/plain") ?? "";
    const u = extractFirstUrl(text);
    if (u) {
      e.preventDefault();
      url = u;
      error = null;
      chromeUrl = null;
      fromClipboard = false;
    }
  }

  async function onCloseTabChange(e: Event) {
    const was = closeTab;
    closeTab = (e.target as HTMLInputElement).checked;
    try {
      const s = await api.setCloseTabAfterCapture(closeTab);
      closeTab = s.close_tab_after_capture;
    } catch (err) {
      closeTab = was;
      error = `Couldn't save that setting: ${friendlyText(err)}`;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- The window is transparent: this card (10 px radius, hairline border) is all that shows. -->
<div class="capture">
  <div class="drag" data-tauri-drag-region>
    <span class="brand" data-tauri-drag-region>Stow</span>
    <span class="muted small" data-tauri-drag-region>Quick capture</span>
    <span class="spacer" data-tauri-drag-region></span>
    {#if chromeUrl}<span class="source muted small"><Icon name="chrome" style="brands" size={12} /> From Chrome</span>{/if}
    {#if fromClipboard}<span class="chip accent">From clipboard</span>{/if}
    <button type="button" class="ghost icon small" aria-label="Close (Esc)" onclick={cancel}><Icon name="xmark" /></button>
  </div>

  <div class="card">
    {#if saved}
      <div class="saved" role="status">
        <div class="big"><Icon name="circle-check" class="saved-icon" /> Saved to <strong>{saved.bucket}</strong></div>
        {#if saved.note}<div class="small muted">{saved.note}</div>{/if}
      </div>
    {:else if duplicate}
      {@const where = buckets.nameOf(duplicate.bucket_id)}
      <div class="dup" role="alert">
        <div>Already saved in <strong>{where}</strong> ({formatRelative(duplicate.created_at)}).</div>
        <div class="small muted">Move it from the Stow window if it belongs elsewhere.</div>
        <div class="row">
          <button type="button" class="primary" onclick={cancel}>Keep in {where}</button>
        </div>
      </div>
    {:else}
      <div class="fields">
        <label class="sr-only" for="cap-url">Web address</label>
        <input
          id="cap-url"
          bind:this={urlInput}
          type="url"
          class="url mono"
          placeholder={reading ? "Reading Chrome…" : "https://…"}
          bind:value={url}
          onpaste={onUrlPaste}
          oninput={() => (fromClipboard = false)}
          spellcheck="false"
          autocomplete="off"
        />
        <label class="sr-only" for="cap-title">Title</label>
        <input id="cap-title" type="text" class="title" placeholder="Title (optional)" bind:value={title} />
        <BucketPicker bind:this={picker} value={bucketId} onChange={(id) => (bucketId = id)} onSubmit={save} label="Save to bucket" />
      </div>

      {#if error}
        <div class="banner red small" role="alert">
          <Icon name="triangle-exclamation" class="banner-icon" />
          <span>{error}{#if hint}<br /><span class="muted">{hint}</span>{/if}</span>
        </div>
      {/if}

      <div class="footer">
        <label class="check small">
          <input type="checkbox" checked={closeTab} onchange={onCloseTabChange} />
          Close tab in Chrome
        </label>
        <span class="spacer"></span>
        <button type="button" onclick={cancel}>Cancel</button>
        <button type="button" class="primary" disabled={saving || reading || !url.trim()} onclick={save}>{saving ? "Saving…" : "Save"}</button>
      </div>
      <div class="hint small muted">Type to pick a bucket · <kbd>⏎</kbd> save · <kbd>esc</kbd> cancel</div>
    {/if}
  </div>
</div>

<style>
  /* A few px of margin so the rounded corners (and the window shadow) show against the desktop. */
  .capture {
    --panel-margin: 6px;
    height: calc(100vh - 2 * var(--panel-margin));
    margin: var(--panel-margin);
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 12px 12px;
    border: 1px solid var(--hairline);
    border-radius: 10px;
    background: var(--canvas);
    overflow: hidden;
  }
  .drag {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    margin: 0 -12px;
    padding: 0 8px 0 14px;
    border-bottom: 1px solid var(--hairline);
    background: var(--canvas);
    cursor: default;
  }
  .brand {
    color: var(--ink);
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .source {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .card {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-card);
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .fields input {
    width: 100%;
    height: 32px;
  }
  .footer {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .footer button {
    height: 32px;
  }
  .hint {
    text-align: right;
  }
  .saved,
  .dup {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 10px;
    text-align: center;
    color: var(--ink);
  }
  .big {
    font-size: 15px;
  }
</style>
