<script lang="ts">
  // The saved readable copy of a page (or, for videos, a big thumbnail + Watch).
  // Remote images in the copy are links unless "Load images" is on for this item.
  import { untrack } from "svelte";
  import * as api from "../api";
  import { friendlyText } from "../errors";
  import { openInBrowser } from "../links";
  import { renderSnapshot } from "../markdown";
  import { items } from "../stores/items.svelte";
  import { ui } from "../stores/ui.svelte";
  import { formatAbsolute } from "../time";
  import type { Item } from "../types";
  import { fileSrc, hostOf } from "../util";
  import Icon from "./Icon.svelte";

  interface Props {
    item: Item;
    isYouTube?: boolean;
  }
  let { item, isYouTube = false }: Props = $props();

  let md = $state<string | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let thumbFailed = $state(false);

  const isVideo = $derived(item.kind === "video");
  const thumb = $derived(thumbFailed ? null : fileSrc(item.image_path));
  const loadImages = $derived(ui.loadImages(item.id));
  const html = $derived(md ? renderSnapshot(md, { loadImages }) : "");
  const busy = $derived(item.snapshot_status === "pending" || item.snapshot_status === "running");

  // Reload when the item, its status or fetch time changes; the async work runs untracked.
  const loadKey = $derived(`${item.id}|${item.snapshot_status}|${item.snapshot_fetched_at ?? ""}`);
  $effect(() => {
    void loadKey;
    untrack(() => void load());
  });

  async function load() {
    const id = item.id;
    const want = item.snapshot_status === "done" || !!item.snapshot_fetched_at;
    loading = true;
    loadError = null;
    try {
      const r = want ? await api.readSnapshot(id) : null;
      if (id === item.id) md = r;
    } catch (e) {
      if (id === item.id) loadError = friendlyText(e);
    } finally {
      if (id === item.id) loading = false;
    }
  }

  const statusText = $derived.by(() => {
    switch (item.snapshot_status) {
      case "none":
        return "No readable copy yet.";
      case "pending":
      case "running":
        return "Saving readable copy…";
      case "failed":
        return `Couldn't save a readable copy. ${item.snapshot_error ? friendlyText(item.snapshot_error) : ""}`.trim();
      case "done":
        return item.snapshot_fetched_at ? `Readable copy from ${formatAbsolute(item.snapshot_fetched_at)}` : "Readable copy saved";
    }
  });
</script>

<div class="page">
  {#if isVideo}
    <div class="video">
      <button type="button" class="poster" aria-label="Play video in browser" onclick={() => openInBrowser(item.url)}>
        {#if thumb}
          <img src={thumb} alt="" onerror={() => (thumbFailed = true)} />
        {:else}
          <span class="noposter" aria-hidden="true"></span>
        {/if}
        <span class="play" aria-hidden="true"><Icon name="play" size={22} /></span>
      </button>
      <div class="vmeta">
        <div class="channel">{item.site_name || hostOf(item.url)}</div>
        {#if item.description}<p class="muted selectable">{item.description}</p>{/if}
        <div>
          <button type="button" onclick={() => openInBrowser(item.url)}>
            <Icon name="play" size={12} /> {isYouTube ? "Watch on YouTube" : "Open video"}
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if !isVideo || md}
    <div class="status" class:failed={item.snapshot_status === "failed"}>
      <span class="dot {item.snapshot_status}" aria-hidden="true"></span>
      <span class="text" title={item.snapshot_error ?? ""}>{loadError ?? statusText}</span>
      <span class="spacer"></span>
      {#if md}
        <label class="check small">
          <input type="checkbox" checked={loadImages} onchange={(e) => ui.setLoadImages(item.id, (e.target as HTMLInputElement).checked)} />
          Load images
        </label>
      {/if}
      <button type="button" class="small" disabled={busy} onclick={() => items.refetch(item.id)}>
        {item.snapshot_status === "failed" ? "Try again" : "Download page again"}
      </button>
    </div>
    {#if item.snapshot_error && item.snapshot_status === "failed"}
      <details class="err-details"><summary>Details</summary><pre>{item.snapshot_error}</pre></details>
    {/if}

    {#if loading && md == null}
      <div class="muted small">Loading…</div>
    {:else if md}
      <article class="md">{@html html}</article>
    {:else if item.snapshot_status === "done"}
      <div class="empty">The readable copy is empty.</div>
    {:else if item.snapshot_status === "failed"}
      <div class="empty">You can still open the original, or try downloading the page again.</div>
    {:else}
      <div class="empty">No readable copy yet. It appears here once the page has been downloaded.</div>
    {/if}
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .video {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .poster {
    position: relative;
    width: 100%;
    max-width: 640px;
    height: auto;
    aspect-ratio: 16 / 9;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--hairline);
    background: var(--surface-dark);
  }
  .poster img,
  .noposter {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .play {
    position: absolute;
    inset: 0;
    margin: auto;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(35, 37, 29, 0.75);
    color: #fff;
  }
  .channel {
    color: var(--ink);
    font-weight: 600;
  }
  .vmeta {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .vmeta p {
    margin: 0;
    max-width: 72ch;
  }
  .status {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--surface-soft);
    font-size: 13px;
  }
  .status.failed {
    background: var(--accent-red-soft);
  }
  .status.failed .text {
    color: var(--ink);
    font-weight: 600;
  }
  .text {
    min-width: 0;
  }
  article {
    padding: 16px 20px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-doc);
  }
</style>
