<script lang="ts">
  import { deleteConfirmCopy, deleteItemLater, doneLabel, toggleDone } from "../itemActions";
  import { openInBrowser } from "../links";
  import { displayTitle, siteOf } from "../listview";
  import { appendNotes, saveNotesByKey } from "../notesSave";
  import { buckets } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { ui, type DetailTab } from "../stores/ui.svelte";
  import { formatAbsolute, formatRelative } from "../time";
  import type { Item } from "../types";
  import { fileSrc } from "../util";
  import AiPanel from "./AiPanel.svelte";
  import AttachmentList from "./AttachmentList.svelte";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Icon from "./Icon.svelte";
  import Menu, { menuAnchor } from "./Menu.svelte";
  import NotesEditor from "./NotesEditor.svelte";
  import PageView from "./PageView.svelte";
  import RecordingList from "./RecordingList.svelte";
  import ReminderList from "./ReminderList.svelte";
  import TabBar, { type TabDef } from "./TabBar.svelte";
  import TaskList from "./TaskList.svelte";

  interface Props {
    item: Item;
  }
  let { item }: Props = $props();

  const tabs: TabDef<DetailTab>[] = [
    { id: "notes", label: "Notes", icon: "note-sticky" },
    { id: "page", label: "Page", icon: "file-lines" },
    { id: "tasks", label: "Tasks", icon: "list-check" },
    { id: "reminders", label: "Reminders", icon: "bell" },
    { id: "files", label: "Files", icon: "paperclip" },
    { id: "recordings", label: "Recordings", icon: "microphone" },
    { id: "ai", label: "AI", icon: "wand-magic-sparkles" },
  ];

  let menu = $state<{ x: number; y: number } | null>(null);
  let confirming = $state(false);
  let thumbFailed = $state(false);

  const title = $derived(displayTitle(item));
  const site = $derived(siteOf(item));
  const isVideo = $derived(item.kind === "video");
  const thumb = $derived(thumbFailed ? null : fileSrc(item.image_path));
  const isYouTube = $derived(/(^|\.)youtube\.com$|(^|\.)youtu\.be$/i.test(safeHost(item.url)));
  const openLabel = $derived(isVideo ? "Watch" : "Open original");

  function safeHost(u: string) {
    try {
      return new URL(u).hostname;
    } catch {
      return "";
    }
  }

  function appendToNotes(md: string) {
    appendNotes("item", item.id, md);
    ui.detailTab = "notes";
  }
</script>

<article class="detail" aria-labelledby="detail-title">
  <header>
    <div class="head">
      {#if thumb}
        <img class="thumb" class:video={isVideo} src={thumb} alt="" onerror={() => (thumbFailed = true)} />
      {/if}
      <div class="headtext">
        <h2 id="detail-title" class="selectable">{title}</h2>
        <a href={item.url} title={item.url} class="truncate url" onclick={(e) => { e.preventDefault(); openInBrowser(item.url); }}>{item.url}</a>
        <div class="meta small muted">
          {#if site}<span>{site}</span><span class="sep" aria-hidden="true">·</span>{/if}
          <span class="kind"><Icon name={isVideo ? "play" : "file-lines"} size={10} /> {isVideo ? "Video" : item.kind === "article" ? "Article" : "Page"}</span>
          <span class="sep" aria-hidden="true">·</span>
          <span>{buckets.nameOf(item.bucket_id)}</span>
          <span class="sep" aria-hidden="true">·</span>
          <span title={formatAbsolute(item.created_at)}>saved {formatRelative(item.created_at)}</span>
          {#if item.done_at}<span class="sep" aria-hidden="true">·</span><span class="ok-text">Archived</span>{/if}
        </div>
      </div>
    </div>
    {#if item.description}
      <p class="desc muted selectable">{item.description}</p>
    {/if}
    <div class="actions">
      <!-- Secondary on purpose: the list pane's "Add link" is the window's one primary. -->
      <button type="button" onclick={() => openInBrowser(item.url)}>
        <Icon name={isVideo ? "play" : "arrow-up-right-from-square"} size={12} /> {openLabel}
      </button>
      <button type="button" title={item.done_at ? "Restore from the archive" : "Archive (A)"} onclick={() => toggleDone(item)}>
        <Icon name={item.done_at ? "rotate-left" : "box-archive"} size={12} /> {doneLabel(item)}
      </button>
      <button type="button" class="ghost danger" title="Delete (⌫)" onclick={() => deleteItemLater(item)}>
        <Icon name="trash" size={12} /> Delete
      </button>
      <button type="button" class="ghost icon" aria-label="More actions" aria-haspopup="menu" onclick={(e) => (menu = menuAnchor(e))}><Icon name="ellipsis" /></button>
    </div>
    {#if confirming}
      <ConfirmInline
        message={deleteConfirmCopy(item)}
        onConfirm={() => {
          confirming = false;
          deleteItemLater(item);
        }}
        onCancel={() => (confirming = false)}
      />
    {/if}
  </header>

  <div class="tabs">
    <TabBar {tabs} active={ui.detailTab} onChange={(t) => (ui.detailTab = t)} idPrefix="item" label="Item sections" />
  </div>

  <div class="body" role="tabpanel" id="item-panel-{ui.detailTab}" aria-labelledby="item-tab-{ui.detailTab}" tabindex="-1">
    {#if ui.detailTab === "notes"}
      <NotesEditor recordKey={`item:${item.id}`} value={item.notes_md} onSave={saveNotesByKey} minRows={14} label="Notes for {title}" />
    {:else if ui.detailTab === "page"}
      <PageView {item} {isYouTube} />
    {:else if ui.detailTab === "tasks"}
      <TaskList parentType="item" parentId={item.id} />
    {:else if ui.detailTab === "reminders"}
      <ReminderList targetType="item" targetId={item.id} defaultMessage={title} />
    {:else if ui.detailTab === "files"}
      <AttachmentList parentType="item" parentId={item.id} />
    {:else if ui.detailTab === "recordings"}
      <RecordingList parentType="item" parentId={item.id} {appendToNotes} />
    {:else if ui.detailTab === "ai"}
      <AiPanel parentType="item" parentId={item.id} {item} {appendToNotes} />
    {/if}
  </div>
</article>

{#if menu}
  <Menu
    x={menu.x}
    y={menu.y}
    label="Item actions"
    onClose={() => (menu = null)}
    items={[
      { label: "Download page again", icon: "rotate", onSelect: () => void items.refetch(item.id), separatorAfter: true },
      { label: "Delete…", icon: "trash", danger: true, onSelect: () => (confirming = true) },
    ]}
  />
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface-card);
  }
  header {
    padding: 18px 24px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .thumb {
    flex: none;
    width: 120px;
    max-height: 90px;
    object-fit: cover;
    border-radius: var(--radius);
    border: 1px solid var(--hairline);
    background: var(--surface-soft);
  }
  .thumb.video {
    width: 160px;
    aspect-ratio: 16 / 9;
    max-height: none;
  }
  .headtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .url {
    font-size: 12.5px;
    max-width: 100%;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .kind {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .sep {
    color: var(--mute);
  }
  .desc {
    margin: 0;
    font-size: 13px;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .tabs {
    padding: 6px 24px 10px;
    border-bottom: 1px solid var(--hairline);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 16px 24px 24px;
  }
</style>
