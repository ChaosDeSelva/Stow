<script lang="ts">
  // Compact header over the item list: "Inbox · 14" (or bucket emoji/name/count),
  // the one primary "Add link" button (⌘L, inline form below), sort menu,
  // bucket ⋯ menu, kind pills and the "Show archived (N)" toggle.
  import { doneCount, openCount, SORT_LABELS, type SortOrder } from "../listview";
  import { buckets } from "../stores/buckets.svelte";
  import { deletions } from "../stores/deletions.svelte";
  import { items } from "../stores/items.svelte";
  import { ui, type KindFilter } from "../stores/ui.svelte";
  import type { Bucket } from "../types";
  import AddLinkForm from "./AddLinkForm.svelte";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Icon from "./Icon.svelte";
  import Menu, { menuAnchor, type MenuItem } from "./Menu.svelte";

  interface Props {
    bucket?: Bucket;
  }
  let { bucket }: Props = $props();

  let menu = $state<{ x: number; y: number; items: MenuItem[]; label: string } | null>(null);
  let editing = $state(false);
  let nameDraft = $state("");
  let emojiDraft = $state("");
  let confirmDelete = $state(false);

  const KINDS: { id: KindFilter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "video", label: "Videos" },
    { id: "article", label: "Articles" },
  ];

  const hidden = (id: string) => deletions.isHidden(id);
  const archivedView = $derived(ui.selection.kind === "archived");
  const total = $derived(
    archivedView ? doneCount(items.list, { kind: "all", hidden }) : openCount(items.list, { kind: "all", hidden }),
  );
  const done = $derived(doneCount(items.list, { kind: ui.kindFilter, hidden }));
  const showDone = $derived(ui.showDone(ui.listKey));
  const title = $derived(
    bucket ? bucket.name : ui.selection.kind === "all" ? "All items" : archivedView ? "Archived" : "Inbox",
  );
  const headIcon = $derived(ui.selection.kind === "all" ? "layer-group" : archivedView ? "box-archive" : "inbox");

  function openSort(e: MouseEvent) {
    const a = menuAnchor(e);
    menu = {
      ...a,
      label: "Sort",
      items: (Object.keys(SORT_LABELS) as SortOrder[]).map((k) => ({
        label: SORT_LABELS[k],
        checked: ui.sortOrder === k,
        onSelect: () => (ui.sortOrder = k),
      })),
    };
  }

  function openBucketMenu(e: MouseEvent) {
    if (!bucket) return;
    const b = bucket;
    const visible = buckets.list.filter((x) => !x.archived_at);
    const idx = visible.findIndex((x) => x.id === b.id);
    menu = {
      ...menuAnchor(e),
      label: "Bucket actions",
      items: [
        { label: "Rename", icon: "pen", onSelect: startEdit },
        { label: "Change emoji", icon: "face-smile", onSelect: startEdit, separatorAfter: true },
        { label: "Move up", icon: "arrow-up", disabled: idx <= 0, onSelect: () => void buckets.move(b.id, -1) },
        { label: "Move down", icon: "arrow-down", disabled: idx < 0 || idx >= visible.length - 1, onSelect: () => void buckets.move(b.id, 1), separatorAfter: true },
        { label: "Delete…", icon: "trash", danger: true, onSelect: () => (confirmDelete = true) },
      ],
    };
  }

  function startEdit() {
    if (!bucket) return;
    nameDraft = bucket.name;
    emojiDraft = bucket.emoji ?? "";
    editing = true;
  }
  async function commit() {
    if (!editing || !bucket) return;
    editing = false;
    const name = nameDraft.trim();
    const emoji = emojiDraft.trim();
    const patch: { name?: string; emoji?: string | null } = {};
    if (name && name !== bucket.name) patch.name = name;
    if (emoji !== (bucket.emoji ?? "")) patch.emoji = emoji || null;
    if (Object.keys(patch).length) await buckets.update(bucket.id, patch);
  }
  function onEditKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void commit();
    } else if (e.key === "Escape") editing = false;
  }
  /** Commit only when focus leaves the whole edit row (emoji ↔ name tabbing stays open). */
  function onEditFocusOut(e: FocusEvent) {
    const next = e.relatedTarget as Node | null;
    if (next && (e.currentTarget as HTMLElement).contains(next)) return;
    void commit();
  }
  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }
  async function doDelete() {
    confirmDelete = false;
    if (!bucket) return;
    const id = bucket.id;
    if (await buckets.remove(id)) {
      ui.select({ kind: "inbox" });
      void items.refreshCounts();
    }
  }
</script>

<div class="list-header">
  <div class="title-row">
    {#if editing}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="editrow" onfocusout={onEditFocusOut}>
        <input type="text" class="emoji" aria-label="Bucket emoji" placeholder="emoji" bind:value={emojiDraft} onkeydown={onEditKey} maxlength="4" />
        <input type="text" class="name" aria-label="Bucket name" bind:value={nameDraft} onkeydown={onEditKey} use:autofocus />
      </div>
    {:else}
      <h2 class="title">
        {#if bucket}
          <span class="emoji-box" aria-hidden="true">{bucket.emoji ?? "•"}</span>
        {:else}
          <Icon name={headIcon} fw />
        {/if}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span class="truncate" ondblclick={startEdit}>{title}</span>
        <span class="count">· {total}</span>
      </h2>
      <span class="spacer"></span>
      {#if !archivedView}
        <button type="button" class="small primary" data-addlink-toggle title="Add one or more links (⌘L)" aria-expanded={ui.addLinkOpen} aria-controls={ui.addLinkOpen ? "addlink-url" : undefined} onclick={() => (ui.addLinkOpen ? ui.closeAddLink() : ui.openAddLink())}>
          <Icon name="plus" size={11} /> Add link
        </button>
      {/if}
      <button type="button" class="ghost small" aria-haspopup="menu" onclick={openSort}>
        <Icon name="arrow-down-wide-short" size={12} /> {SORT_LABELS[ui.sortOrder]}
      </button>
      {#if bucket}
        <button type="button" class="ghost icon small" aria-label="Bucket actions" aria-haspopup="menu" onclick={openBucketMenu}><Icon name="ellipsis" /></button>
      {/if}
    {/if}
  </div>
  {#if ui.selection.kind === "inbox" && !editing}
    <p class="subtitle muted small">Unsorted links</p>
  {/if}
  <div class="filter-row">
    <div class="product-tabs" role="group" aria-label="Kind">
      {#each KINDS as k (k.id)}
        <button type="button" aria-pressed={ui.kindFilter === k.id} class:active={ui.kindFilter === k.id} onclick={() => (ui.kindFilter = k.id)}>{k.label}</button>
      {/each}
    </div>
    <span class="spacer"></span>
    {#if !archivedView && (done > 0 || showDone)}
      <button type="button" class="ghost small" aria-pressed={showDone} onclick={() => ui.setShowDone(ui.listKey, !showDone)}>
        {showDone ? "Hide archived" : `Show archived (${done})`}
      </button>
    {/if}
  </div>
  {#if ui.addLinkOpen && !archivedView}
    <AddLinkForm bucketId={bucket?.id ?? null} onClose={() => ui.closeAddLink()} />
  {/if}
  {#if confirmDelete && bucket}
    <ConfirmInline message={`Delete “${bucket.name}”? Its items move to the Inbox.`} onConfirm={doDelete} onCancel={() => (confirmDelete = false)} />
  {/if}
</div>

{#if menu}
  <Menu items={menu.items} x={menu.x} y={menu.y} label={menu.label} onClose={() => (menu = null)} />
{/if}

<style>
  .list-header {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--hairline);
    background: var(--canvas);
  }
  .title-row,
  .filter-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    font-size: 14px;
  }
  .title .truncate {
    min-width: 0;
  }
  .title-row > button,
  .filter-row > button {
    flex: none;
  }
  .count {
    color: var(--mute);
    font-weight: 600;
    flex: none;
  }
  .subtitle {
    margin: -4px 0 0 30px;
  }
  .editrow {
    flex: 1;
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  .editrow input {
    height: var(--control-h-sm);
  }
  .emoji {
    width: 52px;
    text-align: center;
  }
  .name {
    flex: 1;
  }
</style>
