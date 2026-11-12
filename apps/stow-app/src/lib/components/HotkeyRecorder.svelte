<script lang="ts">
  // "Record shortcut" → "Press keys…": captures one combination (≥1 of ⌃⌥⌘), Esc cancels,
  // ⌫ resets to the default ⌃⌘S. Shows whether the OS accepted the shortcut. When
  // update_settings rejects (code `hotkey`) nothing was saved: `value` stays the old
  // shortcut and the rejection's copy is shown under it.
  import { captureKey, DEFAULT_HOTKEY, hotkeyGlyphs } from "../hotkey";
  import Icon from "./Icon.svelte";

  interface Props {
    value: string;
    /** Apply via update_settings; resolves null when saved, else the error copy to show. */
    onApply: (accelerator: string) => Promise<string | null>;
    registered?: boolean | null;
    error?: string | null;
  }
  let { value, onApply, registered = null, error = null }: Props = $props();

  let recording = $state(false);
  let note = $state<string | null>(null);
  /** Rejection from the last apply (the previous shortcut is still active). */
  let applyError = $state<string | null>(null);
  let busy = $state(false);
  let btn = $state<HTMLButtonElement | null>(null);

  function start() {
    recording = true;
    note = null;
    applyError = null;
  }

  async function apply(acc: string) {
    recording = false;
    if (acc === value) return;
    busy = true;
    applyError = null;
    try {
      applyError = await onApply(acc);
    } finally {
      busy = false;
      btn?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const r = captureKey(e);
    if (r.kind === "cancel") {
      recording = false;
      note = null;
    } else if (r.kind === "reset") void apply(DEFAULT_HOTKEY);
    else if (r.kind === "accelerator") void apply(r.value);
    else if (r.reason === "needs-modifier") note = "Add ⌃, ⌥ or ⌘ to the key.";
  }
</script>

<div class="recorder">
  <div class="row">
    <kbd class="current" aria-label="Current shortcut">{hotkeyGlyphs(value)}</kbd>
    <button
      type="button"
      class="small"
      class:rec={recording}
      bind:this={btn}
      disabled={busy}
      aria-live="polite"
      onclick={() => (recording ? (recording = false) : start())}
      onkeydown={onKey}
      onblur={() => (recording = false)}
    >
      {#if recording}<Icon name="keyboard" size={12} /> Press keys…{:else}Record shortcut{/if}
    </button>
    {#if value !== DEFAULT_HOTKEY}
      <button type="button" class="small ghost" disabled={busy} onclick={() => apply(DEFAULT_HOTKEY)}>Use {hotkeyGlyphs(DEFAULT_HOTKEY)}</button>
    {/if}
  </div>
  {#if recording}
    <div class="small muted">Press the new combination. Esc cancels, ⌫ resets to {hotkeyGlyphs(DEFAULT_HOTKEY)}.</div>
  {/if}
  {#if note}<div class="small warn" role="status">{note}</div>{/if}
  {#if applyError}
    <div class="small warn" role="alert"><Icon name="triangle-exclamation" size={11} /> {applyError} Still using {hotkeyGlyphs(value)}.</div>
  {:else if registered === false}
    <div class="small warn" role="alert">
      <Icon name="triangle-exclamation" size={11} /> Couldn't use {hotkeyGlyphs(value)} — another app already uses it. Pick a different shortcut.
      {#if error}<details class="err-details"><summary>Details</summary><pre>{error}</pre></details>{/if}
    </div>
  {:else if registered === true}
    <div class="small ok-text"><Icon name="check" size={11} /> Works everywhere on this Mac</div>
  {/if}
</div>

<style>
  .recorder {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .current {
    font-size: 13px;
    padding: 3px 8px;
    min-width: 52px;
    text-align: center;
  }
  .rec {
    border-color: var(--accent-blue);
    background: var(--accent-blue-soft);
  }
  .warn {
    color: var(--warning-text);
  }
</style>
