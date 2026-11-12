<script lang="ts">
  import { archiveItemLater, deleteConfirmCopy, deleteItemLater, doneLabel, restoreItem, toggleDone } from "../itemActions";
  import { openInBrowser } from "../links";
  import { displayTitle, siteOf } from "../listview";
  import { buckets, label } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { Item } from "../types";
  import { fileSrc } from "../util";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Icon from "./Icon.svelte";
  import Menu, { menuAnchor, type MenuItem } from "./Menu.svelte";

  interface Props {
    item: Item;
    selected: boolean;
    tabindex?: number;
    highlighted?: boolean;
    /** Part of the multi-selection (⌘-click / ⇧-click / ⌘A). */
    checked?: boolean;
    /** A multi-selection exists: aria-selected then means "checked", not "open in the detail pane". */
    multi?: boolean;
    /** Plain click handler from the list (modifier-aware); defaults to selecting this row. */
    onPick?: (e: MouseEvent) => void;
  }
  let { item, selected, tabindex = -1, highlighted = false, checked = false, multi = false, onPick }: Props = $props();

  let menu = $state<{ x: number; y: number } | null>(null);
  let confirming = $state(false);
  let iconFailed = $state(false);

  const favicon = $derived(iconFailed ? null : fileSrc(item.favicon_path));
  const title = $derived(displayTitle(item));
  const site = $derived(siteOf(item));
  const letter = $derived((site || title || "?").trim().charAt(0).toUpperCase() || "?");
  const kindIcon = $derived(item.kind === "video" ? "play" : item.kind === "article" ? "file-lines" : null);
  const kindLabel = $derived(item.kind === "video" ? "Video" : item.kind === "article" ? "Article" : "Page");

  const statusTitle = $derived.by(() => {
    switch (item.snapshot_status) {
      case "failed":
        return "Couldn't save a readable copy";
      case "done":
        return "Readable copy saved";
      case "running":
        return "Saving readable copy…";
      case "pending":
        return "Readable copy queued";
      default:
        return "No readable copy yet.";
    }
  });

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    menu = menuAnchor(e);
  }

  const menuItems = $derived.by((): MenuItem[] => {
    const moveTargets: MenuItem[] = [
      { label: "Inbox", checked: item.bucket_id == null, onSelect: () => void items.move(item.id, null) },
      ...buckets.list
        .filter((b) => !b.archived_at)
        .map((b) => ({
          label: label(b),
          checked: b.id === item.bucket_id,
          onSelect: () => void items.move(item.id, b.id),
        })),
    ];
    return [
      { label: doneLabel(item), icon: item.done_at ? "rotate-left" : "box-archive", onSelect: () => void toggleDone(item) },
      { label: "Move to…", icon: "folder-open", children: moveTargets },
      { label: "Download page again", icon: "rotate", onSelect: () => void items.refetch(item.id) },
      { label: "Open original", icon: "arrow-up-right-from-square", onSelect: () => openInBrowser(item.url), separatorAfter: true },
      { label: "Delete…", icon: "trash", danger: true, onSelect: () => (confirming = true) },
    ];
  });

  function onDragStart(e: DragEvent) {
    e.dataTransfer?.setData("text/stow-item-id", item.id);
    e.dataTransfer?.setData("text/plain", item.url);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events (the listbox handles keys for its rows) -->
<div
  class="item"
  class:selected
  class:done={!!item.done_at}
  class:fresh={highlighted}
  class:checked
  role="option"
  aria-selected={multi ? checked : selected}
  data-id={item.id}
  {tabindex}
  draggable="true"
  ondragstart={onDragStart}
  onclick={(e) => (onPick ? onPick(e) : ui.selectItem(item.id))}
  onfocus={() => !selected && ui.selectItem(item.id)}
  oncontextmenu={openMenu}
  ondblclick={() => openInBrowser(item.url)}
>
  <span class="handle" aria-hidden="true" title="Drag to a bucket"><Icon name="grip-vertical" size={12} /></span>
  <span class="icon" aria-hidden="true">
    {#if favicon}
      <img src={favicon} alt="" onerror={() => (iconFailed = true)} />
    {:else}
      <span class="letter">{letter}</span>
    {/if}
  </span>
  <div class="text">
    <div class="title truncate" {title}>{title}</div>
    <div class="meta">
      <span class="dot {item.snapshot_status}" role="img" aria-label={statusTitle} title={statusTitle}></span>
      {#if kindIcon}<span class="kind" title={kindLabel}><Icon name={kindIcon} size={10} label={kindLabel} /></span>{/if}
      {#if site}<span class="site truncate">{site}</span>{/if}
      {#if item.done_at}<span class="donechip">Archived</span>{/if}
    </div>
  </div>
  <span class="rowactions" class:shown={selected || checked}>
    {#if item.done_at}
      <button type="button" class="ghost icon small" tabindex="-1" title="Restore" aria-label="Restore {title}" onclick={(e) => { e.stopPropagation(); void restoreItem(item); }}><Icon name="rotate-left" size={12} /></button>
    {:else}
      <button type="button" class="ghost icon small" tabindex="-1" title="Archive (A)" aria-label="Archive {title}" onclick={(e) => { e.stopPropagation(); archiveItemLater(item); }}><Icon name="box-archive" size={12} /></button>
    {/if}
    <button type="button" class="ghost icon small danger" tabindex="-1" title="Delete (⌫)" aria-label="Delete {title}" onclick={(e) => { e.stopPropagation(); deleteItemLater(item); }}><Icon name="trash" size={12} /></button>
    <button type="button" class="ghost icon small" tabindex="-1" aria-label="More actions for {title}" aria-haspopup="menu" onclick={openMenu}><Icon name="ellipsis" /></button>
  </span>
</div>

{#if confirming}
  <div class="confirm-wrap">
    <ConfirmInline
      message={deleteConfirmCopy(item)}
      onConfirm={() => {
        confirming = false;
        deleteItemLater(item);
      }}
      onCancel={() => (confirming = false)}
    />
  </div>
{/if}

{#if menu}
  <Menu items={menuItems} x={menu.x} y={menu.y} label="Item actions" onClose={() => (menu = null)} />
{/if}

<style>
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 8px 8px 6px;
    border-bottom: 1px solid var(--hairline-soft);
    cursor: default;
    transition: background-color 0.4s ease;
  }
  .item:hover {
    background: var(--surface-doc);
  }
  .item.selected {
    background: var(--surface-soft);
  }
  .item.fresh {
    background: var(--accent-blue-soft);
  }
  .item.checked {
    background: var(--accent-blue-soft);
    box-shadow: inset 3px 0 0 var(--accent-blue);
  }
  .item:focus-visible {
    outline: 2px solid var(--accent-blue);
    outline-offset: -2px;
  }
  .handle {
    width: 12px;
    display: inline-grid;
    place-items: center;
    color: var(--mute);
    opacity: 0;
    cursor: grab;
  }
  .item:hover .handle {
    opacity: 1;
  }
  .icon {
    flex: none;
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-sm);
    background: var(--surface-soft);
    border: 1px solid var(--hairline-soft);
    overflow: hidden;
  }
  .item.selected .icon {
    background: var(--surface-card);
  }
  .icon img {
    width: 16px;
    height: 16px;
  }
  .letter {
    font-size: 12px;
    font-weight: 700;
    color: var(--mute);
  }
  .text {
    flex: 1;
    min-width: 0;
    line-height: 1.35;
  }
  .title {
    color: var(--ink);
    font-weight: 500;
  }
  .item.selected .title {
    font-weight: 600;
  }
  .item.done .title {
    color: var(--mute);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--mute);
  }
  .kind {
    display: inline-grid;
    place-items: center;
    color: var(--mute);
  }
  .site {
    min-width: 0;
  }
  .donechip {
    font-size: 11px;
    font-weight: 600;
    color: var(--ok-text);
  }
  /* Row actions: hover, keyboard focus within the row, or the row being selected. */
  .rowactions {
    display: inline-flex;
    flex: none;
    gap: 2px;
    opacity: 0;
  }
  .item:hover .rowactions,
  .item:focus-within .rowactions,
  .item:focus-visible .rowactions,
  .rowactions.shown {
    opacity: 1;
  }
  .confirm-wrap {
    padding: 6px 8px 8px 40px;
    border-bottom: 1px solid var(--hairline-soft);
  }
</style>
