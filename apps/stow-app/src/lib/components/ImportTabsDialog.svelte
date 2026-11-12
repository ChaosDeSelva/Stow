<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../api";
  import { friendlyText } from "../errors";
  import { rememberFocus } from "../focus";
  import { importSummary } from "../util";
  import { buckets } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { toast, ui } from "../stores/ui.svelte";
  import { localDateStamp } from "../time";
  import type { ChromeTab } from "../types";
  import BucketPicker from "./BucketPicker.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  type Row = { tab: ChromeTab; ok: boolean; checked: boolean };

  let rows = $state<Row[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let closeTabs = $state(false);
  let importing = $state(false);
  /** null = Inbox; "__new__" = create "Tabs <date>" on confirm. */
  let bucketChoice = $state<string | null>("__new__");
  const newName = `Tabs ${localDateStamp()}`;

  const isHttp = (u: string) => /^https?:\/\//i.test(u);
  const selected = $derived(rows.filter((r) => r.ok && r.checked));
  const allChecked = $derived(rows.filter((r) => r.ok).every((r) => r.checked));

  const restoreFocus = rememberFocus();
  onMount(() => {
    void load();
    return restoreFocus;
  });


  async function load() {
    loading = true;
    error = null;
    try {
      const tabs = await api.listChromeTabs();
      rows = tabs.map((t) => ({ tab: t, ok: isHttp(t.url), checked: isHttp(t.url) }));
    } catch (e) {
      error = friendlyText(e);
    } finally {
      loading = false;
    }
  }

  function toggleAll(on: boolean) {
    rows = rows.map((r) => (r.ok ? { ...r, checked: on } : r));
  }

  async function confirm() {
    if (!selected.length) return;
    importing = true;
    try {
      let bucketId: string | null = bucketChoice === "__new__" ? null : bucketChoice;
      if (bucketChoice === "__new__") {
        const b = await buckets.create(newName);
        if (!b) return;
        bucketId = b.id;
      }
      const res = await api.importChromeTabs(
        selected.map((r) => r.tab),
        bucketId,
        closeTabs,
      );
      toast.success(importSummary(res.imported, res.skipped));
      void items.load();
      void items.refreshCounts();
      if (bucketId) ui.select({ kind: "bucket", id: bucketId });
      onClose();
    } catch (e) {
      toast.error(e);
    } finally {
      importing = false;
    }
  }

  let dialogEl = $state<HTMLDivElement | null>(null);

  function focusables(): HTMLElement[] {
    if (!dialogEl) return [];
    return Array.from(
      dialogEl.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
      ),
    );
  }

  // Focus the first control when the dialog (or its content) appears.
  $effect(() => {
    void loading;
    void error;
    const el = dialogEl;
    if (!el) return;
    queueMicrotask(() => {
      if (el.contains(document.activeElement)) return;
      (focusables()[0] ?? el).focus();
    });
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    } else if (e.key === "Tab") {
      // Trap Tab inside the dialog.
      const f = focusables();
      if (!f.length) return;
      const first = f[0];
      const last = f[f.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey && (active === first || !dialogEl?.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !dialogEl?.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onmousedown={(e) => e.target === e.currentTarget && onClose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="import-title" tabindex="-1" bind:this={dialogEl}>
    <h3 id="import-title">Import all Chrome tabs</h3>

    {#if loading}
      <div class="muted small">Reading tabs from Chrome…</div>
    {:else if error}
      <div class="banner red"><Icon name="triangle-exclamation" class="banner-icon" /><span>{error}</span></div>
    {:else}
      <div class="row summary">
        <span>{rows.length} tab{rows.length === 1 ? "" : "s"} open, {selected.length} selected</span>
        <span class="spacer"></span>
        <button type="button" class="small ghost" onclick={() => toggleAll(!allChecked)}>{allChecked ? "Deselect all" : "Select all"}</button>
        <button type="button" class="small ghost" onclick={load}><Icon name="rotate" size={11} /> Refresh</button>
      </div>

      <div class="tabs">
        {#each rows as r, i (`${r.tab.window_index}:${r.tab.tab_index}`)}
          <label class="tab" class:off={!r.ok}>
            <input type="checkbox" aria-label={r.tab.title || r.tab.url} disabled={!r.ok} checked={r.checked} onchange={(e) => (rows[i] = { ...r, checked: (e.target as HTMLInputElement).checked })} />
            <span class="t truncate" title={r.tab.url}>{r.tab.title || r.tab.url}</span>
            <span class="u muted small truncate">{r.ok ? r.tab.url : `${r.tab.url} (skipped: not a web page)`}</span>
          </label>
        {/each}
      </div>

      <div class="grid">
        <span class="muted">Into</span>
        <div class="row">
          <label class="check">
            <input type="radio" name="dest" checked={bucketChoice === "__new__"} onchange={() => (bucketChoice = "__new__")} />
            New bucket “{newName}”
          </label>
        </div>
        <span></span>
        <div class="row">
          <label class="check">
            <input type="radio" name="dest" checked={bucketChoice !== "__new__"} onchange={() => (bucketChoice = null)} />
            Existing:
          </label>
          <div class="picker">
            <BucketPicker
              value={bucketChoice === "__new__" ? null : bucketChoice}
              onChange={(id) => (bucketChoice = id)}
            />
          </div>
        </div>
        <span></span>
        <label class="check">
          <input type="checkbox" bind:checked={closeTabs} />
          Close imported tabs in Chrome
        </label>
      </div>
    {/if}

    <div class="row actions">
      <span class="spacer"></span>
      <button type="button" onclick={onClose}>Cancel</button>
      <button type="button" class="primary" disabled={loading || !!error || !selected.length || importing} onclick={confirm}>
        <Icon name="file-import" size={12} /> {importing ? "Importing…" : `Import ${selected.length}`}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 500;
    background: var(--scrim);
    display: grid;
    place-items: center;
  }
  .dialog {
    width: 580px;
    max-width: calc(100vw - 40px);
    max-height: calc(100vh - 60px);
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 24px;
    border-radius: var(--radius);
    background: var(--surface-card);
    border: 1px solid var(--stone);
  }
  h3 {
    margin: 0;
    font-size: 18px;
  }
  .tabs {
    flex: 1;
    min-height: 80px;
    max-height: 260px;
    overflow: auto;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    padding: 4px;
    display: flex;
    flex-direction: column;
    background: var(--surface-doc);
  }
  .tab {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-areas: "c t" "c u";
    column-gap: 2px;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    border-bottom: 1px solid var(--hairline-soft);
    align-items: center;
  }
  .tab:last-child {
    border-bottom: 0;
  }
  .tab:hover {
    background: var(--surface-soft);
  }
  .tab input {
    grid-area: c;
  }
  .tab .t {
    grid-area: t;
    color: var(--ink);
    font-weight: 500;
  }
  .tab .u {
    grid-area: u;
  }
  .tab.off,
  .tab.off .t {
    color: var(--mute);
  }
  .grid {
    display: grid;
    grid-template-columns: 50px 1fr;
    row-gap: 8px;
    column-gap: 8px;
    align-items: center;
  }
  .picker {
    flex: 1;
    min-width: 0;
  }
</style>
