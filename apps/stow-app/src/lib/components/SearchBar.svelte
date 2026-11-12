<script lang="ts">
  // Full-text search box with a results list. The parent decides what to do
  // with a hit; `onQueryChange` lets it hide the navigation while searching.
  import * as api from "../api";
  import { toast } from "../stores/ui.svelte";
  import type { SearchHit } from "../types";
  import Icon from "./Icon.svelte";

  interface Props {
    onSelect: (hit: SearchHit) => void;
    onQueryChange?: (q: string) => void;
    inputId?: string;
  }
  let { onSelect, onQueryChange, inputId = "stow-search" }: Props = $props();

  let q = $state("");
  let hits = $state<SearchHit[]>([]);
  let searching = $state(false);
  let active = $state(-1);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let seq = 0;

  function onInput(e: Event) {
    q = (e.target as HTMLInputElement).value;
    onQueryChange?.(q);
    if (timer) clearTimeout(timer);
    timer = setTimeout(run, 180);
  }

  async function run() {
    const query = q.trim();
    const mySeq = ++seq;
    if (!query) {
      hits = [];
      searching = false;
      return;
    }
    searching = true;
    try {
      const r = await api.search(query);
      if (mySeq === seq) {
        hits = r;
        active = r.length ? 0 : -1;
      }
    } catch (e) {
      if (mySeq === seq) toast.error(e);
    } finally {
      if (mySeq === seq) searching = false;
    }
  }

  export function clear() {
    q = "";
    hits = [];
    active = -1;
    onQueryChange?.("");
  }

  function pick(h: SearchHit) {
    onSelect(h);
    clear();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      clear();
      (e.target as HTMLInputElement).blur();
    } else if (e.key === "ArrowDown" && hits.length) {
      e.preventDefault();
      active = (active + 1) % hits.length;
    } else if (e.key === "ArrowUp" && hits.length) {
      e.preventDefault();
      active = (active - 1 + hits.length) % hits.length;
    } else if (e.key === "Enter" && active >= 0 && hits[active]) {
      e.preventDefault();
      pick(hits[active]);
    }
  }

  function chipClass(t: string) {
    if (t === "item") return "accent";
    if (t === "bucket") return "success";
    return "";
  }
</script>

<div class="search">
  <div class="field">
    <span class="glyph" aria-hidden="true"><Icon name="magnifying-glass" size={12} /></span>
    <input
      id={inputId}
      type="search"
      placeholder="Search ⌘K"
      aria-label="Search"
      aria-keyshortcuts="Meta+K"
      value={q}
      oninput={onInput}
      onkeydown={onKey}
      autocomplete="off"
      spellcheck="false"
    />
  </div>
  {#if q.trim()}
    <div class="results floating" role="listbox" aria-label="Search results">
      {#if searching && !hits.length}
        <div class="muted small pad">Searching…</div>
      {:else if !hits.length}
        <div class="muted small pad">No results</div>
      {/if}
      {#each hits as h, i (h.doc_type + h.doc_id)}
        <div
          class="hit"
          class:active={i === active}
          role="option"
          aria-selected={i === active}
          tabindex="-1"
          onmouseenter={() => (active = i)}
          onclick={() => pick(h)}
          onkeydown={(e) => e.key === "Enter" && pick(h)}
        >
          <div class="row">
            <span class="chip {chipClass(h.doc_type)}">{h.doc_type.replace(/_/g, " ")}</span>
            <span class="title truncate">{h.title || "(untitled)"}</span>
          </div>
          {#if h.snippet}<div class="snippet muted small">{h.snippet}</div>{/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .search {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .field {
    position: relative;
  }
  .glyph {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--mute);
    font-size: 14px;
    pointer-events: none;
  }
  input {
    width: 100%;
    height: var(--control-h-sm);
    padding-left: 28px;
  }
  input[type="search"]::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }
  .results {
    margin-top: 8px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    padding: 4px;
  }
  .pad {
    padding: 6px 8px;
  }
  .hit {
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    border-bottom: 1px solid var(--hairline-soft);
  }
  .hit:last-child {
    border-bottom: 0;
  }
  .hit.active {
    background: var(--surface-soft);
  }
  .title {
    flex: 1;
    min-width: 0;
    color: var(--ink);
    font-weight: 500;
  }
  .snippet {
    margin-top: 2px;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
