<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import * as api from "../api";
  import { errorCode } from "../errors";
    import { reminders } from "../stores/reminders.svelte";
  import { buckets } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { settings } from "../stores/settings.svelte";
import { setup } from "../stores/setup.svelte";
    import { ui } from "../stores/ui.svelte";
  import type { Bucket, SearchHit } from "../types";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Menu, { menuAnchor, type MenuItem } from "./Menu.svelte";
  import SearchBar from "./SearchBar.svelte";
  import Icon from "./Icon.svelte";

  let searching = $state(false);
  let creating = $state(false);
  let newName = $state("");
  let menu = $state<{ x: number; y: number; bucket: Bucket } | null>(null);
  let renameId = $state<string | null>(null);
  let renameDraft = $state("");
  let emojiId = $state<string | null>(null);
  let emojiDraft = $state("");
  let deleteId = $state<string | null>(null);
  let dropTarget = $state<string | null | "inbox">(null);

  const sel = $derived(ui.selection);
  // "Get set up" entry: offered until dismissed or complete, and kept while it is the open view.
  const showSetup = $derived(setup.offered || (sel.kind === "setup" && !!settings.value && !settings.value.onboarding_dismissed));
  const blurTimers = new Set<ReturnType<typeof setTimeout>>();
  onDestroy(() => blurTimers.forEach(clearTimeout));
  /** Run after the current event (blur → click ordering), cancelled on destroy. */
  function later(fn: () => void) {
    const t = setTimeout(() => {
      blurTimers.delete(t);
      fn();
    }, 0);
    blurTimers.add(t);
  }

  // ⌘N anywhere opens the "New bucket" field.
  let seenTick = untrack(() => ui.newBucketTick);
  $effect(() => {
    const t = ui.newBucketTick;
    if (t !== seenTick) {
      seenTick = t;
      untrack(() => (creating = true));
    }
  });
  const isBucket = (id: string) => sel.kind === "bucket" && sel.id === id;

  async function onHit(h: SearchHit) {
    if (h.item_id) {
      // The hit's bucket_id can be stale (FTS index); the item row is authoritative.
      try {
        const it = await api.getItem(h.item_id);
        items.applyUpdated(it);
        // Archived links open in the Archived view, where their row is visible.
        ui.reveal(it);
      } catch (e) {
        if (errorCode(e) === "not_found") ui.toast.error("That item was deleted.");
        else ui.toast.error(e);
        return;
      }
    } else if (h.bucket_id) ui.select({ kind: "bucket", id: h.bucket_id });
    if (h.item_id) {
      const t = h.doc_type;
      if (t === "snapshot" || t === "page") ui.detailTab = "page";
      else if (t === "task") ui.detailTab = "tasks";
      else if (t === "attachment") ui.detailTab = "files";
      else if (t === "recording" || t === "transcript") ui.detailTab = "recordings";
      else if (t === "ai_run") ui.detailTab = "ai";
      else ui.detailTab = "notes";
    }
  }

  async function createBucket() {
    const name = newName.trim();
    if (!name) {
      creating = false;
      return;
    }
    newName = "";
    creating = false;
    const b = await buckets.create(name);
    if (b) ui.select({ kind: "bucket", id: b.id });
  }

  function openMenu(e: MouseEvent, bucket: Bucket) {
    e.preventDefault();
    e.stopPropagation();
    menu = { ...menuAnchor(e), bucket };
  }

  const menuItems = $derived.by((): MenuItem[] => {
    const b = menu?.bucket;
    if (!b) return [];
    const visible = buckets.list.filter((x) => !x.archived_at);
    const idx = visible.findIndex((x) => x.id === b.id);
    return [
      {
        label: "Rename",
        icon: "pen",
        onSelect: () => {
          renameDraft = b.name;
          renameId = b.id;
        },
      },
      {
        label: "Change emoji",
        icon: "face-smile",
        onSelect: () => {
          emojiDraft = b.emoji ?? "";
          emojiId = b.id;
        },
        separatorAfter: true,
      },
      { label: "Move up", icon: "arrow-up", disabled: idx <= 0, onSelect: () => void buckets.move(b.id, -1) },
      {
        label: "Move down",
        icon: "arrow-down",
        disabled: idx >= visible.length - 1,
        onSelect: () => void buckets.move(b.id, 1),
        separatorAfter: true,
      },
      { label: "Delete…", icon: "trash", danger: true, onSelect: () => (deleteId = b.id) },
    ];
  });

  async function commitRename() {
    const id = renameId;
    renameId = null;
    const name = renameDraft.trim();
    const b = buckets.byId(id);
    if (id && b && name && name !== b.name) await buckets.update(id, { name });
  }
  async function commitEmoji() {
    const id = emojiId;
    emojiId = null;
    const b = buckets.byId(id);
    const emoji = emojiDraft.trim();
    if (id && b && emoji !== (b.emoji ?? "")) await buckets.update(id, { emoji: emoji || null });
  }
  async function confirmDelete() {
    const id = deleteId;
    deleteId = null;
    if (!id) return;
    const ok = await buckets.remove(id);
    if (!ok) return;
    if (isBucket(id)) ui.select({ kind: "inbox" });
    // Its items moved to the Inbox: refresh the Inbox list if that is what is showing.
    else if (ui.selection.kind === "inbox" || ui.selection.kind === "all") void items.load();
    void items.refreshCounts();
  }

  function editKey(e: KeyboardEvent, commit: () => void, cancel: () => void) {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") cancel();
  }
  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  // ---- drag & drop of items onto buckets ----
  function hasItem(e: DragEvent) {
    return !!e.dataTransfer && Array.from(e.dataTransfer.types).includes("text/stow-item-id");
  }
  function onDragOver(e: DragEvent, target: string | "inbox") {
    if (!hasItem(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropTarget = target;
  }
  function onDragLeave(target: string | "inbox") {
    if (dropTarget === target) dropTarget = null;
  }
  async function onDrop(e: DragEvent, target: string | "inbox") {
    dropTarget = null;
    const id = e.dataTransfer?.getData("text/stow-item-id");
    if (!id) return;
    e.preventDefault();
    const bucketId = target === "inbox" ? null : target;
    const moved = await items.move(id, bucketId);
    if (moved) ui.toast.success(`Moved to ${buckets.nameOf(bucketId)}`);
  }
</script>

<nav class="sidebar" aria-label="Library">
  <div class="search" role="search">
    <SearchBar onSelect={onHit} onQueryChange={(q) => (searching = !!q.trim())} />
  </div>

  {#if !searching}
    <div class="nav">
      {#if showSetup}
        <button
          type="button"
          class="navitem"
          class:active={sel.kind === "setup"}
          aria-current={sel.kind === "setup" ? "page" : undefined}
          title="Finish setting up Stow"
          onclick={() => ui.select({ kind: "setup" })}
        >
          <span class="nav-icon"><Icon name="wand-magic-sparkles" fw /></span><span class="name">Get set up</span>
          <span class="count" aria-label="{setup.done} of {setup.total} steps done">{setup.done}/{setup.total}</span>
        </button>
      {/if}
      <button
        type="button"
        class="navitem"
        class:active={sel.kind === "inbox"}
        class:drop={dropTarget === "inbox"}
        aria-current={sel.kind === "inbox" ? "page" : undefined}
        title="Links you haven't sorted into a bucket yet."
        onclick={() => ui.select({ kind: "inbox" })}
        ondragover={(e) => onDragOver(e, "inbox")}
        ondragleave={() => onDragLeave("inbox")}
        ondrop={(e) => onDrop(e, "inbox")}
      >
        <span class="nav-icon"><Icon name="inbox" fw /></span><span class="name">Inbox</span>
        {#if items.inboxCount}<span class="count" aria-label="{items.inboxCount} open">{items.inboxCount}</span>{/if}
      </button>
      <button
        type="button"
        class="navitem"
        class:active={sel.kind === "today"}
        aria-current={sel.kind === "today" ? "page" : undefined}
        onclick={() => ui.select({ kind: "today" })}
      >
        <span class="nav-icon"><Icon name="calendar-day" fw /></span><span class="name">Today</span>
        {#if reminders.overdueCount}<span class="count overdue" aria-label="{reminders.overdueCount} overdue">{reminders.overdueCount}</span>{/if}
      </button>
      <button
        type="button"
        class="navitem"
        class:active={sel.kind === "archived"}
        aria-current={sel.kind === "archived" ? "page" : undefined}
        title="Links you've archived. Restore them any time."
        onclick={() => ui.select({ kind: "archived" })}
      >
        <span class="nav-icon"><Icon name="box-archive" fw /></span><span class="name">Archived</span>
      </button>
      <button
        type="button"
        class="navitem"
        class:active={sel.kind === "all"}
        aria-current={sel.kind === "all" ? "page" : undefined}
        onclick={() => ui.select({ kind: "all" })}
      >
        <span class="nav-icon"><Icon name="layer-group" fw /></span><span class="name">All items</span>
        {#if items.counts.all}<span class="count" aria-label="{items.counts.all} open">{items.counts.all}</span>{/if}
      </button>

      <div class="row group">
        <h2 class="section-title" id="buckets-heading">Buckets</h2>
        <span class="spacer"></span>
        <button type="button" class="ghost icon small" aria-label="New bucket" title="New bucket (⌘N)" onclick={() => (creating = true)}><Icon name="plus" /></button>
      </div>

      <ul class="buckets" aria-labelledby="buckets-heading">
        {#each buckets.list.filter((b) => !b.archived_at) as b (b.id)}
          <li>
          {#if renameId === b.id}
            <div class="row edit">
              <input
                type="text"
                aria-label="Bucket name"
                bind:value={renameDraft}
                use:autofocus
                onblur={() => later(() => renameId === b.id && commitRename())}
                onkeydown={(e) => editKey(e, commitRename, () => (renameId = null))}
              />
            </div>
          {:else if emojiId === b.id}
            <div class="row edit">
              <input
                type="text"
                class="emoji-input"
                aria-label="Emoji for {b.name}"
                placeholder="emoji"
                maxlength="4"
                bind:value={emojiDraft}
                use:autofocus
                onblur={() => later(() => emojiId === b.id && commitEmoji())}
                onkeydown={(e) => editKey(e, commitEmoji, () => (emojiId = null))}
              />
              <span class="muted small truncate">for {b.name}</span>
            </div>
          {:else}
            <div
              class="bucketrow"
              class:drop={dropTarget === b.id}
              role="presentation"
              oncontextmenu={(e) => openMenu(e, b)}
              ondragover={(e) => onDragOver(e, b.id)}
              ondragleave={() => onDragLeave(b.id)}
              ondrop={(e) => onDrop(e, b.id)}
            >
              <button
                type="button"
                class="navitem"
                class:active={isBucket(b.id)}
                aria-current={isBucket(b.id) ? "page" : undefined}
                onclick={() => ui.select({ kind: "bucket", id: b.id })}
              >
                <span class="emoji-box" aria-hidden="true">{b.emoji ?? "•"}</span>
                <span class="name truncate">{b.name}</span>
                {#if items.counts[b.id]}<span class="count" aria-label="{items.counts[b.id]} open">{items.counts[b.id]}</span>{/if}
              </button>
              <button type="button" class="ghost icon small more reveal" aria-label="Actions for {b.name}" aria-haspopup="menu" onclick={(e) => openMenu(e, b)}><Icon name="ellipsis" /></button>
            </div>
          {/if}
          {#if deleteId === b.id}
            <ConfirmInline
              message={`Delete “${b.name}”? Its items move to the Inbox.`}
              onConfirm={confirmDelete}
              onCancel={() => (deleteId = null)}
            />
          {/if}
          </li>
        {/each}
      </ul>

        {#if creating}
          <div class="row edit">
            <input
              type="text"
              aria-label="New bucket name"
              placeholder="Bucket name"
              bind:value={newName}
              use:autofocus
              onblur={() => later(() => creating && createBucket())}
              onkeydown={(e) => editKey(e, createBucket, () => { creating = false; newName = ""; })}
            />
          </div>
        {:else}
          <button type="button" class="ghost small newbucket" onclick={() => (creating = true)}><Icon name="plus" size={12} /> New bucket</button>
        {/if}
    </div>
  {/if}

  <div class="spacer"></div>
  <button
    type="button"
    class="navitem settings"
    class:active={sel.kind === "settings"}
    aria-current={sel.kind === "settings" ? "page" : undefined}
    onclick={() => ui.select({ kind: "settings" })}
  >
    <span class="nav-icon"><Icon name="gear" fw /></span><span class="name">Settings</span>
  </button>
</nav>

{#if menu}
  <Menu items={menuItems} x={menu.x} y={menu.y} label="Bucket actions" onClose={() => (menu = null)} />
{/if}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 10px 8px 8px;
    gap: 8px;
    background: var(--canvas);
    border-right: 1px solid var(--hairline);
    overflow: hidden;
    color: var(--body);
    font-size: 13.5px;
    font-weight: 500;
  }
  .search {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 0 1 auto;
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: 0;
    overflow: auto;
  }
  .group {
    margin-top: 12px;
    padding: 0 8px 4px;
  }
  .buckets {
    display: flex;
    flex-direction: column;
    gap: 2px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .bucketrow {
    position: relative;
    display: flex;
    align-items: center;
  }
  .bucketrow .navitem {
    flex: 1;
    min-width: 0;
    padding-right: 34px;
  }
  .bucketrow .more {
    position: absolute;
    right: 3px;
  }
  .bucketrow.drop .navitem {
    border-color: var(--accent-blue);
    background: var(--accent-blue-soft);
  }
  .navitem {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    width: 100%;
    height: 32px;
    padding: 0 6px 0 8px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    gap: 8px;
    cursor: default;
    background: transparent;
    color: var(--body);
    font-size: 13.5px;
    font-weight: 500;
    text-align: left;
  }
  .navitem:hover:not(:disabled) {
    background: var(--surface-soft);
  }
  .navitem.active,
  .navitem.active:hover:not(:disabled) {
    background: var(--surface-card);
    border-color: var(--hairline);
    color: var(--ink);
    font-weight: 600;
  }
  .navitem.drop {
    border-color: var(--accent-blue);
    background: var(--accent-blue-soft);
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .nav-icon {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    flex: none;
    color: var(--mute);
  }
  .navitem.active .nav-icon {
    color: var(--ink);
  }
  .count {
    font-size: 11px;
    font-weight: 600;
    color: var(--mute);
    padding: 0 7px;
    border-radius: 9999px;
    background: var(--surface-soft);
  }
  .navitem.active .count {
    background: var(--surface-soft);
  }
  .count.overdue {
    background: var(--accent-red-soft);
    color: var(--ink);
  }
  .bucketrow:hover .more {
    opacity: 1;
  }
  .group h2 {
    margin: 0;
  }
  .edit {
    padding: 0 2px;
  }
  .edit input {
    flex: 1;
    min-width: 0;
    height: var(--control-h-sm);
  }
  .emoji-input {
    width: 56px;
    flex: none !important;
    text-align: center;
  }
  .newbucket {
    justify-content: flex-start;
    color: var(--mute);
    margin-top: 2px;
  }
  .settings {
    flex: none;
  }
</style>
