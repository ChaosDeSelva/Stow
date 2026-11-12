<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { inTauri, isEditable } from "../util";
  import { settings } from "../stores/settings.svelte";
  import { attachments } from "../stores/attachments.svelte";
  import { buckets } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { recordings } from "../stores/recordings.svelte";
  import { ui } from "../stores/ui.svelte";
  import { formatDuration } from "../time";
  import BucketOverview from "./BucketOverview.svelte";
  import ListHeader from "./ListHeader.svelte";
  import SetupChecklist from "./SetupChecklist.svelte";
  import ItemDetail from "./ItemDetail.svelte";
  import ItemList from "./ItemList.svelte";
  import SettingsView from "./SettingsView.svelte";
  import Sidebar from "./Sidebar.svelte";
  import TodayView from "./TodayView.svelte";
  import Icon from "./Icon.svelte";

  const sel = $derived(ui.selection);
  const bucket = $derived(sel.kind === "bucket" ? buckets.byId(sel.id) : undefined);
  const selectedItem = $derived(items.get(ui.selectedItemId));
  let fileDrop = $state(false);
  let now = $state(Date.now());
  let stopping = $state(false);

  // Load the item list whenever the selection changes to an item-bearing view.
  // Read the dependency, then call the store untracked so its writes can't re-trigger this.
  const listKey = $derived(sel.kind === "bucket" ? `bucket:${sel.id}` : sel.kind);
  $effect(() => {
    const key = listKey;
    untrack(() => {
      if (key === "inbox") void items.load({ kind: "inbox" });
      // Archived = every done item (list_items includeDone; the view keeps only done_at rows).
      else if (key === "all" || key === "archived") void items.load({ kind: "all" });
      else if (key.startsWith("bucket:")) void items.load({ kind: "bucket", id: key.slice(7) });
    });
  });

  // The detail pane may show an item that is not in the loaded list yet.
  $effect(() => {
    const id = ui.selectedItemId;
    if (!id) return;
    untrack(() => {
      if (!items.get(id) && !items.isMissing(id) && !items.hasFailed(id)) void items.ensure(id);
    });
  });

  // A selected bucket that no longer exists (deleted from another window) falls back to the Inbox.
  $effect(() => {
    const s = sel;
    const gone = s.kind === "bucket" && buckets.loaded && !buckets.byId(s.id);
    if (gone) untrack(() => ui.select({ kind: "inbox" }));
  });

  // The "Get set up" view goes away once dismissed (Settings → Show setup checklist brings it back).
  $effect(() => {
    const gone = sel.kind === "setup" && !!settings.value?.onboarding_dismissed;
    if (gone) untrack(() => ui.select({ kind: "inbox" }));
  });

  // Global recording indicator: tick while recording, resolve the parent's title.
  $effect(() => {
    if (!recordings.isRecording) return;
    const t = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(t);
  });
  $effect(() => {
    const s = recordings.session;
    if (s?.parentType === "item") untrack(() => !items.get(s.parentId) && void items.ensure(s.parentId));
  });
  const recordingTitle = $derived.by(() => {
    const s = recordings.session;
    if (!s) return "";
    if (s.parentType === "bucket") return buckets.nameOf(s.parentId);
    const it = items.get(s.parentId);
    return it?.title?.trim() || it?.url || "item";
  });
  const elapsed = $derived(recordings.startedAt != null ? formatDuration(now - recordings.startedAt) : "00:00");

  async function stopRecording() {
    stopping = true;
    try {
      const r = await recordings.stop();
      if (r) ui.toast.success(`Recording saved to ${r.parent_type === "bucket" ? buckets.nameOf(r.parent_id) : recordingTitle || "item"}`);
    } finally {
      stopping = false;
    }
  }

  function goToRecording() {
    const s = recordings.session;
    if (!s) return;
    if (s.parentType === "item") {
      const it = items.get(s.parentId);
      ui.selectBucketAndItem(it?.bucket_id ?? null, s.parentId);
      ui.detailTab = "recordings";
    } else ui.select({ kind: "bucket", id: s.parentId });
  }

  // ⌘K / ⌘F search · ⌘L add link · ⌘1 Inbox · ⌘2 Today · ⌘, Settings · ⌘N new bucket.
  function onKey(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === "l" && !e.shiftKey) {
      // Never while typing (notes, search, the Add link field itself).
      if (isEditable(e.target as Element | null) || isEditable(document.activeElement)) return;
      e.preventDefault();
      ui.openAddLink();
    } else if (k === "k" || k === "f") {
      e.preventDefault();
      const el = document.getElementById("stow-search") as HTMLInputElement | null;
      el?.focus();
      el?.select();
    } else if (k === "1") {
      e.preventDefault();
      ui.select({ kind: "inbox" });
    } else if (k === "2") {
      e.preventDefault();
      ui.select({ kind: "today" });
    } else if (k === ",") {
      e.preventDefault();
      ui.select({ kind: "settings" });
    } else if (k === "n" && !e.shiftKey) {
      e.preventDefault();
      ui.requestNewBucket();
    }
  }

  // Native file drag-drop (Tauri) → attach to the shown item or bucket.
  onMount(() => {
    if (!inTauri()) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload;
        // Internal HTML5 drags (item rows) surface here with no paths: not a file drop.
        if (p.type === "enter") fileDrop = p.paths.length > 0;
        else if (p.type === "over") {
          /* keep the state decided on enter */
        } else if (p.type === "leave") fileDrop = false;
        else if (p.type === "drop") {
          fileDrop = false;
          if (p.paths.length) void attachDropped(p.paths);
        }
      })
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => {
        /* not running inside Tauri */
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  async function attachDropped(paths: string[]) {
    if (!paths.length) return;
    if (ui.selectedItemId) {
      const n = await attachments.addPaths("item", ui.selectedItemId, paths);
      if (n) {
        ui.detailTab = "files";
        ui.toast.success(`Attached ${n} file${n === 1 ? "" : "s"}`);
      }
    } else if (sel.kind === "bucket") {
      const n = await attachments.addPaths("bucket", sel.id, paths);
      if (n) ui.toast.success(`Attached ${n} file${n === 1 ? "" : "s"} to ${buckets.nameOf(sel.id)}`);
    } else {
      ui.toast.info("Select an item or bucket first to attach files.");
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell" class:filedrop={fileDrop}>
  {#if recordings.isRecording}
    <div class="recbar">
      <span class="live" aria-hidden="true"></span>
      <span class="label" role="status">Recording</span>
      <span class="mono time" aria-live="off">{elapsed}</span>
      <span class="sep" aria-hidden="true">—</span>
      <button type="button" class="ghost small target truncate" title="Show" onclick={goToRecording}>{recordingTitle}</button>
      <span class="spacer"></span>
      <button type="button" class="small" disabled={stopping} onclick={stopRecording}><Icon name="stop" size={12} /> Stop</button>
    </div>
  {/if}

  <div class="layout">
    <aside class="sidebar" aria-label="Navigation">
      <Sidebar />
    </aside>

    <main class="main">
      {#if sel.kind === "settings"}
        <section class="wide">
          <SettingsView />
        </section>
      {:else if sel.kind === "setup"}
        <section class="wide">
          <SetupChecklist />
        </section>
      {:else if sel.kind === "today"}
        <section class="wide">
          <TodayView />
        </section>
      {:else}
        <section class="list" aria-label="Items">
          <ListHeader {bucket} />
          <div class="scroll">
            <ItemList />
          </div>
        </section>
        <section class="detail" aria-label="Details">
          {#if selectedItem}
            {#key selectedItem.id}
              <ItemDetail item={selectedItem} />
            {/key}
          {:else if ui.selectedItemId && items.hasFailed(ui.selectedItemId)}
            <div class="empty">
              <div>Couldn't load this item.</div>
              <button type="button" class="small" onclick={() => ui.selectedItemId && items.retry(ui.selectedItemId)}>Retry</button>
            </div>
          {:else if ui.selectedItemId && items.isMissing(ui.selectedItemId)}
            <div class="empty">That item was deleted.</div>
          {:else if ui.selectedItemId}
            <div class="empty">Loading…</div>
          {:else if bucket}
            {#key bucket.id}
              <BucketOverview {bucket} />
            {/key}
          {:else}
            <div class="empty">
              <div>{sel.kind === "archived" ? "Select an archived link to read it or restore it." : "Select an item to see its notes, page, tasks and more."}</div>
              {#if fileDrop}<div class="small">Drop files to attach them.</div>{/if}
            </div>
          {/if}
        </section>
      {/if}
    </main>
  </div>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--canvas);
  }
  .shell.filedrop {
    box-shadow: inset 0 0 0 2px var(--accent-blue);
  }
  .recbar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 12px;
    background: var(--accent-red-soft);
    color: var(--ink);
    border-bottom: 1px solid var(--hairline);
    font-size: 13px;
  }
  .live {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--accent-red);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .label {
    font-weight: 700;
  }
  .time {
    font-weight: 600;
  }
  .sep {
    color: var(--mute);
  }
  .target {
    max-width: 40vw;
    display: block;
    text-align: left;
  }
  .layout {
    flex: 1;
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    min-height: 0;
  }
  /* List column scales with the window (380–620 px); the detail pane takes the rest. */
  .main {
    display: grid;
    grid-template-columns: clamp(380px, 32vw, 620px) minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
  }
  @media (max-width: 1099.98px) {
    .main {
      grid-template-columns: 360px minmax(0, 1fr);
    }
  }
  .sidebar {
    min-height: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    background: var(--surface-card);
    border-right: 1px solid var(--hairline);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .detail,
  .wide {
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    height: 100%;
    background: var(--surface-card);
  }
  .wide {
    grid-column: 1 / -1;
  }
  .detail .empty {
    height: 100%;
    max-width: 420px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
</style>
