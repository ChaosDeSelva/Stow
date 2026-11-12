<script lang="ts">
  // Single tab stop listbox with roving focus. Keys: ↑/↓ select, ⏎ open original,
  // A (or D) archive / restore, M move, ⌫ delete (undoable), ⌘A select all.
  // ⌘-click toggles, ⇧-click extends the multi-selection; a bulk bar then
  // offers Archive, Move to… and Delete (Archived view: Restore and Delete).
  // Esc: the Add link form first, then the move popover, then the multi-selection.
  // None of the letter / ⌫ / ⌘A keys fire while typing or inside a menu or dialog.
  import { tick } from "svelte";
  import { friendlyText } from "../errors";
  import { hotkeyGlyphs } from "../hotkey";
  import { archiveMany, deleteMany, restoreMany, toggleDone } from "../itemActions";
  import { openInBrowser } from "../links";
  import { visibleItems } from "../listview";
  import { rangeIds, selectAll, toggleId } from "../selection";
  import { buckets } from "../stores/buckets.svelte";
  import { deletions } from "../stores/deletions.svelte";
  import { items } from "../stores/items.svelte";
  import { settings } from "../stores/settings.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { Item } from "../types";
  import { isEditable } from "../util";
  import Icon from "./Icon.svelte";
  import ItemRow from "./ItemRow.svelte";
  import MovePopover from "./MovePopover.svelte";

  let listEl = $state<HTMLDivElement | null>(null);
  let moving = $state<{ ids: string[]; x: number; y: number } | null>(null);
  /** Set once the list showed rows, so emptying it reads as "Inbox zero", not "nothing yet". */
  let hadRows = $state(false);
  /** Last plainly clicked row: the anchor for ⇧-click ranges. */
  let anchorId: string | null = null;

  const archivedView = $derived(ui.selection.kind === "archived");
  const rows = $derived(
    visibleItems(items.list, {
      kind: ui.kindFilter,
      sort: ui.sortOrder,
      showDone: ui.showDone(ui.listKey),
      archivedOnly: archivedView,
      hidden: (id) => deletions.isHidden(id),
    }),
  );
  const selIndex = $derived(rows.findIndex((r) => r.id === ui.selectedItemId));
  const focusIndex = $derived(selIndex >= 0 ? selIndex : 0);
  const hotkey = $derived(hotkeyGlyphs(settings.value?.hotkey ?? "Ctrl+Cmd+S"));
  const filtered = $derived(ui.kindFilter !== "all");
  const bucketName = $derived(ui.selection.kind === "bucket" ? buckets.nameOf(ui.selection.id) : "");
  /** Checked rows that are still visible (the batch the bulk bar acts on). */
  const checkedRows = $derived(rows.filter((r) => ui.isChecked(r.id)));
  /** Every checked row is archived (always true in the Archived view): the bulk bar offers Restore. */
  const checkedAllDone = $derived(checkedRows.length > 0 && checkedRows.every((r) => !!r.done_at));

  $effect(() => {
    if (rows.length) hadRows = true;
  });

  // Keep the open row in view (e.g. "Show them" after a batch, search hits).
  $effect(() => {
    const id = ui.selectedItemId;
    if (!id || !listEl) return;
    rowEl(id)?.scrollIntoView?.({ block: "nearest" });
  });

  /** Typing somewhere, or inside a menu / dialog / confirm: list shortcuts stay out of the way. */
  function busy(target: EventTarget | null): boolean {
    const el = target as Element | null;
    if (isEditable(el) || isEditable(document.activeElement)) return true;
    return !!el?.closest?.("[role=menu],[role=dialog],[role=alertdialog]");
  }

  /** Row to focus once `batch` leaves the list: the next one below, else the one above. */
  function survivorAfter(batch: Item[]): string | undefined {
    const gone = new Set(batch.map((b) => b.id));
    const first = rows.findIndex((r) => gone.has(r.id));
    const start = first >= 0 ? first : focusIndex;
    return rows.slice(start).find((r) => !gone.has(r.id))?.id ?? rows.slice(0, start).reverse().find((r) => !gone.has(r.id))?.id;
  }

  /** Run an action that removes `batch` from this list, then keep keyboard focus in the list. */
  function removing(batch: Item[], act: () => unknown) {
    const next = survivorAfter(batch);
    const hadFocus = !!listEl?.contains(document.activeElement) || !!(document.activeElement as Element | null)?.closest?.(".bulkbar");
    void act();
    if (next && hadFocus) void focusId(next);
  }
  function archiveOrRestore(batch: Item[]) {
    if (!batch.length) return;
    // Archiving hides rows at once (undo window); restoring leaves Archived. Only a restore
    // inside a normal list ("Show archived") keeps the rows where they are.
    const allDone = batch.every((b) => b.done_at);
    const leaves = archivedView || !allDone;
    const act = () => (batch.length === 1 ? toggleDone(batch[0]) : allDone ? restoreMany(batch) : archiveMany(batch));
    if (leaves) removing(batch, act);
    else void act();
  }
  function remove(batch: Item[]) {
    if (batch.length) removing(batch, () => deleteMany(batch));
  }

  /** The row element for an id (no selector escaping needed). */
  function rowEl(id: string): HTMLElement | undefined {
    return Array.from(listEl?.querySelectorAll<HTMLElement>("[data-id]") ?? []).find((el) => el.dataset.id === id);
  }

  async function focusId(id: string) {
    ui.selectItem(id);
    await tick();
    rowEl(id)?.focus();
  }
  function focusRow(i: number) {
    const r = rows[i];
    return r ? focusId(r.id) : Promise.resolve();
  }

  function pick(item: Item, e: MouseEvent) {
    if (e.metaKey || e.ctrlKey) {
      // Like Finder: ⌘-clicking a second row keeps the row that was already open.
      const open = ui.selectedItemId;
      const seed = !ui.selectedIds.size && open && open !== item.id && rows.some((r) => r.id === open) ? new Set([open]) : ui.selectedIds;
      ui.setSelectedIds(toggleId(seed, item.id));
      anchorId = item.id;
    } else if (e.shiftKey) {
      ui.setSelectedIds(rangeIds(rows.map((r) => r.id), anchorId ?? ui.selectedItemId, item.id, ui.selectedIds));
    } else {
      ui.clearSelectedIds();
      ui.selectItem(item.id);
      anchorId = item.id;
    }
  }

  function openMove(ids: string[], near?: HTMLElement | null) {
    const r = near?.getBoundingClientRect();
    moving = { ids, x: r ? r.left + 40 : 100, y: r ? r.bottom : 100 };
  }

  function onKey(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey || e.isComposing) return;
    if (busy(e.target)) return;
    const cur = rows[focusIndex];
    const batch = checkedRows.length ? checkedRows : cur ? [cur] : [];
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        void focusRow(Math.min(rows.length - 1, selIndex + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        void focusRow(Math.max(0, selIndex - 1));
        break;
      case "Home":
        e.preventDefault();
        void focusRow(0);
        break;
      case "End":
        e.preventDefault();
        void focusRow(rows.length - 1);
        break;
      case "Enter":
        if (cur) {
          e.preventDefault();
          openInBrowser(cur.url);
        }
        break;
      case "m":
      case "M":
        if (batch.length && !archivedView) {
          e.preventDefault();
          openMove(batch.map((b) => b.id), rowEl(batch[0].id));
        }
        break;
      case "a":
      case "A":
      case "d":
      case "D":
        if (batch.length) {
          e.preventDefault();
          archiveOrRestore(batch);
        }
        break;
      case "Backspace":
      case "Delete":
        if (batch.length) {
          e.preventDefault();
          remove(batch);
        }
        break;
    }
  }

  // Window-level: ⌘A (select all visible rows) and Esc (form > popover > multi-selection).
  // Menus, popovers, pickers and the form stop their own Esc, so it never reaches here twice.
  function onWindowKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (e.defaultPrevented || e.isComposing) return;
      const t = e.target as Element | null;
      if (t?.closest?.("[role=menu],[role=dialog],[role=alertdialog]")) return;
      if (ui.addLinkOpen) {
        if (isEditable(t) && !t?.closest?.(".addlink")) return;
        e.preventDefault();
        ui.closeAddLink();
      } else if (moving) {
        e.preventDefault();
        moving = null;
      } else if (ui.selectedIds.size && !isEditable(t)) {
        e.preventDefault();
        ui.clearSelectedIds();
      }
      return;
    }
    if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== "a") return;
    if (busy(e.target) || !rows.length) return;
    e.preventDefault();
    ui.setSelectedIds(selectAll(rows.map((r) => r.id)));
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#if items.loading && !rows.length}
  <div class="muted small pad" role="status">Loading…</div>
{:else if items.loadError}
  <div class="empty" role="alert">
    <div class="strong">Couldn't load this list.</div>
    <div class="small">{friendlyText(items.loadError)}</div>
    <button type="button" class="small" onclick={() => items.load()}>Retry</button>
  </div>
{:else if !rows.length}
  <div class="empty">
    {#if filtered}
      <div class="empty-icon"><Icon name="magnifying-glass" size={28} /></div>
      <div class="strong">Nothing here with this filter.</div>
    {:else if archivedView}
      <div class="empty-icon"><Icon name="box-archive" size={28} /></div>
      <div class="strong">Nothing archived yet.</div>
      <div class="small">Press <kbd>A</kbd> on a link, or hover it and click Archive, when you're done with it.</div>
    {:else if ui.selection.kind === "bucket"}
      <div class="empty-icon"><Icon name="folder-open" size={28} /></div>
      <div class="strong">No links in {bucketName} yet.</div>
      <div class="ways">
        <button type="button" class="small" title="Add one or more links (⌘L)" onclick={() => ui.openAddLink()}><Icon name="plus" size={11} /> Add link</button>
        <kbd>⌘L</kbd>
      </div>
    {:else if hadRows && ui.selection.kind === "inbox"}
      <div class="empty-icon"><Icon name="circle-check" style="regular" size={28} /></div>
      <div class="strong">Inbox zero. Nice.</div>
    {:else}
      <div class="strong intro">
        <Icon name="inbox" class="intro-icon" />
        {#if ui.selection.kind === "inbox"}Inbox is where new links land before you sort them into a bucket.{:else}Nothing stowed yet.{/if}
      </div>
      <ul class="ways-list">
        <li>
          <button type="button" class="small" title="Add one or more links (⌘L)" onclick={() => ui.openAddLink()}><Icon name="plus" size={11} /> Add link</button>
          <kbd>⌘L</kbd>
        </li>
        <li>Press <kbd>{hotkey}</kbd> over any Chrome tab</li>
        <li>Paste a link anywhere (<kbd>⌘V</kbd>)</li>
      </ul>
    {/if}
  </div>
{/if}

<div class="sr-only" role="status" aria-live="polite">{checkedRows.length ? `${checkedRows.length} selected` : ""}</div>
{#if checkedRows.length}
  <div class="bulkbar" role="toolbar" aria-label="Selected items">
    <span class="strong">{checkedRows.length} selected</span>
    <span class="sep" aria-hidden="true">·</span>
    {#if checkedAllDone}
      <button type="button" class="small" onclick={() => archiveOrRestore(checkedRows)}><Icon name="rotate-left" size={11} /> Restore</button>
    {:else}
      <button type="button" class="small" onclick={() => archiveOrRestore(checkedRows)}><Icon name="box-archive" size={11} /> Archive</button>
    {/if}
    {#if !archivedView}
      <button type="button" class="small" onclick={(e) => openMove(checkedRows.map((r) => r.id), e.currentTarget as HTMLElement)}><Icon name="folder-open" size={11} /> Move to…</button>
    {/if}
    <button type="button" class="small ghost danger" onclick={() => remove(checkedRows)}><Icon name="trash" size={11} /> Delete</button>
    <span class="sep" aria-hidden="true">·</span>
    <button type="button" class="ghost small" aria-label="Clear selection" title="Clear selection (esc)" onclick={() => { ui.clearSelectedIds(); void focusRow(focusIndex); }}>Clear</button>
  </div>
{/if}

{#if rows.length}
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="list" role="listbox" aria-label="Items" aria-multiselectable="true" bind:this={listEl} onkeydown={onKey}>
    {#each rows as item, i (item.id)}
      <ItemRow
        {item}
        selected={ui.selectedItemId === item.id}
        checked={ui.isChecked(item.id)}
        multi={checkedRows.length > 0}
        tabindex={i === focusIndex ? 0 : -1}
        highlighted={ui.isHighlighted(item.id)}
        onPick={(e) => pick(item, e)}
      />
    {/each}
  </div>
  <p class="hint small muted">↑↓ select · ⏎ open · {archivedView ? "A restore" : "A archive · M move"} · ⌫ delete · ⌘-click / ⇧-click / ⌘A multi-select</p>
{/if}

{#if moving}
  <MovePopover itemIds={moving.ids} x={moving.x} y={moving.y} onClose={() => (moving = null)} />
{/if}

<style>
  .list {
    display: flex;
    flex-direction: column;
  }
  .pad {
    padding: 10px 12px;
  }
  .empty-icon {
    color: var(--mute);
    line-height: 1;
    margin-bottom: 10px;
  }
  .empty :global(.intro-icon) {
    color: var(--mute);
    margin-right: 4px;
  }
  /* One centred block (text-align from the global .empty); the inbox icon sits inline on the first line. */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    max-width: 420px;
    margin: 48px auto;
    padding: 0 16px;
  }
  .strong {
    color: var(--ink);
    font-weight: 600;
  }
  .intro {
    display: block;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .ways,
  .ways-list li {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }
  .ways-list {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12.5px;
    color: var(--body);
  }
  .bulkbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--hairline);
    background: var(--accent-blue-soft);
    font-size: 12.5px;
    color: var(--ink);
  }
  .sep {
    color: var(--mute);
  }
  .hint {
    margin: 0;
    padding: 8px 12px;
    text-align: center;
  }
</style>
