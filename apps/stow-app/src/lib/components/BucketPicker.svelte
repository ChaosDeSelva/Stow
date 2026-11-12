<script lang="ts">
  // Type-ahead bucket picker. Rows: Inbox, matching buckets, and
  // "New bucket “<typed>”" which creates the bucket on pick.
  import { onDestroy } from "svelte";
  import { buckets } from "../stores/buckets.svelte";
  import type { Bucket } from "../types";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Current bucket (null = Inbox); undefined = none (e.g. a batch from several buckets). */
    value: string | null | undefined;
    onChange: (bucketId: string | null) => void;
    autofocus?: boolean;
    placeholder?: string;
    /** Called when Enter is pressed with the dropdown closed (e.g. submit a form). */
    onSubmit?: () => void;
    label?: string;
  }
  let { value, onChange, autofocus = false, placeholder = "Bucket…", onSubmit, label = "Bucket" }: Props = $props();
  const uid = $props.id();

  type Row = { kind: "inbox" } | { kind: "bucket"; bucket: Bucket } | { kind: "new"; name: string };

  let query = $state("");
  let open = $state(false);
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let creating = $state(false);
  /** Max dropdown height: the space left below the field (capture window is short). */
  let maxH = $state(220);
  let blurTimer: ReturnType<typeof setTimeout> | null = null;

  const selectedLabel = $derived(value === undefined ? "" : buckets.nameOf(value));

  const rows = $derived.by((): Row[] => {
    const q = query.trim().toLowerCase();
    const out: Row[] = [];
    if (!q || "inbox".includes(q)) out.push({ kind: "inbox" });
    for (const b of buckets.list) {
      if (b.archived_at) continue;
      if (!q || b.name.toLowerCase().includes(q) || (b.emoji ?? "").includes(q)) {
        out.push({ kind: "bucket", bucket: b });
      }
    }
    const typed = query.trim();
    if (typed && !buckets.list.some((b) => b.name.toLowerCase() === typed.toLowerCase())) {
      out.push({ kind: "new", name: typed });
    }
    return out;
  });
  const activeIndex = $derived(Math.min(active, Math.max(0, rows.length - 1)));

  $effect(() => {
    if (autofocus && input) input.focus();
  });

  onDestroy(() => {
    if (blurTimer) clearTimeout(blurTimer);
  });

  function measure() {
    if (!input) return;
    const r = input.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12;
    maxH = Math.max(96, Math.min(220, below));
  }

  function show() {
    measure();
    open = true;
    const idx = rows.findIndex((r) =>
      value === null ? r.kind === "inbox" : r.kind === "bucket" && r.bucket.id === value,
    );
    active = idx >= 0 ? idx : 0;
  }

  function hide() {
    open = false;
    query = "";
  }

  /** Focus the field and open the list (used by the capture panel). */
  export function focus() {
    input?.focus();
  }

  async function pick(row: Row) {
    if (row.kind === "inbox") onChange(null);
    else if (row.kind === "bucket") onChange(row.bucket.id);
    else {
      creating = true;
      const b = await buckets.create(row.name);
      creating = false;
      if (b) onChange(b.id);
    }
    hide();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (!open) show();
      else active = (activeIndex + 1) % Math.max(1, rows.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) show();
      else active = (activeIndex - 1 + rows.length) % Math.max(1, rows.length);
    } else if (e.key === "Enter") {
      if (e.isComposing) return;
      const row = open ? rows[activeIndex] : undefined;
      if (!row && !onSubmit) return;
      e.preventDefault();
      e.stopPropagation();
      // "Type to pick a bucket · ⏎ save": Enter picks the highlighted row, then submits.
      if (row) void pick(row).then(() => onSubmit?.());
      else onSubmit?.();
    } else if (e.key === "Escape") {
      if (open) {
        e.preventDefault();
        e.stopPropagation();
        hide();
      }
    } else if (e.key === "Tab") {
      hide();
    }
  }

  function onInput(e: Event) {
    query = (e.target as HTMLInputElement).value;
    if (!open) show();
    active = 0;
  }

  function onBlur() {
    // Let a click on a row land first.
    blurTimer = setTimeout(() => {
      blurTimer = null;
      if (document.activeElement !== input) hide();
    }, 120);
  }

  function rowId(i: number) {
    return `${uid}-opt-${i}`;
  }
</script>

<div class="picker" class:open>
  <input
    bind:this={input}
    type="text"
    role="combobox"
    aria-label={label}
    aria-expanded={open}
    aria-controls="{uid}-list"
    aria-autocomplete="list"
    aria-activedescendant={open && rows.length ? rowId(activeIndex) : undefined}
    placeholder={open ? "Type to filter or create…" : placeholder}
    value={open ? query : selectedLabel}
    onfocus={show}
    onclick={() => !open && show()}
    oninput={onInput}
    onkeydown={onKey}
    onblur={onBlur}
    disabled={creating}
    autocomplete="off"
    spellcheck="false"
  />
  <span class="caret" aria-hidden="true"><Icon name="chevron-down" size={10} /></span>
  {#if open}
    <div class="list floating" id="{uid}-list" role="listbox" aria-label={label} style="max-height:{maxH}px">
      {#each rows as row, i}
        <div
          class="row"
          id={rowId(i)}
          class:active={i === activeIndex}
          role="option"
          aria-selected={i === activeIndex}
          tabindex="-1"
          onmousedown={(e) => e.preventDefault()}
          onmouseenter={() => (active = i)}
          onclick={() => pick(row)}
          onkeydown={(e) => e.key === "Enter" && pick(row)}
        >
          {#if row.kind === "inbox"}
            <span class="emoji-box"><Icon name="inbox" size={11} /></span><span class="name truncate">Inbox</span>
          {:else if row.kind === "bucket"}
            <span class="emoji-box">{row.bucket.emoji ?? "•"}</span><span class="name truncate">{row.bucket.name}</span>
          {:else}
            <span class="emoji-box"><Icon name="plus" size={11} /></span><span class="name truncate">New bucket “{row.name}”</span>
          {/if}
          {#if (row.kind === "inbox" && value === null) || (row.kind === "bucket" && row.bucket.id === value)}
            <span class="tick"><Icon name="check" size={11} label="Current" /></span>
          {/if}
        </div>
      {/each}
      {#if !rows.length}
        <div class="row muted">No buckets</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  input {
    width: 100%;
    padding-right: 26px;
  }
  .caret {
    position: absolute;
    right: 10px;
    top: 45%;
    transform: translateY(-50%);
    color: var(--mute);
    pointer-events: none;
  }
  .list {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(100% + 4px);
    z-index: 50;
    overflow: auto;
    padding: 4px;
  }
  /* [emoji] [label grows + truncates] [check, fixed at the end]: nothing absolute, nothing overlaps. */
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px 5px 5px;
    border-radius: var(--radius-sm);
    color: var(--ink);
    font-weight: 500;
  }
  .row.active {
    background: var(--surface-soft);
  }
  .name {
    flex: 1 1 auto;
    min-width: 0;
  }
  .tick {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 14px;
    font-size: 11px;
    color: var(--mute);
  }
</style>
