<script lang="ts">
  // Markdown textarea with preview. Autosave is debounced (600 ms) and flushed
  // on unmount and whenever `recordKey` changes, so no edit is ever lost.
  import { onDestroy, untrack } from "svelte";
  import { createAutosave, registerAutosave, type Autosave } from "../autosave";
  import { renderUserMarkdown } from "../markdown";
  import Icon from "./Icon.svelte";

  interface Props {
    value: string;
    /** Receives the key of the record the text belongs to; never resolve it from the current selection. */
    onSave: (recordKey: string, md: string) => void | Promise<unknown>;
    /** Identity of the record being edited, e.g. `item:<id>` / `bucket:<id>`. */
    recordKey: string;
    placeholder?: string;
    minRows?: number;
    /** Accessible name of the textarea. */
    label?: string;
  }
  let { value, onSave, recordKey, placeholder = "Notes (Markdown)…", minRows = 8, label = "Notes" }: Props = $props();

  let text = $state("");
  let preview = $state(false);
  let saving = $state(0);
  let lastSaved = $state("");
  let failed = $state(false);
  let autosave: Autosave<string> | null = null;
  let unregister: (() => void) | null = null;
  let currentKey: string | null = null;
  const dirty = $derived(text !== lastSaved);

  function bind(key: string) {
    // Capture the key and the save function for THIS record so a later flush
    // never writes old text into a newly selected record.
    const save = onSave;
    const a = createAutosave<string>((md) => {
      saving++;
      failed = false;
      // Returned so flush()/flushAll() can await the store's write (never rejects).
      return Promise.resolve(save(key, md))
        .then(() => {
          // Only a resolved save counts as "Saved".
          if (currentKey === key) lastSaved = md;
        })
        .catch(() => {
          if (currentKey === key) failed = true;
        })
        .finally(() => saving--);
    }, 600);
    unregister?.();
    unregister = registerAutosave(a);
    autosave = a;
    currentKey = key;
  }

  $effect(() => {
    const key = recordKey;
    const v = value;
    untrack(() => {
      if (key !== currentKey) {
        autosave?.flush();
        bind(key);
        text = v;
        lastSaved = v;
        failed = false;
        return;
      }
      // External change (another tab appended, metadata landed): adopt it only
      // when nothing local is pending or in flight.
      if (!autosave?.pending && saving === 0 && v !== text) {
        text = v;
        lastSaved = v;
      }
    });
  });

  onDestroy(() => {
    autosave?.flush();
    unregister?.();
    unregister = null;
  });

  function onInput(e: Event) {
    text = (e.target as HTMLTextAreaElement).value;
    autosave?.schedule(text);
  }
  function onBlur() {
    autosave?.flush();
  }
  function togglePreview() {
    if (!preview) autosave?.flush();
    preview = !preview;
  }

  export function flush(): Promise<unknown> | undefined {
    return autosave?.flush();
  }
</script>

<div class="notes">
  <div class="bar">
    <span class="small" class:muted={!failed} class:failed role="status">
      {#if saving}<Icon name="rotate" size={10} /> Saving…{:else if failed}<Icon name="triangle-exclamation" size={10} /> Save failed — edit to retry{:else if dirty}<Icon name="pen" size={10} /> Unsaved{:else}<Icon name="circle-check" size={10} /> Saved{/if}
    </span>
    <span class="spacer"></span>
    <div class="product-tabs" role="group" aria-label="Notes view">
      <button type="button" aria-pressed={!preview} class:active={!preview} onclick={() => preview && togglePreview()}>Edit</button>
      <button type="button" aria-pressed={preview} class:active={preview} onclick={() => !preview && togglePreview()}>Preview</button>
    </div>
  </div>
  {#if preview}
    <div class="md preview">
      {#if text.trim()}
        {@html renderUserMarkdown(text)}
      {:else}
        <span class="muted">Nothing to preview.</span>
      {/if}
    </div>
  {:else}
    <textarea
      aria-label={label}
      value={text}
      {placeholder}
      rows={minRows}
      spellcheck="true"
      oninput={onInput}
      onblur={onBlur}
    ></textarea>
  {/if}
</div>

<style>
  .notes {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .failed {
    color: var(--danger-text);
    font-weight: 600;
  }
  textarea {
    width: 100%;
    min-height: 160px;
    font-family: var(--font-mono);
    font-size: 13px;
    tab-size: 2;
  }
  .preview {
    padding: 16px 20px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-doc);
    min-height: 160px;
  }
</style>
